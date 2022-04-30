use std::collections::HashMap;

use mcl_rs::{
    download::Manager,
    file::{game::Repository, Hierarchy},
    process::GameCommand,
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

    let client = Client::new();

    let manifest = fetch_manifest(&client).await.unwrap();
    let last_release = manifest.latest_release().unwrap();
    let version = fetch_version_info(&client, &last_release).await.unwrap();

    let download = info_span!("download_latest_release");
    let file_hierarchy = Hierarchy::with_default_structure(&version.id);
    async {
        let mut repository = Repository::new(Manager::default());
        repository.track_libraries(
            file_hierarchy.libraries_dir.as_path(),
            file_hierarchy.natives_dir.as_path(),
            &version,
        );
        repository.track_client(file_hierarchy.version_dir.as_path(), &version);
        repository
            .track_asset_objects(file_hierarchy.assets_dir.as_path(), &version)
            .await
            .unwrap();
        repository.pull_indices(512).await.unwrap();
        assert_eq!(repository.pulled_indices(), repository.indices());
    }
    .instrument(download)
    .await;

    let features = HashMap::new();
    let process = GameCommand::new(
        &file_hierarchy.gamedir,
        "java".as_ref(),
        &version,
        &features,
    );
    process
        .build_with_default_params(&file_hierarchy, &version, "test")
        .output()
        .await
        .unwrap();
    opentelemetry::global::shutdown_tracer_provider();
}
