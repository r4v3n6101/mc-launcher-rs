use futures::{stream, StreamExt};
use mc_launcher_api::metadata::{
    game::GameInfo, manifest::VersionsManifest, resources::VERSIONS_MANIFEST_URL,
};

#[tokio::test]
async fn print_all_game_infos() {
    let manifest: VersionsManifest = reqwest::get(VERSIONS_MANIFEST_URL)
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    stream::iter(manifest.versions)
        .for_each_concurrent(32, |version_info| async move {
            println!(
                "{:?}",
                reqwest::get(version_info.url)
                    .await
                    .unwrap()
                    .json::<GameInfo>()
                    .await
                    .map_err(|e| eprintln!("Cannot parse {} because of {}", version_info.id, e))
                    .unwrap()
            );
        })
        .await;
}
