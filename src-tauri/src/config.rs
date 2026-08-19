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
}

fn default_desktop_page() -> String {
    "desktop/index.html".into()
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            page: default_desktop_page(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LauncherConfig {
    #[serde(default = "default_launcher_hotkey")]
    pub hotkey: String,
}

fn default_launcher_hotkey() -> String {
    "Alt+Space".into()
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            hotkey: default_launcher_hotkey(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiConfig {
    #[serde(default = "default_api_enabled")]
    pub enabled: bool,
    #[serde(default = "default_api_port")]
    pub port: u16,
}

fn default_api_enabled() -> bool {
    true
}

fn default_api_port() -> u16 {
    47831
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            enabled: default_api_enabled(),
            port: default_api_port(),
        }
    }
}

fn default_theme() -> String {
    "paper".into()
}

fn default_shortcuts_path() -> String {
    "shortcuts.json".into()
}

fn default_autostart() -> bool {
    true
}

fn default_snap_threshold() -> i32 {
    12
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserConfig {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_shortcuts_path")]
    pub shortcuts: String,
    #[serde(default = "default_autostart")]
    pub autostart: bool,
    #[serde(default = "default_snap_threshold")]
    pub snap_threshold: i32,
    #[serde(default)]
    pub launcher: LauncherConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub desktop: DesktopConfig,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            theme: default_theme(),
            shortcuts: default_shortcuts_path(),
            autostart: default_autostart(),
            snap_threshold: default_snap_threshold(),
            launcher: LauncherConfig::default(),
            api: ApiConfig::default(),
            desktop: DesktopConfig::default(),
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
pub struct RuntimeState {
    #[serde(default = "default_api_token")]
    pub api_token: String,
    #[serde(default)]
    pub api_bound_port: Option<u16>,
    #[serde(default)]
    pub session_widgets: Vec<SessionWidget>,
    #[serde(default)]
    pub active_layout: Option<String>,
    #[serde(default)]
    pub catalog: CatalogState,
}

fn default_version() -> String {
    "0.1.0".into()
}

fn default_api_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            api_token: default_api_token(),
            api_bound_port: None,
            session_widgets: Vec::new(),
            active_layout: None,
            catalog: CatalogState::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: String,
    pub theme: String,
    pub shortcuts: String,
    pub autostart: bool,
    pub api_enabled: bool,
    pub api_token: String,
    pub api_port: u16,
    #[serde(default)]
    pub api_bound_port: Option<u16>,
    pub snap_threshold: i32,
    pub launcher_hotkey: String,
    pub session_widgets: Vec<SessionWidget>,
    pub active_layout: Option<String>,
    #[serde(default)]
    pub catalog: CatalogState,
    #[serde(default)]
    pub desktop: DesktopConfig,
}

impl AppConfig {
    pub fn from_parts(user: UserConfig, runtime: RuntimeState) -> Self {
        Self {
            version: user.version,
            theme: user.theme,
            shortcuts: user.shortcuts,
            autostart: user.autostart,
            api_enabled: user.api.enabled,
            api_token: runtime.api_token,
            api_port: user.api.port,
            api_bound_port: runtime.api_bound_port,
            snap_threshold: user.snap_threshold,
            launcher_hotkey: user.launcher.hotkey,
            session_widgets: runtime.session_widgets,
            active_layout: runtime.active_layout,
            catalog: runtime.catalog,
            desktop: user.desktop,
        }
    }

    pub fn to_user_config(&self) -> UserConfig {
        UserConfig {
            version: self.version.clone(),
            theme: self.theme.clone(),
            shortcuts: self.shortcuts.clone(),
            autostart: self.autostart,
            snap_threshold: self.snap_threshold,
            launcher: LauncherConfig {
                hotkey: self.launcher_hotkey.clone(),
            },
            api: ApiConfig {
                enabled: self.api_enabled,
                port: self.api_port,
            },
            desktop: self.desktop.clone(),
        }
    }

    pub fn to_runtime_state(&self) -> RuntimeState {
        RuntimeState {
            api_token: self.api_token.clone(),
            api_bound_port: self.api_bound_port,
            session_widgets: self.session_widgets.clone(),
            active_layout: self.active_layout.clone(),
            catalog: self.catalog.clone(),
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
    widgets_root: PathBuf,
    root: PathBuf,
}

impl ConfigStore {
    pub fn new(widgets_root: &Path) -> AppResult<Self> {
        let root = widgets_root.join(".versailles");
        let legacy = widgets_root.join(".deck");
        if legacy.is_dir() && !root.exists() {
            if let Err(err) = fs::rename(&legacy, &root) {
                tracing::warn!("Could not migrate .deck → .versailles: {err}");
            }
        }
        fs::create_dir_all(root.join("layouts"))?;
        Ok(Self {
            widgets_root: widgets_root.to_path_buf(),
            root,
        })
    }

    pub fn runtime_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn user_config_path(&self) -> PathBuf {
        self.widgets_root.join("versailles.json")
    }

    pub fn layouts_dir(&self) -> PathBuf {
        self.root.join("layouts")
    }

    pub fn load(&self) -> AppResult<AppConfig> {
        let user = self.load_user_config()?;
        let runtime = self.load_runtime_state()?;
        Ok(AppConfig::from_parts(user, runtime))
    }

    pub fn load_user_config(&self) -> AppResult<UserConfig> {
        let path = self.user_config_path();
        if !path.exists() {
            let cfg = UserConfig::default();
            self.save_user_config(&cfg)?;
            return Ok(cfg);
        }
        let raw = fs::read_to_string(&path)?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            let cfg = UserConfig::default();
            self.save_user_config(&cfg)?;
            return Ok(cfg);
        }
        match serde_json::from_str::<UserConfig>(trimmed) {
            Ok(cfg) => Ok(cfg),
            Err(err) => {
                tracing::warn!("Invalid versailles.json ({err}); recreating defaults");
                let cfg = UserConfig::default();
                self.save_user_config(&cfg)?;
                Ok(cfg)
            }
        }
    }

    pub fn save_user_config(&self, config: &UserConfig) -> AppResult<()> {
        atomic_write(&self.user_config_path(), &serde_json::to_string_pretty(config)?)
    }

    pub fn load_runtime_state(&self) -> AppResult<RuntimeState> {
        let path = self.runtime_path();
        if !path.exists() {
            let cfg = RuntimeState::default();
            self.save_runtime_state(&cfg)?;
            return Ok(cfg);
        }
        let raw = fs::read_to_string(&path)?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            let cfg = RuntimeState::default();
            self.save_runtime_state(&cfg)?;
            return Ok(cfg);
        }
        match serde_json::from_str::<RuntimeState>(trimmed) {
            Ok(cfg) => Ok(cfg),
            Err(err) => {
                tracing::warn!("Invalid runtime config ({err}); recreating defaults");
                let cfg = RuntimeState::default();
                self.save_runtime_state(&cfg)?;
                Ok(cfg)
            }
        }
    }

    pub fn save_runtime_state(&self, config: &RuntimeState) -> AppResult<()> {
        atomic_write(&self.runtime_path(), &serde_json::to_string_pretty(config)?)
    }

    pub fn save_runtime_from_app(&self, config: &AppConfig) -> AppResult<()> {
        self.save_runtime_state(&config.to_runtime_state())
    }

    pub fn save_user_from_app(&self, config: &AppConfig) -> AppResult<()> {
        self.save_user_config(&config.to_user_config())
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
            .unwrap_or("versailles")
    ));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
