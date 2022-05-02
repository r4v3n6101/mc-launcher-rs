use std::{
    fmt::Debug,
    io::{self, Cursor},
    path::PathBuf,
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
    },
    resources::get_asset_url,
};

use super::file::Hierarchy;

#[derive(Debug)]
struct RemoteMetadata {
    url: Url,
    size: u64,
}

impl From<&Resource> for RemoteMetadata {
    fn from(res: &Resource) -> Self {
        Self {
            url: res.url.clone(),
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
    async fn validate(&self) -> crate::Result<bool> {
        if !self.local_path.exists() {
            return Ok(false);
        }

        let metadata = fs::metadata(&self.local_path).await?;
        if metadata.len() != self.metadata.size {
            return Ok(false);
        }

        Ok(true)
    }
    #[instrument]
    async fn pull(&self, downloader: &Manager) -> crate::Result<()> {
        downloader
            .download_file(self.metadata.url.clone(), &self.local_path)
            .await?;
        if let IndexType::NativeArtifact { extract_dir } = &self.itype {
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
        Ok(())
    }
}

pub struct Repository {
    downloader: Manager,
    info: VersionInfo,
    indices: Vec<Index>,
    tracked: Vec<usize>,
}

impl Repository {
    pub async fn fetch(client: Client, hierarchy: &Hierarchy, remote: Url) -> crate::Result<Self> {
        let downloader = Manager::new(client);
        let info_path = hierarchy.version_dir.join("info.json");
        if !info_path.exists() {
            downloader.download_file(remote, &info_path).await?;
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
                    size: *size,
                },
                local_path: hierarchy.assets_dir.join(if is_legacy_assets {
                    format!("virtual/legacy/{}", path)
                } else {
                    // TODO : may be panic
                    format!("objects/{}/{}", &hash[..2], &hash)
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
            info,
            indices,
            tracked: vec![],
        })
    }

    pub fn downloader(&self) -> &Manager {
        &self.downloader
    }

    pub fn version_info(&self) -> &VersionInfo {
        &self.info
    }

    pub fn tracked_size(&self) -> u64 {
        self.indices.iter().map(|index| index.metadata.size).sum()
    }

    #[instrument(skip(self))]
    pub async fn track_invalid(&mut self) -> crate::Result<()> {
        for (i, index) in self.indices.iter().enumerate() {
            if !index.validate().await? {
                self.tracked.push(i);
            }
        }
        Ok(())
    }

    #[instrument(skip(self))]
    pub fn track_all(&mut self) {
        self.tracked = (0..self.indices.len()).collect();
    }

    #[instrument(skip(self))]
    pub async fn pull_tracked(&self, concurrency: usize) -> crate::Result<()> {
        stream::iter(self.indices.iter())
            .map(Ok)
            .try_for_each_concurrent(concurrency, |index| index.pull(&self.downloader))
            .await
    }
}
