use std::{collections::HashMap, path::PathBuf};

use serde_derive::Deserialize;

#[derive(Deserialize, Debug)]
pub struct AssetMetadata {
    pub hash: String,
    pub size: usize,
}

#[derive(Deserialize, Debug)]
pub struct AssetIndex {
    pub map_to_resources: Option<bool>,
    pub objects: HashMap<String, AssetMetadata>,
}

impl AssetMetadata {
    pub fn hashed_id(&self) -> String {
        format!("{}/{}", self.hash, &self.hash[..2])
    }
}

impl AssetIndex {
    pub fn iter_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        let old_format = self.map_to_resources.unwrap_or(false);
        self.objects.iter().map(move |(path, metadata)| {
            let mut path_buf = PathBuf::new();
            if old_format {
                path_buf.push(path);
            } else {
                path_buf.push(metadata.hashed_id());
            }
            path_buf
        })
    }
}
