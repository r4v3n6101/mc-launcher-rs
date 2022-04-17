use mc_launcher_api::serde::manifest::VersionsManifest;

const VERSIONS_MANIFEST_URL: &str = "https://launchermeta.mojang.com/mc/game/version_manifest.json";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let manifest: VersionsManifest = reqwest::get(VERSIONS_MANIFEST_URL)
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("{:?}", manifest);
}
