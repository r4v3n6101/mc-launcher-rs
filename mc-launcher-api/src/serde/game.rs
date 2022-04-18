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
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: ReleaseType,
    pub minimum_launcher_version: usize,
    pub release_time: DateTime<Utc>,
    pub time: DateTime<Utc>,
    pub downloads: HashMap<String, FileDescription>,
    pub asset_index: AssetIndex,
    pub assets: String,
    pub main_class: String,
    #[serde(flatten)]
    pub arguments: Arguments,
    // TODO : libraries
    pub java_version: Option<JavaVersion>,
    pub logging: Option<Logging>,
    pub compliance_level: Option<usize>,
}
