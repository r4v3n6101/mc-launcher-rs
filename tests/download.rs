use mcl_rs::{
    file::GameRepository,
    resources::{fetch_manifest, fetch_version_info},
};
use reqwest::Client;
use tracing::{info_span, subscriber, Instrument};
use tracing_subscriber::{layer::SubscriberExt, Registry};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_latest_release() {
    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_service_name("mc-launcher-api")
        .install_simple()
        .unwrap();
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(telemetry);
    subscriber::set_global_default(subscriber).unwrap();

    let client = Client::new();

    let manifest = fetch_manifest(&client).await.unwrap();
    let last_release = manifest.latest_release().unwrap();
    let version = fetch_version_info(&client, &last_release).await.unwrap();

    let download = info_span!("download_latest_release");
    async {
        let mut file_storage = GameRepository::with_default_location_and_client(&version);

        // Assets are small, so more concurrent task will be efficient. Libraries are big and not
        // efficient to processing with a lot of tasks as they will wait each other to download's
        // end.
        file_storage.fetch_all(128, 16, false).await.unwrap();
    }
    .instrument(download)
    .await;

    opentelemetry::global::shutdown_tracer_provider();
}
