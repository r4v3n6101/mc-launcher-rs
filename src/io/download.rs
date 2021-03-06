use std::{
    fmt::Debug,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use reqwest::{Client, IntoUrl};
use tokio::{
    fs::{create_dir_all, File},
    io::{AsyncWriteExt, BufWriter},
};
use tracing::{debug, instrument, trace};

#[derive(Debug, Default)]
pub struct Manager {
    client: Client,
    downloaded_bytes: Arc<AtomicU64>,
}

impl Clone for Manager {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            downloaded_bytes: Arc::clone(&self.downloaded_bytes),
        }
    }
}

impl Manager {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            downloaded_bytes: Default::default(),
        }
    }

    pub fn reset(&self) {
        self.downloaded_bytes.store(0, Ordering::Relaxed);
    }

    pub fn downloaded_bytes(&self) -> u64 {
        self.downloaded_bytes.load(Ordering::Relaxed)
    }

    #[instrument]
    pub async fn download_file<U, P>(&self, url: U, path: P) -> crate::Result<()>
    where
        U: IntoUrl + Debug,
        P: AsRef<Path> + Debug,
    {
        const BUF_SIZE: usize = 1024 * 1024; //  1mb

        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            create_dir_all(parent).await?;
        }
        let file = File::create(&path).await?;
        let mut output = BufWriter::with_capacity(BUF_SIZE, file);
        let mut response = self.client.get(url).send().await?;
        debug!(?response, "Remote responded");
        while let Some(chunk) = response.chunk().await? {
            let len = chunk.len();
            trace!(len, "New chunk arrived");
            output.write_all(&chunk).await?;
            self.downloaded_bytes
                .fetch_add(len as u64, Ordering::Relaxed);
        }
        output.flush().await?;

        Ok(())
    }
}
