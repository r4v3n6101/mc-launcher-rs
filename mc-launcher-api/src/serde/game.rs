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

#[cfg(test)]
mod tests {

    use super::GameInfo;

    const VERSION_22W15A_INFO_URL: &str = "https://launchermeta.mojang.com/v1/packages/884c9042aa5877be1e8e282a4723a7778e1246ab/22w15a.json";

    #[tokio::test]
    async fn print_game_info() {
        let game_info: GameInfo = reqwest::get(VERSION_22W15A_INFO_URL)
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        println!("{:?}", game_info);
    }
}
