use crate::error::{AppError, AppResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogState {
    /// App ids already shown in the action bar (used to flag newly installed apps).
    #[serde(default)]
    pub seen_ids: Vec<String>,
    /// Apps the user hid from the action bar.
    #[serde(default)]
    pub hidden_ids: Vec<String>,
}

/// Desktop-as-a-website shell. Layout lives in `Documents\\Widgets\\desktop\\index.html`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Path under Documents\\Widgets for the desktop website (HTML you edit).
    #[serde(default = "default_desktop_page")]
    pub page: String,
    /// Separate always-on-top HUD. Covers app title bars — off by default.
    /// The HUD on the desktop page is the normal one.
    #[serde(default)]
    pub anywhere_bar: bool,
}

fn default_desktop_page() -> String {
    "desktop/index.html".into()
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            page: default_desktop_page(),
            anywhere_bar: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionWidget {
    pub id: String,
    pub position: Position,
    pub always_on_top: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: String,
    pub autostart: bool,
    pub open_manager_on_startup: bool,
    pub api_enabled: bool,
    pub api_token: String,
    pub api_port: u16,
    pub snap_threshold: i32,
    pub launcher_hotkey: String,
    pub session_widgets: Vec<SessionWidget>,
    pub active_layout: Option<String>,
    #[serde(default)]
    pub catalog: CatalogState,
    #[serde(default)]
    pub desktop: DesktopConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: "0.1.0".into(),
            autostart: true,
            open_manager_on_startup: false,
            api_enabled: true,
            api_token: uuid::Uuid::new_v4().to_string(),
            api_port: 47831,
            snap_threshold: 12,
            launcher_hotkey: "Alt+Space".into(),
            session_widgets: Vec::new(),
            active_layout: None,
            catalog: CatalogState::default(),
            desktop: DesktopConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonitorFingerprint {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPlacement {
    pub x: i32,
    pub y: i32,
    pub monitor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LayoutTemplate {
    pub name: String,
    pub created_at: String,
    pub monitors: Vec<MonitorFingerprint>,
    pub widgets: HashMap<String, LayoutPlacement>,
}

pub struct ConfigStore {
    root: PathBuf,
}

impl ConfigStore {
    pub fn new(widgets_root: &Path) -> AppResult<Self> {
        let root = widgets_root.join(".deck");
        fs::create_dir_all(root.join("layouts"))?;
        Ok(Self { root })
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn layouts_dir(&self) -> PathBuf {
        self.root.join("layouts")
    }

    pub fn load(&self) -> AppResult<AppConfig> {
        let path = self.config_path();
        if !path.exists() {
            let cfg = AppConfig::default();
            self.save(&cfg)?;
            return Ok(cfg);
        }
        let raw = fs::read_to_string(&path)?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            let cfg = AppConfig::default();
            self.save(&cfg)?;
            return Ok(cfg);
        }
        match serde_json::from_str::<AppConfig>(trimmed) {
            Ok(cfg) => Ok(cfg),
            Err(err) => {
                tracing::warn!("Invalid config.json ({err}); recreating defaults");
                let cfg = AppConfig::default();
                self.save(&cfg)?;
                Ok(cfg)
            }
        }
    }

    pub fn save(&self, config: &AppConfig) -> AppResult<()> {
        atomic_write(&self.config_path(), &serde_json::to_string_pretty(config)?)
    }

    pub fn save_layout(&self, layout: &LayoutTemplate) -> AppResult<()> {
        let path = self.layouts_dir().join(format!("{}.json", sanitize(&layout.name)));
        atomic_write(&path, &serde_json::to_string_pretty(layout)?)
    }

    pub fn load_layout(&self, name: &str) -> AppResult<LayoutTemplate> {
        let path = self.layouts_dir().join(format!("{}.json", sanitize(name)));
        let raw = fs::read_to_string(path)
            .map_err(|_| AppError::msg(format!("Layout '{name}' not found")))?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn list_layouts(&self) -> AppResult<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(self.layouts_dir())? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

pub fn atomic_write(path: &Path, content: &str) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::msg("Invalid config path"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("deck")
    ));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
