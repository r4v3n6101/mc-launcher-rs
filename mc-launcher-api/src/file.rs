use std::{
    fmt::Debug,
    io, iter,
    path::{Path, PathBuf},
};

use futures_util::{stream, StreamExt, TryStreamExt};
use sha1::{Digest, Sha1};
use tokio::{
    fs::{create_dir_all, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt, BufWriter},
    task,
};
use tracing::{debug, info, instrument, trace, warn};

use crate::{
    metadata::{
        assets::{AssetIndex, AssetMetadata},
        game::{LibraryResource, LibraryResources, LoggerConfig, Resource, VersionInfo},
    },
    resources::get_asset_url,
};

#[derive(Debug, Clone, Copy)]
enum FileType {
    Asset,
    AssetIndex,
    Artifact,
    NativeArtifact,
    Client,
    Log,
}

#[derive(Debug, Clone)]
struct FileIndex {
    remote_location: String,
    remote_size: usize,
    remote_sha1: String,
    location: PathBuf,
    ftype: FileType,
}

impl FileIndex {
    fn from_remote(resource: &Resource, location: PathBuf, ftype: FileType) -> Self {
        Self {
            remote_location: resource.url.clone(),
            remote_size: resource.size,
            remote_sha1: resource.sha1.clone(),
            location,
            ftype,
        }
    }

    #[instrument]
    async fn validate(&self) -> crate::Result<bool> {
        match OpenOptions::new().read(true).open(&self.location).await {
            Ok(mut file) => {
                info!("GameFile already exists");
                let local_size = file.metadata().await?.len();
                let remote_size = self.remote_size;
                if local_size != remote_size as u64 {
                    warn!(local_size, remote_size, "File length mismatch");
                    return Ok(false);
                }

                let mut filebuf = Vec::with_capacity(remote_size);
                file.read_to_end(&mut filebuf).await?;

                let local_sha1 = task::spawn_blocking(move || {
                    hex::encode({
                        let mut hasher = Sha1::default();
                        hasher.update(&filebuf);
                        hasher.finalize()
                    })
                })
                .await?;
                let remote_sha1 = &self.remote_sha1;
                if &local_sha1 != remote_sha1 {
                    warn!(%local_sha1, %remote_sha1, "File sha1sum mismatch");
                    return Ok(false);
                }

                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                warn!("GameFile not exists");
                Ok(false)
            }
            Err(e) => Err(e.into()),
        }
    }

    #[instrument]
    async fn pull(&self) -> crate::Result<()> {
        const BUF_SIZE: usize = 32 * 1024; // 32kb

        let path = &self.location;
        if let Some(parent) = path.parent() {
            create_dir_all(parent).await?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(path)
            .await?;
        let mut output = BufWriter::with_capacity(BUF_SIZE, file);
        let mut response = reqwest::get(&self.remote_location).await?;
        debug!(?response, "Remote GameFile responded");
        while let Some(chunk) = response.chunk().await? {
            trace!(len = chunk.len(), "New chunk arrived");
            output.write_all(&chunk).await?;
        }
        output.flush().await?;
        Ok(())
    }
}

pub struct GameRepository {
    root_dir: Box<Path>,
    indices: Vec<FileIndex>,
    // TODO : arena for separate indices access (not looking all of them)
}

impl GameRepository {
    #[instrument(skip_all)]
    pub fn with_default_hierarchy(
        root_dir: impl AsRef<Path> + Debug,
        version: &VersionInfo,
        asset_index: &AssetIndex,
    ) -> Self {
        let root_dir: Box<Path> = root_dir.as_ref().into();
        let versions_dir = root_dir.join("versions/").join(&version.id);
        let libraries_dir = root_dir.join("libraries/");
        let logs_dir = root_dir.join("logs/");
        let legacy_assets = asset_index.map_to_resources.unwrap_or(false);
        let assets_dir = root_dir.join("assets/");
        let assets_indices_dir = assets_dir.join("indexes/");
        let assets_objects_dir = if legacy_assets {
            assets_dir.join("virtual/legacy/")
        } else {
            assets_dir.join("objects/")
        };

        let libraries = version
            .libraries
            .iter()
            // TODO : Filter by rules and inspect name mb
            .map(|lib| &lib.resources)
            .filter_map(|LibraryResources { artifact, other }| {
                let lib_res_to_index = |&LibraryResource {
                                            ref resource,
                                            ref path,
                                        },
                                        ftype| {
                    FileIndex::from_remote(&resource, libraries_dir.join(&path), ftype)
                };
                let artifact = iter::once(lib_res_to_index(artifact.as_ref()?, FileType::Artifact));
                let other = other
                    .as_ref()?
                    .iter()
                    .map(move |(_, value)| lib_res_to_index(value, FileType::NativeArtifact));
                Some(artifact.chain(other))
            })
            .flatten();

        let client = version
            .downloads
            .iter()
            .filter_map(|(name, res)| if name == "client" { Some(res) } else { None })
            .map(|resource| {
                FileIndex::from_remote(&resource, versions_dir.join("client.dir"), FileType::Client)
            });

        let log_config = version.logging.iter().map(|l| &l.client.config).map(
            |LoggerConfig { resource, id }| {
                FileIndex::from_remote(&resource, logs_dir.join(&id), FileType::Log)
            },
        );

        let assets_index = iter::once(FileIndex::from_remote(
            &version.asset_index.resource,
            assets_indices_dir.join(format!("{}.json", &version.asset_index.id)),
            FileType::AssetIndex,
        ));

        let assets =
            asset_index
                .objects
                .iter()
                .map(
                    |(path, metadata @ AssetMetadata { hash, size })| FileIndex {
                        location: assets_objects_dir.join(if legacy_assets {
                            path.clone()
                        } else {
                            metadata.hashed_id()
                        }),
                        remote_location: get_asset_url(&metadata),
                        remote_size: *size,
                        remote_sha1: hash.clone(),
                        ftype: FileType::Asset,
                    },
                );

        let indices = client
            .chain(libraries)
            .chain(log_config)
            .chain(assets)
            .chain(assets_index)
            .inspect(|index| trace!(?index))
            .collect();

        Self { root_dir, indices }
    }

    #[instrument(skip(self))]
    pub async fn pull_invalid(
        &self,
        concurrency: usize,
        invalidate_all: bool,
    ) -> crate::Result<()> {
        stream::iter(&self.indices)
            .map(Ok)
            .try_filter_map(|index| async move {
                if invalidate_all || !index.validate().await? {
                    Ok(Some(index.clone())) // clone index to pass to tokio as its 'static bound
                } else {
                    Ok(None)
                }
            })
            .try_for_each_concurrent(concurrency, |index| async move {
                task::spawn(async move { index.pull().await })
                    .await
                    .map_err(crate::Error::from)?
            })
            .await
    }
}
