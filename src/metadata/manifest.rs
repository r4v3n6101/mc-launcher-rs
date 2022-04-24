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
pub struct Version {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: ReleaseType,
    pub url: String,
    pub time: DateTime<Utc>,
    pub release_time: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Deserialize, Debug)]
pub struct VersionsManifest {
    pub latest: Latest,
    pub versions: Vec<Version>,
}

impl VersionsManifest {
    pub fn get_version(&self, id: &str) -> Option<&Version> {
        self.versions
            .iter()
            .find(|version| version.id.eq_ignore_ascii_case(id))
    }

    pub fn latest_release(&self) -> Option<&Version> {
        self.get_version(&self.latest.release)
    }

    pub fn latest_snapshot(&self) -> Option<&Version> {
        self.get_version(&self.latest.snapshot)
    }
}
