use std::collections::HashMap;

use mcl_rs::{
    io::{file::Hierarchy, sync::Repository},
    process::GameCommand,
    resources::fetch_manifest,
};
use reqwest::Client;
use tokio::process::Command;
use tracing::subscriber;
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

    let file_hierarchy = Hierarchy::with_default_structure(&last_release.id);

    let repository = Repository::fetch(client, file_hierarchy, &last_release)
        .await
        .unwrap();
    repository.pull(512).await.unwrap();

    let features = HashMap::new();
    let command = GameCommand::new(
        &repository.hierarchy().gamedir,
        "java".as_ref(),
        &repository.version_info(),
        &features,
    );
    Command::from(command.build_with_default_params(
        &repository.hierarchy(),
        &repository.version_info(),
        "test",
    ))
    .spawn()
    .unwrap()
    .wait()
    .await
    .unwrap();

    opentelemetry::global::shutdown_tracer_provider();
}
