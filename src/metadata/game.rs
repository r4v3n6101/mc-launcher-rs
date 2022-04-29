use std::{collections::HashMap, env::consts, iter};

use chrono::{DateTime, Utc};
use serde_derive::Deserialize;
use serde_with::{serde_as, OneOrMany, SpaceSeparator, StringWithSeparator};

use super::manifest::ReleaseType;

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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
pub struct Rules(Vec<Rule>);

#[serde_as]
#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    RuleSpecific {
        #[serde_as(deserialize_as = "OneOrMany<_>")]
        value: Vec<String>,
        rules: Rules,
    },
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub enum Arguments {
    #[serde(rename = "arguments")]
    Modern {
        game: Vec<Argument>,
        jvm: Vec<Argument>,
    },
    #[serde(rename = "minecraftArguments")]
    Legacy(#[serde_as(as = "StringWithSeparator::<SpaceSeparator, String>")] Vec<String>),
}

#[derive(Deserialize, Debug)]
pub struct Resource {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexResource {
    #[serde(flatten)]
    pub resource: Resource,
    pub id: String,
    pub total_size: u64,
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
    pub rules: Option<Rules>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: usize,
}

#[derive(Deserialize, Debug)]
pub struct Downloads {
    pub client: Resource,
    pub server: Option<Resource>,
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
    pub downloads: Downloads,
    pub asset_index: AssetIndexResource,
    pub assets: String,
    pub main_class: String,
    #[serde(flatten)]
    pub arguments: Arguments,

    pub java_version: Option<JavaVersion>,
    pub logging: Option<Logging>,
    pub compliance_level: Option<usize>,
}

impl RuleAction {
    pub fn value(self) -> bool {
        match self {
            Self::Allow => true,
            Self::Disallow => false,
        }
    }

    pub fn invert(self) -> Self {
        match self {
            Self::Allow => Self::Disallow,
            Self::Disallow => Self::Allow,
        }
    }

    pub fn chain(self, other: Self) -> Self {
        match (self, other) {
            (Self::Allow, Self::Allow) => Self::Allow,
            _ => Self::Disallow,
        }
    }
}

impl Rule {
    fn calculate_action(&self, params: &HashMap<&str, bool>) -> RuleAction {
        if let Some(os) = &self.os {
            if let Some(name) = &os.name {
                if name != consts::OS {
                    return self.action.invert();
                }
            }
            if let Some(arch) = &os.arch {
                if arch != consts::ARCH {
                    return self.action.invert();
                }
            }
            if let Some(version) = &os.version {
                // TODO: version parsing using crate
            }
        }
        if let Some(features) = &self.features {
            for (k, v) in features.iter() {
                if params.get(k.as_str()).unwrap_or(&false) != v {
                    return self.action.invert();
                }
            }
        }
        self.action
    }

    pub fn is_allowed(&self, params: &HashMap<&str, bool>) -> bool {
        self.calculate_action(params) == RuleAction::Allow
    }
}

impl Rules {
    pub fn is_allowed(&self, params: &HashMap<&str, bool>) -> bool {
        self.0.iter().any(|rule| rule.is_allowed(params))
    }
}

impl Argument {
    pub fn iter_strings<'a, 'b: 'a>(
        &'a self,
        features: &'b HashMap<&'b str, bool>,
    ) -> Box<dyn Iterator<Item = &'a str> + 'a> {
        match self {
            Self::Plain(s) => Box::new(iter::once(s.as_str())),
            Self::RuleSpecific { value, rules } => {
                if rules.is_allowed(features) {
                    Box::new(value.iter().map(String::as_str))
                } else {
                    Box::new(iter::empty())
                }
            }
        }
    }
}

impl Arguments {
    pub fn iter_jvm_args<'a, 'b: 'a>(
        &'a self,
        params: &'b HashMap<&'b str, bool>,
    ) -> Box<dyn Iterator<Item = &'a str> + 'a> {
        match self {
            Self::Modern { jvm, .. } => Box::new(
                jvm.iter()
                    .flat_map(|argument| argument.iter_strings(params)),
            ),
            Self::Legacy(_) => Box::new(iter::empty()),
        }
    }

    pub fn iter_game_args<'a, 'b: 'a>(
        &'a self,
        params: &'b HashMap<&'b str, bool>,
    ) -> Box<dyn Iterator<Item = &'a str> + 'a> {
        match self {
            Self::Modern { game, .. } => Box::new(
                game.iter()
                    .flat_map(|argument| argument.iter_strings(params)),
            ),
            Self::Legacy(s) => Box::new(s.iter().map(String::as_str)),
        }
    }
}

impl Library {
    pub fn is_supported_by_rules(&self) -> bool {
        self.rules
            .as_ref()
            .map(|rules| rules.is_allowed(&HashMap::new()))
            .unwrap_or(false)
    }
}

impl LibraryResources {
    pub fn get_native_for_os(&self) -> Option<&LibraryResource> {
        let native_str: &'static str = match consts::OS {
            "macos" if consts::ARCH == "aarch64" => "natives-macos-arm64",
            "linux" => "natives-linux",
            "windows" => "natives-windows",
            "macos" => "natives-macos",
            _ => panic!("unsupported target"),
        };
        self.other.as_ref().and_then(|other| other.get(native_str))
    }
}
