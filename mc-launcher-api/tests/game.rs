use mc_launcher_api::{
    file::GameRepository,
    metadata::{game::VersionInfo, manifest::VersionsManifest},
    resources::{fetch_manifest, fetch_version_info},
};
use reqwest::Client;
use tracing::{info_span, subscriber, trace, Instrument};
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

    let metadata = info_span!("acquire_metadata");
    let client = Client::new();
    let version: VersionInfo = async {
        let manifest: VersionsManifest = fetch_manifest(&client).await.unwrap();
        trace!(?manifest);
        let last_release = manifest.latest_release().unwrap();
        fetch_version_info(&client, &last_release).await.unwrap()
    }
    .instrument(metadata)
    .await;

    trace!(?version);

    let download = info_span!("download_latest_release");
    async {
        let mut file_storage = GameRepository::with_default_hierarchy(env!("OUT_DIR"), &version);
        file_storage.pull_invalid(32, false).await.unwrap();
    }
    .instrument(download)
    .await;

    opentelemetry::global::shutdown_tracer_provider();
}
