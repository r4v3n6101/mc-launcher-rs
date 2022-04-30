use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    iter,
    path::Path,
};

use tokio::process::{Child, Command};
use tracing::{instrument, trace};

use crate::{file::Hierarchy, metadata::game::VersionInfo};

fn substitute_arg<'a>(arg: &'a str, params: &'a HashMap<&str, &OsStr>) -> Cow<'a, OsStr> {
    if let Some(i) = arg.find("${") {
        if let Some(j) = arg[i..].find('}') {
            let replacement = params
                .get(&arg[i + 2..i + j])
                .copied()
                .unwrap_or_else(|| OsStr::new(""));
            let mut output = OsString::new();
            output.push(OsStr::new(&arg[..i]));
            output.push(replacement);
            output.push(OsStr::new(&arg[i + j + 1..]));
            return Cow::Owned(output);
        }
    }
    Cow::Borrowed(OsStr::new(arg))
}

pub struct GameProcess<'a> {
    cwd: &'a Path,
    jvm_args: Vec<&'a str>,
    game_args: Vec<&'a str>,
    main_class: &'a str,
}

impl<'a> GameProcess<'a> {
    pub fn new<'b: 'a>(
        cwd: &'a Path,
        version: &'a VersionInfo,
        features: &'b HashMap<&str, bool>,
    ) -> Self {
        let jvm_args = version.arguments.iter_jvm_args(features).collect();
        let game_args = version.arguments.iter_game_args(features).collect();

        Self {
            cwd,
            jvm_args,
            game_args,
            main_class: &version.main_class,
        }
    }

    #[instrument(skip(self))]
    pub fn spawn_game(&self, params: &HashMap<&str, &OsStr>) -> crate::Result<Child> {
        let jvm_args = self.jvm_args.iter().map(|arg| substitute_arg(arg, params));
        let game_args = self.game_args.iter().map(|arg| substitute_arg(arg, params));
        // TODO : replace java
        let mut command = Command::new("java");
        command.current_dir(self.cwd);
        command.args(jvm_args);
        command.arg(OsStr::new(&self.main_class));
        command.args(game_args);
        trace!(?command);

        Ok(command.spawn()?)
    }

    pub fn spawn_with_default_params(
        &self,
        version: &'a VersionInfo,
        username: &'a str,
        hierarchy: &'a Hierarchy,
    ) -> crate::Result<Child> {
        const LAUNCHER_NAME: &str = env!("CARGO_PKG_NAME");
        const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

        let classpath = env::join_paths(
            version
                .libraries
                .iter()
                .filter_map(|lib| {
                    if lib.is_supported_by_rules() {
                        lib.resources.artifact.as_ref()
                    } else {
                        None
                    }
                })
                .map(|artifact| hierarchy.libraries_dir.join(&artifact.path))
                .chain(iter::once(hierarchy.version_dir.join("client.jar"))),
        )
        .expect("idk");
        // TODO : remove expect to error
        let mut params: HashMap<&str, &OsStr> = HashMap::new();
        params.insert("classpath", &classpath);
        params.insert("natives_directory", hierarchy.natives_dir.as_os_str());
        params.insert("game_directory", self.cwd.as_os_str());
        params.insert("assets_root", hierarchy.assets_dir.as_os_str());
        params.insert("version_name", version.id.as_ref());
        params.insert("assets_index_name", version.assets.as_ref());
        params.insert("launcher_name", LAUNCHER_NAME.as_ref());
        params.insert("launcher_version", LAUNCHER_VERSION.as_ref());
        params.insert("auth_player_name", username.as_ref());
        // TODO : and so on

        self.spawn_game(&params)
    }
}
