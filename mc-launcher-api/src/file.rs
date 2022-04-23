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

use crate::metadata::game::{
    LibraryResource, LibraryResources, LoggerConfig, Resource, VersionInfo,
};

#[derive(Debug)]
enum FileType {
    Asset,
    Artifact,
    NativeArtifact,
    Client,
    Log,
}

#[derive(Debug)]
struct FileIndex {
    remote_location: String,
    remote_size: usize,
    remote_sha1: String,
    location: PathBuf,
    ftype: FileType,
}

impl FileIndex {
    #[instrument]
    async fn validate(&mut self) -> crate::Result<bool> {
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
    async fn pull(&mut self) -> crate::Result<()> {
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
}

impl GameRepository {
    #[instrument(skip_all)]
    pub fn with_default_hierarchy(
        root_dir: impl AsRef<Path> + Debug,
        version: &VersionInfo,
    ) -> Self {
        let root_dir: Box<Path> = root_dir.as_ref().into();
        let versions_dir = root_dir.join("versions/").join(&version.id);
        let libraries_dir = root_dir.join("libraries/");
        let logs_dir = root_dir.join("logs/");
        let _assets_dir = root_dir.join("assets/");

        let libraries = version
            .libraries
            .iter()
            // TODO : Filter by rules and inspect name mb
            .inspect(|library| trace!(?library))
            .map(|lib| &lib.resources)
            .filter_map(|LibraryResources { artifact, other }| {
                let lib_res_to_index = |&LibraryResource {
                                            resource:
                                                Resource {
                                                    ref sha1,
                                                    ref size,
                                                    ref url,
                                                },
                                            ref path,
                                        },
                                        ftype| FileIndex {
                    location: libraries_dir.join(&path),
                    remote_location: url.clone(),
                    remote_size: *size,
                    remote_sha1: sha1.clone(),
                    ftype,
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
            .inspect(|resource| trace!(?resource))
            .map(|Resource { sha1, url, size }| FileIndex {
                location: versions_dir.join("client.jar"),
                remote_location: url.clone(),
                remote_size: *size,
                remote_sha1: sha1.clone(),
                ftype: FileType::Client,
            });

        let log_config = version
            .logging
            .iter()
            .inspect(|logging| trace!(?logging))
            .map(|l| &l.client.config)
            .map(
                |LoggerConfig {
                     resource: Resource { sha1, size, url },
                     id,
                 }| FileIndex {
                    location: logs_dir.join(&id),
                    remote_location: url.clone(),
                    remote_size: *size,
                    remote_sha1: sha1.clone(),
                    ftype: FileType::Log,
                },
            );
        // TODO : assets

        let indices = client.chain(libraries).chain(log_config).collect();

        Self { root_dir, indices }
    }

    #[instrument(skip(self))]
    pub async fn pull_invalid(
        &mut self,
        concurrency: usize,
        invalidate_all: bool,
    ) -> crate::Result<()> {
        stream::iter(&mut self.indices)
            .map(Ok)
            .try_filter_map(|game_file| async {
                if invalidate_all || !game_file.validate().await? {
                    Ok(Some(game_file))
                } else {
                    Ok(None)
                }
            })
            .try_for_each_concurrent(concurrency, FileIndex::pull)
            .await
    }
}
