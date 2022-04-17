use serde_derive::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Deserialize, Debug)]
pub struct OsFilter {
    pub name: Option<String>,
    pub version: Option<String>,
    pub arch: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Rule {
    pub action: RuleAction,
    pub os: Option<OsFilter>,
}
