use std::{
    fmt::Debug,
    io::Cursor,
    path::{Path, PathBuf},
};

use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::Client;
use sha1::{Digest, Sha1};
use tokio::{
    fs::{create_dir_all, File},
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, BufWriter},
    task,
};
use tracing::{debug, info, instrument, trace};
use zip::ZipArchive;

use crate::{
    metadata::{
        assets::{AssetIndex, AssetMetadata},
        game::{Resource, VersionInfo},
    },
    resources::get_asset_url,
};

#[instrument(skip(writer))]
async fn download<W: AsyncWrite + Unpin>(
    client: &Client,
    url: impl AsRef<str> + Debug,
    writer: &mut W,
) -> crate::Result<()> {
    let mut response = client.get(url.as_ref()).send().await?;
    debug!(?response, "Remote responded");
    while let Some(chunk) = response.chunk().await? {
        trace!(len = chunk.len(), "New chunk arrived");
        writer.write_all(&chunk).await?;
    }
    Ok(())
}

#[derive(Debug)]
struct RemoteMetadata {
    location: String,
    sha1: String,
    size: usize,
}

impl From<&Resource> for RemoteMetadata {
    fn from(res: &Resource) -> Self {
        Self {
            location: res.url.clone(),
            sha1: res.sha1.clone(),
            size: res.size,
        }
    }
}

#[derive(Debug)]
struct Index {
    metadata: RemoteMetadata,
    location: PathBuf,
}

impl Index {
    #[instrument]
    async fn is_match_to_remote(&self) -> crate::Result<bool> {
        let mut file = File::open(&self.location).await?;

        let metadata = file.metadata().await?;
        let remote_size = self.metadata.size;
        let local_size = metadata.len();
        if local_size != remote_size as u64 {
            return Ok(false);
        }

        let remote_sha1 = &self.metadata.sha1;
        let local_sha1 = &hex::encode({
            let mut filebuf = Vec::with_capacity(remote_size);
            file.read_to_end(&mut filebuf).await?;

            task::spawn_blocking(|| {
                let mut sha1 = Sha1::new();
                sha1.update(filebuf);
                sha1.finalize()
            })
            .await?
        });
        if local_sha1 != remote_sha1 {
            return Ok(false);
        }

        Ok(true)
    }

    #[instrument]
    async fn pull(&self, client: &Client, validate: bool) -> crate::Result<()> {
        const BUF_SIZE: usize = 1024 * 1024; //  1mb

        if !self.location.exists() || (validate && !self.is_match_to_remote().await?) {
            if let Some(parent) = self.location.parent() {
                create_dir_all(parent).await?;
            }
            let file = File::create(&self.location).await?;
            let mut output = BufWriter::with_capacity(BUF_SIZE, file);
            download(client, &self.metadata.location, &mut output).await?;
            output.flush().await?;
            info!("File downloaded");
        } else {
            info!("File already exists");
        }

        Ok(())
    }
}

pub struct Repository {
    client: Client,
    indices: Vec<Index>,
    /// they are treated as `indices`, but it's special case with zip archives
    natives_indices: Vec<RemoteMetadata>,
    natives_dir: PathBuf,
}

impl Repository {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            indices: vec![],
            natives_indices: vec![],
            natives_dir: PathBuf::new(),
        }
    }

    pub fn track_version_info(
        client: Client,
        assets_dir: &Path,
        libraries_dir: &Path,
        version_dir: &Path,
        natives_dir: &Path,
        version: &VersionInfo,
    ) -> Self {
        let mut indices = Vec::new();
        indices.push(Index {
            metadata: RemoteMetadata::from(&version.asset_index.resource),
            location: assets_dir.join(format!("indexes/{}.json", &version.asset_index.id)),
        });
        indices.push(Index {
            metadata: RemoteMetadata::from(&version.downloads.client),
            location: version_dir.join("client.jar"),
        });
        if let Some(logging) = &version.logging {
            indices.push(Index {
                metadata: RemoteMetadata::from(&logging.client.config.resource),
                location: version_dir.join(&logging.client.config.id),
            });
        }
        indices.extend(
            version
                .libraries
                .iter()
                .filter_map(|lib| {
                    if lib.is_supported_by_rules() {
                        lib.resources.artifact.as_ref()
                    } else {
                        None
                    }
                })
                .map(|artifact| Index {
                    metadata: RemoteMetadata::from(&artifact.resource),
                    location: libraries_dir.join(&artifact.path),
                }),
        );
        // Corner case where we can't store it like usual indices
        // TODO : external method with unpacking
        let natives_indices = version
            .libraries
            .iter()
            .filter_map(|lib| {
                if lib.is_supported_by_rules() {
                    lib.resources.get_native_for_os()
                } else {
                    None
                }
            })
            .map(|artifact| RemoteMetadata::from(&artifact.resource))
            .collect();
        Self {
            client,
            indices,
            natives_indices,
            natives_dir: natives_dir.to_path_buf(),
        }
    }

    pub fn track_asset_index(client: Client, assets_dir: &Path, asset_index: &AssetIndex) -> Self {
        let is_legacy_assets = asset_index.map_to_resources.unwrap_or(false);
        let indices = asset_index
            .objects
            .iter()
            .map(|(path, metadata @ AssetMetadata { hash, size })| Index {
                metadata: RemoteMetadata {
                    location: get_asset_url(metadata),
                    sha1: hash.clone(),
                    size: *size,
                },
                location: assets_dir.join(if is_legacy_assets {
                    format!("virtual/legacy/{}", path)
                } else {
                    format!("object/{}", metadata.hashed_id())
                }),
            })
            .collect();
        Self {
            client,
            indices,
            natives_indices: vec![],
            natives_dir: PathBuf::new(),
        }
    }

    #[instrument(skip(self))]
    pub async fn pull_files(&self, concurrency: usize, validate: bool) -> crate::Result<()> {
        stream::iter(self.indices.iter())
            .map(Ok)
            .try_for_each_concurrent(concurrency, |index| index.pull(&self.client, validate))
            .await?;
        if validate || !self.natives_dir.exists() {
            for native_metadata in &self.natives_indices {
                let mut filebuf = Vec::with_capacity(native_metadata.size);
                download(&self.client, &native_metadata.location, &mut filebuf).await?;
                let natives_dir = self.natives_dir.clone();
                // TODO : span here
                task::spawn_blocking(move || {
                    let mut cursor = Cursor::new(filebuf);
                    let mut native_artifact = ZipArchive::new(&mut cursor)?;
                    native_artifact.extract(natives_dir)
                })
                .await??;
            }
        }
        Ok(())
    }
}
