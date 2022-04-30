use std::path::PathBuf;

pub mod game;

pub struct Hierarchy {
    pub gamedir: PathBuf,
    pub assets_dir: PathBuf,
    pub libraries_dir: PathBuf,
    pub version_dir: PathBuf,
    pub natives_dir: PathBuf,
}

impl Hierarchy {
    pub fn with_default_structure(id: &str) -> Self {
        let gamedir = dirs::data_dir()
            .map(|data| data.join("minecraft"))
            .or_else(|| dirs::home_dir().map(|home| home.join(".minecraft")))
            .expect("neither home nor data dirs found");
        let assets_dir = gamedir.join("assets/");
        let libraries_dir = gamedir.join("libraries/");
        let version_dir = gamedir.join(format!("versions/{}", id));
        let natives_dir = version_dir.join("natives/");

        Self {
            gamedir,
            assets_dir,
            libraries_dir,
            version_dir,
            natives_dir,
        }
    }
}
