use chrono::{DateTime, Utc};
use serde_derive::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseType {
    Release,
    Snapshot,
    OldAlpha,
    OldBeta,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: ReleaseType,
    pub url: String,
    pub time: DateTime<Utc>,
    pub release_time: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
pub struct LatestInfo {
    pub release: String,
    pub snapshot: String,
}

#[derive(Deserialize, Debug)]
pub struct VersionsManifest {
    pub latest: LatestInfo,
    pub versions: Vec<VersionInfo>,
}

#[cfg(test)]
mod tests {

    use super::VersionsManifest;

    #[tokio::test]
    async fn print_versions_manifest() {
        let manifest: VersionsManifest = reqwest::get(crate::VERSIONS_MANIFEST_URL)
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        println!("{:?}", manifest);
    }
}
