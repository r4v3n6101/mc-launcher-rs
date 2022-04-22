use std::{io, iter, path::Path};

use futures_util::{stream, StreamExt, TryStreamExt};
use tokio::{
    fs::{create_dir_all, File, OpenOptions},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{debug, instrument, trace};

use crate::metadata::game::{GameInfo, LibraryResource, LibraryResources, Resource};

#[derive(Debug)]
enum FileType {
    Asset,
    Native,
    Artifact,
    Binary,
    Log,
}

#[derive(Debug)]
struct FileMetadata {
    remote_location: String,
    remote_size: usize,
    remote_sha1: String,
    location: Box<Path>,
    file_type: FileType,
}

#[derive(Debug)]
struct GameFile {
    metadata: FileMetadata,
    file: Option<File>,
}

impl GameFile {
    #[instrument]
    async fn init(metadata: FileMetadata) -> crate::Result<Self> {
        let path = &metadata.location;
        let file = match OpenOptions::new().write(true).read(true).open(path).await {
            Ok(file) => {
                debug!(?path, "GameFile already exists");
                Some(file)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                debug!(?path, "GameFile not exists");
                None
            }
            Err(e) => return Err(e.into()),
        };
        Ok(Self { metadata, file })
    }

    #[instrument]
    async fn pull(&mut self) -> crate::Result<()> {
        const BUF_SIZE: usize = 32 * 1024; // 32kb

        let file = match self.file.as_mut() {
            Some(file) => file,
            None => {
                let path = &self.metadata.location;
                if let Some(parent) = path.parent() {
                    create_dir_all(parent).await?;
                }
                let file = OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .open(path)
                    .await?;

                debug!(?path, "Created new GameFile");
                self.file.insert(file)
            }
        };
        let mut output = BufWriter::with_capacity(BUF_SIZE, file);
        let mut response = reqwest::get(&self.metadata.remote_location).await?;
        debug!(?response, "Remote GameFile responded");
        while let Some(chunk) = response.chunk().await? {
            trace!(len = chunk.len(), "New chunk arrived");
            output.write_all(&chunk).await?;
        }
        output.flush().await?;
        Ok(())
    }
}

pub struct FileStorage {
    root_dir: Box<Path>,
    files: Vec<GameFile>,
}

impl FileStorage {
    #[instrument(skip(root_dir))]
    pub async fn with_default_hierarchy(
        root_dir: impl AsRef<Path>,
        game_info: &GameInfo,
    ) -> crate::Result<Self> {
        let root_dir: Box<Path> = root_dir.as_ref().into();
        let bin_dir = root_dir.join("bin/").join(&game_info.id);
        let libs_dir = root_dir.join("libs/");
        let _assets_dir = root_dir.join("assets/");

        let libraries = game_info
            .libraries
            .iter()
            // TODO : Filter by rules and inspect name mb
            .inspect(|lib| debug!(lib = lib.name.as_str(), "Remote library"))
            .map(|lib| &lib.resources)
            .filter_map(|LibraryResources { artifact, other }| {
                let lib_res_to_game_file = |lib_res: &LibraryResource, file_type| FileMetadata {
                    location: libs_dir.join(&lib_res.path).into_boxed_path(),
                    remote_location: lib_res.resource.url.clone(),
                    remote_size: lib_res.resource.size,
                    remote_sha1: lib_res.resource.sha1.clone(),
                    file_type,
                };
                let artifact =
                    iter::once(lib_res_to_game_file(artifact.as_ref()?, FileType::Artifact));
                let other = other
                    .as_ref()?
                    .iter()
                    .map(move |(_, value)| lib_res_to_game_file(value, FileType::Native));
                Some(artifact.chain(other))
            })
            .flatten();

        let binaries = game_info
            .downloads
            .iter()
            .inspect(|(name, _)| debug!(%name, "Remote binary"))
            .map(|(name, Resource { sha1, url, size })| {
                let filename = match name.as_str() {
                    "client" => "client.jar",
                    "client_mappings" => "client_mappings.txt",
                    "server" => "server.jar",
                    "server_mappings" => "server_mappings.txt",
                    _ => "unknown",
                };
                FileMetadata {
                    location: bin_dir.join(filename).into_boxed_path(),
                    remote_location: url.clone(),
                    remote_size: *size,
                    remote_sha1: sha1.clone(),
                    file_type: FileType::Binary,
                }
            });
        // TODO : assets & log

        let files = stream::iter(binaries.chain(libraries))
            .then(GameFile::init)
            .try_collect()
            .await?;

        Ok(Self { root_dir, files })
    }

    #[instrument(skip(self))]
    pub async fn force_pull_all(&mut self, concurrency: usize) -> crate::Result<()> {
        stream::iter(&mut self.files)
            .map(Ok)
            .try_for_each_concurrent(concurrency, GameFile::pull)
            .await
    }
}
