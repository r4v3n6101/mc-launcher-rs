use std::{
    fmt::Debug,
    io,
    path::{Path, PathBuf},
};

use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::Client;
use tokio::{
    fs::{self, create_dir_all, File},
    io::{AsyncWrite, AsyncWriteExt, BufWriter},
};
use tracing::{debug, info, instrument, trace};

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

#[instrument]
async fn download_if_absent(
    client: &Client,
    path: impl AsRef<Path> + Debug,
    url: impl AsRef<str> + Debug,
    force: bool,
) -> crate::Result<()> {
    const BUF_SIZE: usize = 1024 * 1024; //  1mb

    let path = path.as_ref();
    let url = url.as_ref();
    if force || !path.exists() {
        if let Some(parent) = path.parent() {
            create_dir_all(parent).await?;
        }
        let file = File::create(path).await?;
        let mut output = BufWriter::with_capacity(BUF_SIZE, file);
        download(client, url, &mut output).await?;
        output.flush().await?;
        info!(?path, %url, "File downloaded");
    } else {
        info!(?path, "File already exists");
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
struct FileIndex {
    metadata: RemoteMetadata,
    location: PathBuf,
}

impl FileIndex {
    async fn fetch(&self, client: &Client, invalidate: bool) -> crate::Result<()> {
        download_if_absent(client, &self.location, &self.metadata.location, invalidate).await
    }
}

pub struct GameRepository {
    client: Client,

    root_dir: PathBuf,
    assets_dir: PathBuf,
    libraries_dir: PathBuf,
    logs_dir: PathBuf,
    version_dir: PathBuf,

    asset_index: FileIndex,
    client_bin: FileIndex,
    log_config: Option<FileIndex>,
    libraries: Vec<FileIndex>,
    asset_objects: Vec<FileIndex>,
    // natives?
}

impl GameRepository {
    pub fn new(
        client: Client,
        assets_dir: PathBuf,
        libraries_dir: PathBuf,
        logs_dir: PathBuf,
        version_dir: PathBuf,
        root_dir: PathBuf,
        version: &VersionInfo,
    ) -> Self {
        let asset_index = FileIndex {
            metadata: RemoteMetadata::from(&version.asset_index.resource),
            location: assets_dir.join(format!("indexes/{}.json", &version.asset_index.id)),
        };
        let client_bin = FileIndex {
            metadata: RemoteMetadata::from(&version.downloads.client),
            location: version_dir.join("client.jar"),
        };
        let log_config = version.logging.as_ref().map(|logging| FileIndex {
            metadata: RemoteMetadata::from(&logging.client.config.resource),
            location: logs_dir.join(&logging.client.config.id),
        });
        let libraries = version
            .libraries
            .iter()
            // TODO : Filter by rules and inspect name mb
            .filter_map(|lib| lib.resources.artifact.as_ref())
            .map(|artifact| FileIndex {
                metadata: RemoteMetadata::from(&artifact.resource),
                location: libraries_dir.join(&artifact.path),
            })
            .collect();
        Self {
            client,

            root_dir,
            assets_dir,
            libraries_dir,
            logs_dir,
            version_dir,

            asset_index,
            client_bin,
            log_config,
            libraries,
            asset_objects: vec![],
        }
    }

    pub fn with_default_hierarchy(
        client: Client,
        version: &VersionInfo,
        root_dir: PathBuf,
    ) -> Self {
        Self::new(
            client,
            root_dir.join("assets/"),
            root_dir.join("libraries/"),
            root_dir.join("logs/"),
            root_dir.join(format!("versions/{}", &version.id)),
            root_dir,
            version,
        )
    }

    pub fn with_default_location_and_client(version: &VersionInfo) -> Self {
        let root_dir = dirs::data_dir()
            .map(|data| data.join("minecraft"))
            .or_else(|| dirs::home_dir().map(|home| home.join(".minecraft")))
            .expect("neither home nor data dirs found");
        Self::with_default_hierarchy(Client::new(), version, root_dir)
    }

    #[instrument(skip(self))]
    async fn track_asset_objects(&mut self, asset_index: &AssetIndex) -> crate::Result<()> {
        let is_legacy_assets = asset_index.map_to_resources.unwrap_or(false);
        self.asset_objects = asset_index
            .objects
            .iter()
            .map(
                |(path, metadata @ AssetMetadata { hash, size })| FileIndex {
                    metadata: RemoteMetadata {
                        location: get_asset_url(metadata),
                        sha1: hash.clone(),
                        size: *size,
                    },
                    location: self.assets_dir.join(if is_legacy_assets {
                        format!("virtual/legacy/{path}")
                    } else {
                        format!("object/{}", metadata.hashed_id())
                    }),
                },
            )
            .inspect(|index| debug!(?index, "Tracked asset object"))
            .collect();
        Ok(())
    }

    // TODO : check flag for validation
    #[instrument(skip(self))]
    async fn fetch_assets(&mut self, concurrency: usize, invalidate: bool) -> crate::Result<()> {
        self.asset_index.fetch(&self.client, invalidate).await?;
        let filebuf = fs::read(&self.asset_index.location).await?;
        let asset_index: AssetIndex = serde_json::from_slice(&filebuf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.track_asset_objects(&asset_index).await?;
        stream::iter(self.asset_objects.iter())
            .map(Ok)
            .try_for_each_concurrent(concurrency, |index| index.fetch(&self.client, invalidate))
            .await
    }

    #[instrument(skip(self))]
    async fn fetch_libraries(&mut self, concurrency: usize, invalidate: bool) -> crate::Result<()> {
        stream::iter(self.libraries.iter())
            .map(Ok)
            .try_for_each_concurrent(concurrency, |index| index.fetch(&self.client, invalidate))
            .await
    }

    #[instrument(skip(self))]
    async fn fetch_client(&mut self, invalidate: bool) -> crate::Result<()> {
        self.client_bin.fetch(&self.client, invalidate).await
    }

    #[instrument(skip(self))]
    async fn fetch_log_config(&mut self, invalidate: bool) -> crate::Result<()> {
        if let Some(index) = &self.log_config {
            index.fetch(&self.client, invalidate).await?;
        }
        Ok(())
    }

    // concurrency
    #[instrument(skip(self))]
    pub async fn fetch_all(
        &mut self,
        assets_concurrency: usize,
        libraries_concurrency: usize,
        invalidate: bool,
    ) -> crate::Result<()> {
        self.fetch_assets(assets_concurrency, invalidate).await?;
        self.fetch_libraries(libraries_concurrency, invalidate)
            .await?;
        self.fetch_client(invalidate).await?;
        self.fetch_log_config(invalidate).await?;

        Ok(())
    }
}
