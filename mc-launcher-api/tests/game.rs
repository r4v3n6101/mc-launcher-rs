use mc_launcher_api::{
    file::GameRepository,
    resources::{fetch_asset_index, fetch_manifest, fetch_version_info},
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
    let asset_index = fetch_asset_index(&client, &version).await.unwrap();

    let download = info_span!("download_latest_release");
    async {
        let file_storage =
            GameRepository::with_default_hierarchy(env!("OUT_DIR"), &version, &asset_index);
        file_storage.pull_invalid(32, false).await.unwrap();
    }
    .instrument(download)
    .await;

    opentelemetry::global::shutdown_tracer_provider();
}
