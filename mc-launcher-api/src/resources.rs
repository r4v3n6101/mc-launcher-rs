use reqwest::Client;

use crate::metadata::{
    assets::{AssetIndex, AssetMetadata},
    game::VersionInfo,
    manifest::{Version, VersionsManifest},
};

pub static VERSIONS_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest.json";
pub static RESOURCE_REGISTRY_URL: &str = "http://resources.download.minecraft.net/";

pub async fn fetch_manifest(client: &Client) -> crate::Result<VersionsManifest> {
    Ok(client
        .get(VERSIONS_MANIFEST_URL)
        .send()
        .await?
        .json()
        .await?)
}

pub async fn fetch_version_info(client: &Client, version: &Version) -> crate::Result<VersionInfo> {
    Ok(client.get(&version.url).send().await?.json().await?)
}

pub async fn fetch_asset_index(
    client: &Client,
    version: &VersionInfo,
) -> crate::Result<AssetIndex> {
    Ok(client
        .get(&version.asset_index.resource.url)
        .send()
        .await?
        .json()
        .await?)
}

pub fn get_asset_url(asset_metadata: &AssetMetadata) -> String {
    format!("{}{}", RESOURCE_REGISTRY_URL, asset_metadata.hashed_id())
}
