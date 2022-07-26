use std::collections::HashMap;

use serde_derive::Deserialize;

fn empty_hash() -> String {
    String::from("00null")
}

#[derive(Deserialize, Debug)]
pub struct AssetMetadata {
    #[serde(default = "empty_hash")]
    pub hash: String,
    pub size: u64,
}

#[derive(Deserialize, Debug)]
pub struct AssetIndex {
    pub map_to_resources: Option<bool>,
    pub objects: HashMap<String, AssetMetadata>,
}
