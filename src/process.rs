use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    iter,
    path::Path,
};

use tokio::process::{Child, Command};

use crate::metadata::game::VersionInfo;

pub fn spawn_game<'a>(
    gamedir: &'a Path,
    assets_dir: &'a Path,
    libraries_dir: &'a Path,
    natives_dir: &'a Path,
    version_dir: &'a Path,
    version: &'a VersionInfo,
    features: &HashMap<&str, bool>,
) -> Child {
    fn format_args<'a>(arg: &'a str, params: &'a HashMap<&str, &OsStr>) -> Cow<'a, OsStr> {
        if let Some(i) = arg.find("${") {
            if let Some(j) = arg[i..].find('}') {
                let replacement = params
                    .get(&arg[i + 2..i + j])
                    .copied()
                    .unwrap_or_else(|| OsStr::new("null"));
                let mut output = OsString::new();
                output.push(OsStr::new(&arg[..i]));
                output.push(replacement);
                output.push(OsStr::new(&arg[i + j + 1..]));
                return Cow::Owned(output);
            }
        }
        Cow::Borrowed(OsStr::new(arg))
    }

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
            .map(|artifact| libraries_dir.join(&artifact.path))
            .chain(iter::once(version_dir.join("client.jar"))),
    )
    .expect("idk");
    // TODO : remove expect to error
    //
    // TODO : move out params to args
    let mut params: HashMap<&str, &OsStr> = HashMap::new();
    params.insert("classpath", &classpath);
    params.insert("natives_directory", natives_dir.as_os_str());
    params.insert("game_directory", gamedir.as_os_str());
    params.insert("assets_root", assets_dir.as_os_str());
    params.insert("version_name", OsStr::new(&version.id));
    params.insert("assets_index_name", OsStr::new(&version.assets));

    // TODO : default values if absent
    let jvm_args = version
        .arguments
        .iter_jvm_args(features)
        .map(|arg| format_args(arg, &params));
    let game_args = version
        .arguments
        .iter_game_args(features)
        .map(|arg| format_args(arg, &params));

    // TODO : replace java
    Command::new("java")
        .current_dir(gamedir)
        .args(jvm_args)
        .arg(OsStr::new(&version.main_class))
        .args(game_args)
        .spawn()
        .expect("idc rn")
}
