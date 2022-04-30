use std::{
    fmt::Debug,
    io::{self, Cursor},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use futures_util::{stream, StreamExt, TryStreamExt};
use tokio::{fs, task};
use tracing::instrument;
use zip::ZipArchive;

use crate::{
    io::download::Manager,
    metadata::{
        assets::{AssetIndex, AssetMetadata},
        game::{Resource, VersionInfo},
    },
    resources::get_asset_url,
};

#[instrument]
async fn validate_file(
    path: impl AsRef<Path> + Debug,
    expected_sha1: &str,
    expected_size: u64,
) -> crate::Result<bool> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(false);
    }

    let metadata = fs::metadata(path).await?;
    if metadata.len() != expected_size {
        return Ok(false);
    }

    #[cfg(feature = "sha1")]
    {
        use hex::encode;
        use sha1::{Digest, Sha1};
        let local_sha1 = &encode({
            let filebuf = fs::read(path).await?;
            task::spawn_blocking(|| {
                let mut sha1 = Sha1::new();
                sha1.update(filebuf);
                sha1.finalize()
            })
            .await?
        });
        if local_sha1 != expected_sha1 {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Debug)]
struct RemoteMetadata {
    url: String,
    sha1: String,
    size: u64,
}

impl From<&Resource> for RemoteMetadata {
    fn from(res: &Resource) -> Self {
        Self {
            url: res.url.clone(),
            sha1: res.sha1.clone(),
            size: res.size,
        }
    }
}

#[derive(Debug)]
enum IndexType {
    GameFile { path: PathBuf },
    NativeArtifact { path: PathBuf, natives_dir: PathBuf },
}

#[derive(Debug)]
struct Index {
    metadata: RemoteMetadata,
    itype: IndexType,
}

impl Index {
    #[instrument]
    async fn pull(&self, downloader: &Manager) -> crate::Result<()> {
        match &self.itype {
            IndexType::GameFile { path } => {
                if !validate_file(&path, &self.metadata.sha1, self.metadata.size).await? {
                    downloader.download_file(&self.metadata.url, &path).await?;
                }
            }
            IndexType::NativeArtifact { path, natives_dir } => {
                if !validate_file(&path, &self.metadata.sha1, self.metadata.size).await?
                    || !natives_dir.exists()
                {
                    downloader.download_file(&self.metadata.url, &path).await?;
                    let filebuf = fs::read(&path).await?;
                    let natives_dir = natives_dir.clone();
                    // TODO : span here
                    task::spawn_blocking(move || {
                        let mut cursor = Cursor::new(filebuf);
                        let mut native_artifact = ZipArchive::new(&mut cursor)?;
                        native_artifact.extract(natives_dir)
                    })
                    .await??;
                }
            }
        }
        Ok(())
    }
}

// TODO : lifetime for indices
pub struct Repository {
    downloader: Manager,
    indices: Vec<Index>,
    pulled_indices: AtomicUsize,
}

impl Repository {
    pub fn new(downloader: Manager) -> Self {
        Self {
            downloader,
            indices: vec![],
            pulled_indices: AtomicUsize::new(0),
        }
    }

    pub fn downloader(&self) -> &Manager {
        &self.downloader
    }

    pub fn indices(&self) -> usize {
        self.indices.len()
    }

    pub fn pulled_indices(&self) -> usize {
        self.pulled_indices.load(Ordering::Relaxed)
    }

    pub fn purge(&mut self) {
        self.indices.clear();
    }

    pub async fn track_asset_objects(
        &mut self,
        assets_dir: &Path,
        version: &VersionInfo,
    ) -> crate::Result<()> {
        let asset_index_path = assets_dir.join(format!("indexes/{}.json", version.assets));

        let asset_index = Index {
            metadata: RemoteMetadata::from(&version.asset_index.resource),
            itype: IndexType::GameFile {
                path: asset_index_path.clone(),
            },
        };
        asset_index.pull(&self.downloader).await?;

        let asset_index: AssetIndex = {
            let filebuf = fs::read(&asset_index_path).await?;
            serde_json::from_slice(&filebuf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        };
        let is_legacy_assets = asset_index.map_to_resources.unwrap_or(false);

        for (path, metadata @ AssetMetadata { hash, size }) in &asset_index.objects {
            self.indices.push(Index {
                metadata: RemoteMetadata {
                    url: get_asset_url(metadata),
                    sha1: hash.clone(),
                    size: *size,
                },
                itype: IndexType::GameFile {
                    path: assets_dir.join(if is_legacy_assets {
                        format!("virtual/legacy/{}", path)
                    } else {
                        format!("objects/{}", metadata.hashed_id())
                    }),
                },
            });
        }
        Ok(())
    }

    pub fn track_libraries(
        &mut self,
        libraries_dir: &Path,
        natives_dir: &Path,
        version: &VersionInfo,
    ) {
        for lib in &version.libraries {
            if lib.is_supported_by_rules() {
                let resources = &lib.resources;
                if let Some(artifact) = &resources.artifact {
                    self.indices.push(Index {
                        metadata: RemoteMetadata::from(&artifact.resource),
                        itype: IndexType::GameFile {
                            path: libraries_dir.join(&artifact.path),
                        },
                    });
                }
                if let Some(native_artifact) = resources.get_native_for_os() {
                    self.indices.push(Index {
                        metadata: RemoteMetadata::from(&native_artifact.resource),
                        itype: IndexType::NativeArtifact {
                            path: libraries_dir.join(&native_artifact.path),
                            natives_dir: natives_dir.to_path_buf(),
                        },
                    });
                }
            }
        }
    }

    pub fn track_client(&mut self, version_dir: &Path, version: &VersionInfo) {
        self.indices.push(Index {
            metadata: RemoteMetadata::from(&version.downloads.client),
            itype: IndexType::GameFile {
                path: version_dir.join("client.jar"),
            },
        });
        if let Some(logging) = &version.logging {
            self.indices.push(Index {
                metadata: RemoteMetadata::from(&logging.client.config.resource),
                itype: IndexType::GameFile {
                    path: version_dir.join(&logging.client.config.id),
                },
            });
        }
    }

    #[instrument(skip(self))]
    pub async fn pull_indices(&self, concurrency: usize) -> crate::Result<()> {
        stream::iter(self.indices.iter())
            .map(Ok)
            .try_for_each_concurrent(concurrency, |index| async {
                index.pull(&self.downloader).await?;
                self.pulled_indices.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .await
    }
}
