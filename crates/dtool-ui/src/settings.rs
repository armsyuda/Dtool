pub mod filler;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolbarVisibility {
    pub top: bool,
    pub context: bool,
    pub tools: bool,
}

impl Default for ToolbarVisibility {
    fn default() -> Self {
        Self { top: true, context: true, tools: true }
    }
}

pub(crate) enum ToolbarArea {
    Top,
    Context,
    Tools,
}

impl ToolbarArea {
    pub(crate) fn from_menu_id(id: &str) -> Option<Self> {
        match id {
            "view.top_toolbar.show" => Some(Self::Top),
            "view.context_toolbar.show" => Some(Self::Context),
            "view.tools.show" => Some(Self::Tools),
            _ => None,
        }
    }

    pub(crate) fn preference<'a>(&self, visibility: &'a mut ToolbarVisibility) -> &'a mut bool {
        match self {
            Self::Top => &mut visibility.top,
            Self::Context => &mut visibility.context,
            Self::Tools => &mut visibility.tools,
        }
    }

    pub(crate) fn checked(&self, mut visibility: ToolbarVisibility) -> bool {
        *self.preference(&mut visibility)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub background_grey: f32,
    pub ui_brightness: f32,
    pub text_contrast: f32,
    pub high_contrast: bool,
    pub tool_handle_size: f32,
    pub status_bar_visible: bool,
    pub toolbars: ToolbarVisibility,
    pub tabs_show_zoom: bool,
    pub tabs_show_format: bool,
    pub tabs_show_size: bool,
    pub tabs_show_preview: bool,
    pub language: String,
    pub allow_ai: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        UiSettings {
            background_grey: 0.25,
            ui_brightness: 0.5,
            text_contrast: 0.5,
            high_contrast: false,
            tool_handle_size: 1.0,
            status_bar_visible: true,
            toolbars: ToolbarVisibility::default(),
            tabs_show_zoom: true,
            tabs_show_format: false,
            tabs_show_size: false,
            tabs_show_preview: false,
            language: "ko".to_string(),
            allow_ai: true,
        }
    }
}

impl UiSettings {
    pub fn load(path: &Path) -> UiSettings {
        match fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => UiSettings::default(),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        dtool_export::write_atomic(path, json.as_bytes())
    }

    pub fn default_path() -> PathBuf {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            PathBuf::from(appdata).join("DTool").join("ui_settings.json")
        } else {
            PathBuf::from("ui_settings.json")
        }
    }
}
