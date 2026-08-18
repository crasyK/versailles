use crate::error::{AppError, AppResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
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
    #[serde(default = "default_desktop_enabled")]
    pub enabled: bool,
    /// Path under Documents\\Widgets for the desktop website (HTML you edit).
    #[serde(default = "default_desktop_page")]
    pub page: String,
}

fn default_desktop_page() -> String {
    "desktop/index.html".into()
}

fn default_desktop_enabled() -> bool {
    true
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            page: default_desktop_page(),
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
pub struct AppConfig {
    pub version: String,
    pub autostart: bool,
    pub api_port: u16,
    pub snap_threshold: i32,
    pub launcher_hotkey: String,
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
            api_port: 47841,
            snap_threshold: 12,
            launcher_hotkey: "Alt+Space".into(),
            active_layout: None,
            catalog: CatalogState::default(),
            desktop: DesktopConfig::default(),
        }
    }
}

pub struct ConfigStore {
    root: PathBuf,
}

impl ConfigStore {
    pub fn new(widgets_root: &Path) -> AppResult<Self> {
        let root = widgets_root.join(".versailles");
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
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
            .unwrap_or("versailles")
    ));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
