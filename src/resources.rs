use reqwest::Client;
use url::Url;

use crate::metadata::{assets::AssetMetadata, manifest::VersionsManifest};

pub static VERSIONS_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest.json";
pub static RESOURCE_REGISTRY_URL: &str = "http://resources.download.minecraft.net";

pub async fn fetch_manifest(client: &Client) -> crate::Result<VersionsManifest> {
    Ok(client
        .get(VERSIONS_MANIFEST_URL)
        .send()
        .await?
        .json()
        .await?)
}

pub fn get_asset_url(asset_metadata: &AssetMetadata) -> Url {
    Url::parse(&format!(
        "{}/{}/{}",
        RESOURCE_REGISTRY_URL,
        &asset_metadata.hash[..2],
        &asset_metadata.hash
    ))
    .unwrap()
}
