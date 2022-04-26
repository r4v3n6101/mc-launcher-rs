use std::{
    fmt::Debug,
    io::{self, Cursor},
    path::PathBuf,
    sync::Arc,
};

use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::Client;
use tokio::{
    fs::{self, create_dir_all, File},
    io::{AsyncWrite, AsyncWriteExt, BufWriter},
    task,
};
use tracing::{debug, info, info_span, instrument, trace, Instrument};
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
struct FileIndex {
    metadata: RemoteMetadata,
    location: PathBuf,
}

impl FileIndex {
    #[instrument]
    async fn fetch(&self, client: &Client, invalidate: bool) -> crate::Result<()> {
        const BUF_SIZE: usize = 1024 * 1024; //  1mb

        if invalidate || !self.location.exists() {
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

pub struct GameRepository {
    client: Client,

    root_dir: PathBuf,
    assets_dir: PathBuf,
    libraries_dir: PathBuf,
    logs_dir: PathBuf,
    version_dir: PathBuf,
    natives_dir: PathBuf,

    asset_index: FileIndex,
    client_bin: FileIndex,
    log_config: Option<FileIndex>,
    libraries: Vec<FileIndex>,
    natives: Vec<RemoteMetadata>,
}

impl GameRepository {
    pub fn new(
        client: Client,
        assets_dir: PathBuf,
        libraries_dir: PathBuf,
        logs_dir: PathBuf,
        version_dir: PathBuf,
        natives_dir: PathBuf,
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
            // TODO : Filter by rules
            .filter_map(|lib| lib.resources.artifact.as_ref())
            .map(|artifact| FileIndex {
                metadata: RemoteMetadata::from(&artifact.resource),
                location: libraries_dir.join(&artifact.path),
            })
            .collect();
        let natives = version
            .libraries
            .iter()
            // TODO : Filter by rules
            .filter_map(|lib| lib.resources.get_native_for_os())
            .map(|artifact| RemoteMetadata::from(&artifact.resource))
            .collect();
        Self {
            client,

            root_dir,
            assets_dir,
            libraries_dir,
            logs_dir,
            version_dir,
            natives_dir,

            asset_index,
            client_bin,
            log_config,
            libraries,
            natives,
        }
    }

    pub fn with_default_hierarchy(client: Client, version: &VersionInfo) -> Self {
        let root_dir = dirs::data_dir()
            .map(|data| data.join("minecraft"))
            .or_else(|| dirs::home_dir().map(|home| home.join(".minecraft")))
            .expect("neither home nor data dirs found");
        let assets_dir = root_dir.join("assets/");
        let libraries_dir = root_dir.join("libraries/");
        let logs_dir = root_dir.join("logs/");
        let version_dir = root_dir.join(format!("versions/{}", &version.id));
        let natives_dir = version_dir.join("natives/");
        Self::new(
            client,
            assets_dir,
            libraries_dir,
            logs_dir,
            version_dir,
            natives_dir,
            root_dir,
            version,
        )
    }

    fn track_asset_objects(&self, asset_index: &AssetIndex) -> Vec<FileIndex> {
        let is_legacy_assets = asset_index.map_to_resources.unwrap_or(false);
        asset_index
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
                        format!("virtual/legacy/{}", path)
                    } else {
                        format!("object/{}", metadata.hashed_id())
                    }),
                },
            )
            .collect()
    }

    #[instrument(skip(self))]
    async fn fetch_assets(&self, concurrency: usize, invalidate: bool) -> crate::Result<()> {
        let invalidate = invalidate || !self.assets_dir.exists();
        self.asset_index.fetch(&self.client, invalidate).await?;
        let filebuf = fs::read(&self.asset_index.location).await?;
        let asset_index: AssetIndex = serde_json::from_slice(&filebuf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let asset_objects = self.track_asset_objects(&asset_index);
        stream::iter(asset_objects.iter())
            .map(Ok)
            .try_for_each_concurrent(concurrency, |index| index.fetch(&self.client, invalidate))
            .await
    }

    #[instrument(skip(self))]
    async fn fetch_libraries(&self, concurrency: usize, invalidate: bool) -> crate::Result<()> {
        let invalidate = invalidate || !self.libraries_dir.exists();
        stream::iter(self.libraries.iter())
            .map(Ok)
            .try_for_each_concurrent(concurrency, |index| index.fetch(&self.client, invalidate))
            .await
    }

    #[instrument(skip(self))]
    async fn fetch_natives(&self) -> crate::Result<()> {
        for native_metadata in &self.natives {
            let mut filebuf = Vec::with_capacity(native_metadata.size);
            download(&self.client, &native_metadata.location, &mut filebuf).await?;
            let natives_dir = self.natives_dir.clone();
            // TODO : span here
            task::spawn_blocking(move || {
                let _span = info_span!("unzip").entered();
                let mut cursor = Cursor::new(filebuf);
                let mut native_artifact = ZipArchive::new(&mut cursor)?;
                native_artifact.extract(natives_dir)
            })
            .await??;
        }
        Ok(())
    }

    #[instrument(skip(self))]
    async fn fetch_bins(&self, invalidate: bool) -> crate::Result<()> {
        self.client_bin.fetch(&self.client, invalidate).await?;
        if invalidate || !self.version_dir.exists() || !self.natives_dir.exists() {
            self.fetch_natives().await?;
        }
        Ok(())
    }

    #[instrument(skip(self))]
    async fn fetch_log_config(&self, invalidate: bool) -> crate::Result<()> {
        if let Some(index) = &self.log_config {
            index.fetch(&self.client, invalidate).await?;
        }
        Ok(())
    }

    // TODO : rempve invalidate flag and change to check
    #[instrument(skip(self))]
    pub async fn fetch_all(
        self: Arc<Self>,
        concurrency: usize,
        invalidate: bool,
    ) -> crate::Result<()> {
        let invalidate = invalidate || !self.root_dir.exists();

        // avg ratio of assets and libraries file sizes are 8, so assets should be 8 times concurrently
        let assets_task = {
            let selfie = Arc::clone(&self);
            task::spawn(
                async move { selfie.fetch_assets(concurrency * 8, invalidate).await }
                    .in_current_span(),
            )
        };
        let libraries_task = {
            let selfie = Arc::clone(&self);
            task::spawn(
                async move { selfie.fetch_libraries(concurrency, invalidate).await }
                    .in_current_span(),
            )
        };
        let client_task = {
            let selfie = Arc::clone(&self);
            task::spawn(async move { selfie.fetch_bins(invalidate).await }.in_current_span())
        };
        let log_config = {
            let selfie = Arc::clone(&self);
            task::spawn(async move { selfie.fetch_log_config(invalidate).await }.in_current_span())
        };

        assets_task.await??;
        libraries_task.await??;
        client_task.await??;
        log_config.await??;

        Ok(())
    }
}
