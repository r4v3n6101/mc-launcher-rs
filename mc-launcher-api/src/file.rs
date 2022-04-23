use std::{
    fmt::Debug,
    io,
    path::{Path, PathBuf},
};

use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::Client;
use tokio::{
    fs::{self, create_dir_all, File},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{debug, info, instrument, trace};

use crate::{
    metadata::{
        assets::{AssetIndex, AssetMetadata},
        game::{LibraryResources, Resource, VersionInfo},
    },
    resources::get_asset_url,
};

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
        let mut response = client.get(url).send().await?;
        debug!(?response, "Remote responded");
        while let Some(chunk) = response.chunk().await? {
            trace!(len = chunk.len(), "New chunk arrived");
            output.write_all(&chunk).await?;
        }
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
    async fn fetch(
        client: &Client,
        metadata: RemoteMetadata,
        location: PathBuf,
        invalidate: bool,
    ) -> crate::Result<Self> {
        download_if_absent(client, &location, &metadata.location, invalidate).await?;
        Ok(Self { metadata, location })
    }
}

pub struct GameRepository {
    client: Client,
    version: VersionInfo,

    root_dir: PathBuf,
    assets_dir: PathBuf,
    libraries_dir: PathBuf,
    logs_dir: PathBuf,
    version_dir: PathBuf,

    asset_index: Option<AssetIndex>,
    log_config: Option<FileIndex>,
    client_bin: Option<FileIndex>,
    asset_objects: Vec<FileIndex>,
    libraries: Vec<FileIndex>,
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
        version: VersionInfo,
    ) -> Self {
        Self {
            client,
            version,

            root_dir,
            assets_dir,
            libraries_dir,
            logs_dir,
            version_dir,

            asset_index: None,
            log_config: None,
            client_bin: None,
            asset_objects: vec![],
            libraries: vec![],
        }
    }

    pub fn with_default_hierarchy(client: Client, version: VersionInfo, root_dir: PathBuf) -> Self {
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

    pub fn with_default_location_and_client(version: VersionInfo) -> Self {
        let root_dir = dirs::data_dir()
            .map(|data| data.join("minecraft"))
            .or_else(|| dirs::home_dir().map(|home| home.join(".minecraft")))
            .expect("neither home nor data dirs found");
        Self::with_default_hierarchy(Client::new(), version, root_dir)
    }

    // TODO : check flag for validation
    #[instrument(skip(self))]
    async fn fetch_assets(&mut self, concurrency: usize, invalidate: bool) -> crate::Result<()> {
        let asset_index = match (&self.asset_index, invalidate) {
            (Some(asset_index), false) => {
                info!("Asset index already present");
                asset_index
            }
            _ => {
                let asset_index_resource = &self.version.asset_index;
                let asset_index = FileIndex::fetch(
                    &self.client,
                    RemoteMetadata::from(&asset_index_resource.resource),
                    self.assets_dir
                        .join(format!("indexes/{}.json", &asset_index_resource.id)),
                    invalidate,
                )
                .await?;

                let filebuf = fs::read(&asset_index.location).await?;
                let asset_index = serde_json::from_slice(&filebuf)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                info!(?asset_index, "Asset index downloaded");
                self.asset_index.insert(asset_index)
            }
        };

        let is_legacy_assets = asset_index.map_to_resources.unwrap_or(false);
        self.asset_objects = stream::iter(
            asset_index
                .objects
                .iter()
                .inspect(|entry| trace!(?entry, "Asset")),
        )
        .map(|(path, metadata @ AssetMetadata { hash, size })| {
            FileIndex::fetch(
                &self.client,
                RemoteMetadata {
                    location: get_asset_url(metadata),
                    sha1: hash.clone(),
                    size: *size,
                },
                self.assets_dir.join(if is_legacy_assets {
                    format!("virtual/legacy/{path}")
                } else {
                    format!("object/{}", metadata.hashed_id())
                }),
                invalidate,
            )
        })
        .buffer_unordered(concurrency)
        .try_collect()
        .await?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn fetch_libraries(&mut self, concurrency: usize, invalidate: bool) -> crate::Result<()> {
        let lib_resources = self
            .version
            .libraries
            .iter()
            // TODO : Filter by rules and inspect name mb
            .inspect(|library| trace!(?library, "Library"))
            .map(|lib| &lib.resources)
            .flat_map(|LibraryResources { artifact, other }| {
                other
                    .iter()
                    .flat_map(|other| other.iter().map(|(_, value)| value))
                    .chain(artifact.iter())
            });
        self.libraries = stream::iter(lib_resources)
            .map(|lib_res| {
                FileIndex::fetch(
                    &self.client,
                    RemoteMetadata::from(&lib_res.resource),
                    self.libraries_dir.join(&lib_res.path),
                    invalidate,
                )
            })
            .buffer_unordered(concurrency)
            .try_collect()
            .await?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn fetch_client(&mut self, invalidate: bool) -> crate::Result<()> {
        let client_resource = self
            .version
            .downloads
            .iter()
            .inspect(|downloads| trace!(?downloads, "Downloads"))
            .find_map(|(name, res)| if name == "client" { Some(res) } else { None });
        self.client_bin = match client_resource {
            Some(client_resource) => Some(
                FileIndex::fetch(
                    &self.client,
                    RemoteMetadata::from(client_resource),
                    self.version_dir.join("client.jar"),
                    invalidate,
                )
                .await?,
            ),
            None => None,
        };

        Ok(())
    }

    #[instrument(skip(self))]
    async fn fetch_log_config(&mut self, invalidate: bool) -> crate::Result<()> {
        let log_config = self
            .version
            .logging
            .as_ref()
            // Unstable: .inspect(|logging| trace!(?logging, "Logging"))
            .map(|logging| &logging.client.config);
        self.log_config = match log_config {
            Some(log_config) => Some(
                FileIndex::fetch(
                    &self.client,
                    RemoteMetadata::from(&log_config.resource),
                    self.logs_dir.join(&log_config.id),
                    invalidate,
                )
                .await?,
            ),
            None => None,
        };

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
