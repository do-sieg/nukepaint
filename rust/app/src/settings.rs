//! Per-user preferences, kept in a TOML file in the user's config folder.

use std::{fs, path::{Path, PathBuf}};

use serde::{Deserialize, Serialize};

use crate::{checkerboard::Checkerboard, dock::DockLayout, grid::Grid, tools::RightClick, tools_panel::ToolOrder};

const MAX_RECENT_FILES: usize = 10;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Folder the file dialogs start in.
    last_dir: Option<PathBuf>,
    /// Files last opened, most recent first.
    recent_files: Vec<PathBuf>,
    /// What the right button does with the tools that draw in a colour.
    right_click: RightClick,
    /// Whether the drawing tools show the selection's box and the button by it.
    selection_controls: bool,
    /// Last, as they are tables.
    grid: Grid,
    checkerboard: Checkerboard,
    /// Which dock each section is in.
    dock: DockLayout,
    /// The order of the tools within their group.
    tool_order: ToolOrder,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_dir: None,
            recent_files: Vec::new(),
            right_click: RightClick::default(),
            selection_controls: true,
            grid: Grid::default(),
            checkerboard: Checkerboard::default(),
            dock: DockLayout::default(),
            tool_order: ToolOrder::default(),
        }
    }
}

impl Settings {
    /// Reads the settings, or starts from the defaults if there are none or they cannot be read.
    pub fn load() -> Self {
        let mut settings: Self = file_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        settings.dock.normalize();
        settings.tool_order.normalize();
        settings
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

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn set_grid(&mut self, grid: Grid) {
        self.grid = grid;
        self.save();
    }

    pub fn checkerboard(&self) -> &Checkerboard {
        &self.checkerboard
    }

    pub fn set_checkerboard(&mut self, checkerboard: Checkerboard) {
        self.checkerboard = checkerboard;
        self.save();
    }

    pub fn dock(&self) -> &DockLayout {
        &self.dock
    }

    pub fn set_dock(&mut self, dock: DockLayout) {
        self.dock = dock;
        self.save();
    }

    pub fn tool_order(&self) -> &ToolOrder {
        &self.tool_order
    }

    pub fn set_tool_order(&mut self, order: ToolOrder) {
        self.tool_order = order;
        self.save();
    }

    pub fn right_click(&self) -> RightClick {
        self.right_click
    }

    pub fn set_right_click(&mut self, right_click: RightClick) {
        self.right_click = right_click;
        self.save();
    }

    pub fn selection_controls(&self) -> bool {
        self.selection_controls
    }

    pub fn set_selection_controls(&mut self, shown: bool) {
        self.selection_controls = shown;
        self.save();
    }
}

fn file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "power-paint").map(|dirs| dirs.config_dir().join("settings.toml"))
}
