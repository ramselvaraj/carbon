//! Carbon's own settings and per-vault session state.
//! Lives in `~/.config/carbon`; the vault's `.obsidian` folder is only ever read.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    /// Styled source with markup hidden away from the caret line.
    Live,
    /// Raw markdown with syntax colouring.
    Source,
    /// Rendered, read-only.
    Reading,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShape {
    Bar,
    Block,
    Underline,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub theme: ThemeMode,
    pub font_size: f32,
    pub ui_scale: f32,
    pub readable_line_length: bool,
    pub default_view: ViewMode,
    pub autosave_secs: f32,
    pub cursor_effect: String,
    pub cursor_shape: CursorShape,
    pub cursor_blink: bool,
    /// Keep rendering every frame so time-based custom shaders animate.
    pub cursor_continuous: bool,
    /// None = follow the Omarchy theme.
    pub cursor_color: Option<[u8; 3]>,
    /// Background fill alpha (text is never affected). Hyprland blurs what shows through.
    pub background_opacity: f32,
    pub recent_vaults: Vec<PathBuf>,
    pub left_sidebar: bool,
    pub right_sidebar: bool,
    pub left_width: f32,
    pub right_width: f32,
    pub show_line_numbers: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            font_size: 16.0,
            ui_scale: 1.0,
            readable_line_length: true,
            default_view: ViewMode::Live,
            autosave_secs: 1.0,
            cursor_effect: "trail".into(),
            cursor_shape: CursorShape::Bar,
            cursor_blink: true,
            cursor_continuous: false,
            cursor_color: None,
            background_opacity: 0.82,
            recent_vaults: vec![],
            left_sidebar: true,
            right_sidebar: false,
            left_width: 260.0,
            right_width: 260.0,
            show_line_numbers: false,
        }
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("carbon")
}

impl Config {
    pub fn load() -> Self {
        std::fs::read_to_string(config_dir().join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join("config.json"), s);
        }
    }

    pub fn remember_vault(&mut self, p: &Path) {
        self.recent_vaults.retain(|v| v != p);
        self.recent_vaults.insert(0, p.to_path_buf());
        self.recent_vaults.truncate(12);
        self.save();
    }
}

/// What was open in a vault last time.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
    pub expanded: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SessionTab {
    pub path: String,
    pub pinned: bool,
    pub view: Option<ViewMode>,
}

fn session_file(vault: &Path) -> PathBuf {
    // Stable per-vault key without pulling in a hash crate.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in vault.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    config_dir().join("sessions").join(format!("{h:016x}.json"))
}

impl Session {
    pub fn load(vault: &Path) -> Self {
        std::fs::read_to_string(session_file(vault))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, vault: &Path) {
        let f = session_file(vault);
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(f, s);
        }
    }
}
