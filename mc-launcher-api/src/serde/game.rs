use super::{manifest::ReleaseType, rule::Rule};
use chrono::{DateTime, Utc};
use serde_derive::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Argument {
    Plain { value: String },
    OsSpecific { value: String, rules: Vec<Rule> },
}

#[derive(Deserialize, Debug)]
pub struct Arguments {
    pub game: Vec<Argument>,
    pub jvm: Vec<Argument>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: usize,
    pub total_size: usize,
    pub url: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: usize,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub arguments: Arguments,
    pub asset_index: AssetIndex,
    pub assets: String,
    pub compliance_level: usize,
    // TODO : downloads
    pub id: String,
    pub java_version: JavaVersion,
    // TODO : libraries
    // TODO : logging
    pub main_class: String,
    pub minimum_launcher_version: usize,
    pub release_time: DateTime<Utc>,
    pub time: DateTime<Utc>,
    #[serde(rename = "type")]
    pub release_type: ReleaseType,
}
