use std::{sync::Arc, time::Duration};

use mcl_rs::{
    file::GameRepository,
    resources::{fetch_manifest, fetch_version_info},
};
use reqwest::Client;
use tracing::{info_span, subscriber, Instrument};
use tracing_subscriber::{layer::SubscriberExt, Registry};

#[tokio::test]
async fn download_latest_release() {
    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_service_name("mc-launcher-api")
        .install_simple()
        .unwrap();
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(telemetry);
    subscriber::set_global_default(subscriber).unwrap();

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let manifest = fetch_manifest(&client).await.unwrap();
    let last_release = manifest.latest_release().unwrap();
    let version = fetch_version_info(&client, &last_release).await.unwrap();

    let download = info_span!("download_latest_release");
    async {
        let file_storage = Arc::new(GameRepository::with_default_hierarchy(client, &version));

        // Assets are small, so more concurrent task will be efficient. Libraries are big and not
        // efficient to processing with a lot of tasks as they will wait each other to download's
        // end. That's why fetch_all will multiply concurrency for assets
        file_storage.fetch_all(32, false).await.unwrap();
    }
    .instrument(download)
    .await;

    opentelemetry::global::shutdown_tracer_provider();
}
