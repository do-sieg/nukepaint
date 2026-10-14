//! Per-user preferences, kept in a TOML file in the user's config folder.

use std::{fs, path::{Path, PathBuf}};

use serde::{Deserialize, Serialize};

const MAX_RECENT_FILES: usize = 10;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Folder the file dialogs start in.
    last_dir: Option<PathBuf>,
    /// Files last opened, most recent first.
    recent_files: Vec<PathBuf>,
}

impl Settings {
    /// Reads the settings, or starts from the defaults if there are none or they cannot be read.
    pub fn load() -> Self {
        file_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let Some(path) = file_path() else {
            return;
        };
        let result = toml::to_string(self)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                if let Some(dir) = path.parent() {
                    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
                }
                fs::write(&path, text).map_err(|error| error.to_string())
            });
        if let Err(error) = result {
            eprintln!("Could not save the settings to {}: {error}", path.display());
        }
    }

    pub fn last_dir(&self) -> Option<&Path> {
        self.last_dir.as_deref()
    }

    pub fn set_last_dir(&mut self, dir: &Path) {
        self.last_dir = Some(dir.to_path_buf());
        self.save();
    }

    pub fn recent_files(&self) -> &[PathBuf] {
        &self.recent_files
    }

    /// Moves the file to the top of the recent list.
    pub fn add_recent_file(&mut self, path: &Path) {
        self.recent_files.retain(|recent| recent != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(MAX_RECENT_FILES);
        self.save();
    }

    pub fn remove_recent_file(&mut self, path: &Path) {
        self.recent_files.retain(|recent| recent != path);
        self.save();
    }

    pub fn clear_recent_files(&mut self) {
        self.recent_files.clear();
        self.save();
    }
}

fn file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "power-paint").map(|dirs| dirs.config_dir().join("settings.toml"))
}
