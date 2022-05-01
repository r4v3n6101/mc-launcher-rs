use std::{
    fmt::Debug,
    io::{self, Cursor},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::Client;
use tokio::{fs, task};
use tracing::instrument;
use url::Url;
use zip::ZipArchive;

use crate::{
    io::download::Manager,
    metadata::{
        assets::{AssetIndex, AssetMetadata},
        game::{Resource, VersionInfo},
        manifest::Version,
    },
    resources::get_asset_url,
};

use super::file::Hierarchy;

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
    url: Url,
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
    GameFile,
    NativeArtifact { extract_dir: PathBuf },
}

#[derive(Debug)]
struct Index {
    metadata: RemoteMetadata,
    local_path: PathBuf,
    itype: IndexType,
}

impl Index {
    #[instrument]
    async fn pull(&self, downloader: &Manager) -> crate::Result<()> {
        if !validate_file(&self.local_path, &self.metadata.sha1, self.metadata.size).await? {
            downloader
                .download_file(self.metadata.url.clone(), &self.local_path)
                .await?;
        }
        if let IndexType::NativeArtifact { extract_dir } = &self.itype {
            if !extract_dir.exists() {
                let filebuf = fs::read(&self.local_path).await?;
                let extract_dir = extract_dir.clone();
                // TODO : span here
                task::spawn_blocking(move || {
                    let mut cursor = Cursor::new(filebuf);
                    let mut native_artifact = ZipArchive::new(&mut cursor)?;
                    native_artifact.extract(extract_dir)
                })
                .await??;
            }
        }
        Ok(())
    }
}

pub struct Repository {
    downloader: Manager,
    hierarchy: Hierarchy,
    info: VersionInfo,

    indices: Vec<Index>,
    pulled_indices: AtomicUsize,
}

impl Repository {
    // TODO : remove version if not exists (no internet connection)
    // TODO : do not move hierarchy
    pub async fn fetch(
        client: Client,
        hierarchy: Hierarchy,
        version: &Version,
    ) -> crate::Result<Self> {
        let downloader = Manager::new(client);
        let info_path = hierarchy.version_dir.join("info.json");
        if !info_path.exists() {
            downloader
                .download_file(version.url.clone(), &info_path)
                .await?;
        }
        let info: VersionInfo = {
            let filebuf = fs::read(&info_path).await?;
            serde_json::from_slice(&filebuf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        };

        let asset_index_path = hierarchy
            .assets_dir
            .join(format!("indexes/{}.json", info.assets));
        let asset_index = Index {
            metadata: RemoteMetadata::from(&info.asset_index.resource),
            local_path: asset_index_path.clone(),
            itype: IndexType::GameFile,
        };
        asset_index.pull(&downloader).await?;
        let asset_index: AssetIndex = {
            let filebuf = fs::read(&asset_index_path).await?;
            serde_json::from_slice(&filebuf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        };

        // should be 'nuff
        let mut indices = Vec::with_capacity(asset_index.objects.len() + info.libraries.len() + 2);

        // assets
        let is_legacy_assets = asset_index.map_to_resources.unwrap_or(false);
        for (path, metadata @ AssetMetadata { hash, size }) in &asset_index.objects {
            indices.push(Index {
                metadata: RemoteMetadata {
                    url: get_asset_url(metadata),
                    sha1: hash.clone(),
                    size: *size,
                },
                local_path: hierarchy.assets_dir.join(if is_legacy_assets {
                    format!("virtual/legacy/{}", path)
                } else {
                    format!("objects/{}", metadata.hashed_id())
                }),
                itype: IndexType::GameFile,
            });
        }

        // libraries
        for lib in &info.libraries {
            if lib.is_supported_by_rules() {
                let resources = &lib.resources;
                if let Some(artifact) = &resources.artifact {
                    indices.push(Index {
                        metadata: RemoteMetadata::from(&artifact.resource),
                        local_path: hierarchy.libraries_dir.join(&artifact.path),
                        itype: IndexType::GameFile,
                    });
                }
                if let Some(native_artifact) = resources.get_native_for_os() {
                    indices.push(Index {
                        metadata: RemoteMetadata::from(&native_artifact.resource),
                        local_path: hierarchy.libraries_dir.join(&native_artifact.path),
                        itype: IndexType::NativeArtifact {
                            extract_dir: hierarchy.natives_dir.to_path_buf(),
                        },
                    });
                }
            }
        }

        // client and other
        indices.push(Index {
            metadata: RemoteMetadata::from(&info.downloads.client),
            local_path: hierarchy.version_dir.join("client.jar"),
            itype: IndexType::GameFile,
        });
        if let Some(logging) = &info.logging {
            indices.push(Index {
                metadata: RemoteMetadata::from(&logging.client.config.resource),
                local_path: hierarchy.version_dir.join(&logging.client.config.id),
                itype: IndexType::GameFile,
            });
        }

        Ok(Self {
            downloader,
            hierarchy,
            info,
            indices,
            pulled_indices: AtomicUsize::new(0),
        })
    }

    pub fn downloader(&self) -> &Manager {
        &self.downloader
    }

    pub fn hierarchy(&self) -> &Hierarchy {
        &self.hierarchy
    }

    pub fn version_info(&self) -> &VersionInfo {
        &self.info
    }

    pub fn indices(&self) -> usize {
        self.indices.len()
    }

    pub fn pulled_indices(&self) -> usize {
        self.pulled_indices.load(Ordering::Relaxed)
    }

    #[instrument(skip(self))]
    pub async fn pull(&self, concurrency: usize) -> crate::Result<()> {
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
