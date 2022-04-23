use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_derive::Deserialize;

use super::manifest::ReleaseType;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Deserialize, Debug)]
pub struct OsDescription {
    pub name: Option<String>,
    pub version: Option<String>,
    pub arch: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Rule {
    pub action: RuleAction,
    pub os: Option<OsDescription>,
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum ArgumentValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    RuleSpecific {
        value: ArgumentValue,
        rules: Vec<Rule>,
    },
}

#[derive(Deserialize, Debug)]
pub enum Arguments {
    #[serde(rename = "arguments")]
    Modern {
        game: Vec<Argument>,
        jvm: Vec<Argument>,
    },
    #[serde(rename = "minecraftArguments")]
    Legacy(String),
}

#[derive(Deserialize, Debug)]
pub struct Resource {
    pub sha1: String,
    pub size: usize,
    pub url: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexResource {
    #[serde(flatten)]
    pub resource: Resource,
    pub id: String,
    pub total_size: usize,
}

#[derive(Deserialize, Debug)]
pub struct LoggerConfig {
    #[serde(flatten)]
    pub resource: Resource,
    pub id: String,
}

#[derive(Deserialize, Debug)]
pub struct LoggerDescription {
    pub argument: String,
    #[serde(rename = "type")]
    pub log_type: String,
    #[serde(rename = "file")]
    pub config: LoggerConfig,
}

#[derive(Deserialize, Debug)]
pub struct Logging {
    pub client: LoggerDescription,
}

#[derive(Deserialize, Debug)]
pub struct LibraryResource {
    #[serde(flatten)]
    pub resource: Resource,
    pub path: String,
}

#[derive(Deserialize, Debug)]
pub struct LibraryResources {
    pub artifact: Option<LibraryResource>,
    #[serde(rename = "classifiers")]
    pub other: Option<HashMap<String, LibraryResource>>,
}

#[derive(Deserialize, Debug)]
pub struct Library {
    #[serde(rename = "downloads")]
    pub resources: LibraryResources,
    pub name: String,
    pub rules: Option<Vec<Rule>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: usize,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: ReleaseType,
    pub minimum_launcher_version: usize,
    pub release_time: DateTime<Utc>,
    pub time: DateTime<Utc>,
    pub libraries: Vec<Library>,
    pub downloads: HashMap<String, Resource>,
    pub asset_index: AssetIndexResource,
    pub assets: String,
    pub main_class: String,
    #[serde(flatten)]
    pub arguments: Arguments,

    pub java_version: Option<JavaVersion>,
    pub logging: Option<Logging>,
    pub compliance_level: Option<usize>,
}

// TODO : chain arguments util
// TODO : check rule
//
