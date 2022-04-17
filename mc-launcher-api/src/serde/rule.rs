use std::collections::HashMap;

use serde_derive::Deserialize;

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
