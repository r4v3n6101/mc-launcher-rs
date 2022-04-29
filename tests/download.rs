use std::collections::HashMap;

use mcl_rs::{
    download::Manager,
    file::Repository,
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
    let gamedir = dirs::data_dir()
        .map(|data| data.join("minecraft"))
        .or_else(|| dirs::home_dir().map(|home| home.join(".minecraft")))
        .expect("neither home nor data dirs found");
    let assets_dir = gamedir.join("assets/");
    let libraries_dir = gamedir.join("libraries/");
    let version_dir = gamedir.join(format!("versions/{}", &version.id));
    let natives_dir = version_dir.join("natives/");
    async {
        let mut repository = Repository::new(Manager::default());
        repository.track_libraries(libraries_dir.as_path(), natives_dir.as_path(), &version);
        repository.track_client(version_dir.as_path(), &version);
        repository
            .track_asset_objects(assets_dir.as_path(), &version)
            .await
            .unwrap();
        repository.pull_indices(512).await.unwrap();
        assert_eq!(repository.pulled_indices(), repository.indices());
    }
    .instrument(download)
    .await;

    let features = HashMap::new();
    let jvm_args = version
        .arguments
        .iter_jvm_args(&features)
        .collect::<Vec<_>>()
        .join(" ");
    let game_args = version
        .arguments
        .iter_game_args(&features)
        .collect::<Vec<_>>()
        .join(" ");

    println!("JVM args: {}", jvm_args);
    println!("Game args: {}", game_args);

    opentelemetry::global::shutdown_tracer_provider();
}
