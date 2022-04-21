use mc_launcher_api::{
    game::FileStorage,
    metadata::{game::GameInfo, manifest::VersionsManifest, resources::VERSIONS_MANIFEST_URL},
};
use tracing::subscriber;
use tracing_subscriber::{layer::SubscriberExt, Registry};

#[tokio::test]
async fn download_latest_release() {
    let tracer = opentelemetry_jaeger::new_pipeline()
        .install_simple()
        .unwrap();
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(telemetry);
    subscriber::set_global_default(subscriber).unwrap();

    let manifest: VersionsManifest = reqwest::get(VERSIONS_MANIFEST_URL)
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let last_release = manifest
        .versions
        .iter()
        .find(|vinfo| vinfo.id == manifest.latest.release)
        .unwrap();
    let game_info: GameInfo = reqwest::get(&last_release.url)
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut file_storage = FileStorage::create_with_default_hierarchy(env!("OUT_DIR"), &game_info)
        .await
        .unwrap();
    file_storage.force_pull_all(32).await.unwrap();

    opentelemetry::global::shutdown_tracer_provider();
}
