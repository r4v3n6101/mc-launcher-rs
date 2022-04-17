use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_derive::Deserialize;

use super::{manifest::ReleaseType, rule::Rule};

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
pub struct Arguments {
    pub game: Vec<Argument>,
    pub jvm: Vec<Argument>,
}

#[derive(Deserialize, Debug)]
pub struct FileDescription {
    pub sha1: String,
    pub size: usize,
    pub url: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    #[serde(flatten)]
    pub file_description: FileDescription,
    pub id: String,
    pub total_size: usize,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: usize,
}

#[derive(Deserialize, Debug)]
pub struct LoggerConfig {
    #[serde(flatten)]
    pub file_description: FileDescription,
    pub id: String,
}

#[derive(Deserialize, Debug)]
pub struct LoggingConfig {
    pub argument: String,
    #[serde(rename = "file")]
    pub config: LoggerConfig,
    #[serde(rename = "type")]
    pub log_type: String,
}

#[derive(Deserialize, Debug)]
pub struct LoggingInfo {
    #[serde(rename = "client")]
    pub client_config: LoggingConfig,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub arguments: Option<Arguments>,
    pub asset_index: AssetIndex,
    pub assets: String,
    pub compliance_level: Option<usize>,
    pub downloads: HashMap<String, FileDescription>,
    pub id: String,
    pub java_version: Option<JavaVersion>,
    // TODO : libraries
    pub logging_info: Option<LoggingInfo>,
    pub main_class: String,
    pub minecraft_arguments: Option<String>,
    pub minimum_launcher_version: usize,
    pub release_time: DateTime<Utc>,
    pub time: DateTime<Utc>,
    #[serde(rename = "type")]
    pub release_type: ReleaseType,
}
