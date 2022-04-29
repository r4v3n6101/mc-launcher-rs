use std::collections::HashMap;

use serde_derive::Deserialize;

#[derive(Deserialize, Debug)]
pub struct AssetMetadata {
    pub hash: String,
    pub size: u64,
}

#[derive(Deserialize, Debug)]
pub struct AssetIndex {
    pub map_to_resources: Option<bool>,
    pub objects: HashMap<String, AssetMetadata>,
}

impl AssetMetadata {
    pub fn hashed_id(&self) -> String {
        format!("{}/{}", &self.hash[..2], self.hash)
    }
}
