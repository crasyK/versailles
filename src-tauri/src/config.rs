use crate::error::{AppError, AppResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
            enabled: true,
            page: default_desktop_page(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LauncherConfig {
    #[serde(default = "default_launcher_hotkey")]
    pub hotkey: String,
    /// Spawnable id for tray left-click / show_launcher when multiple hotkey pieces exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
}

/// `overlay.dim`: a flag (legacy) or a mode name.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum DimSetting {
    Flag(bool),
    Mode(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpawnableOverlayConfig {
    /// `true` / `"host"`: host dim (default). `"page"`: the page draws its own
    /// backdrop in a monitor-sized window. `false` / `"none"`: no backdrop.
    #[serde(default)]
    pub dim: Option<DimSetting>,
    #[serde(default)]
    pub dismiss_on_blur: Option<bool>,
    #[serde(default)]
    pub dismiss_on_dim_click: Option<bool>,
    /// Allow dragging the spawn window by its background. Overlays default false.
    #[serde(default)]
    pub draggable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpawnableEngineOpts {
    #[serde(default)]
    pub blur_dismiss_ms: Option<u32>,
    #[serde(default)]
    pub suggestion_limit: Option<u32>,
    #[serde(default)]
    pub compact: Option<bool>,
    #[serde(default)]
    pub launch_tick: Option<bool>,
    #[serde(default)]
    pub search_hf: Option<String>,
    #[serde(default)]
    pub time_aware_defaults: Option<bool>,
    #[serde(default)]
    pub auto_dismiss_launch: Option<bool>,
    /// `tc` pieces: the window covers the monitor and the page lays out the
    /// chrome. The host builds such a window monitor-sized and keeps it hidden
    /// on first open until the page marks `html[data-stage]`.
    #[serde(default)]
    pub cover_monitor: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpawnableConfig {
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub overlay: Option<SpawnableOverlayConfig>,
    #[serde(default)]
    pub opts: Option<SpawnableEngineOpts>,
    /// Scoped shortcuts for this spawnable; `"inherit"` uses top-level shortcuts.
    #[serde(default)]
    pub shortcuts: Option<ShortcutsSpec>,
}

fn default_spawnables() -> HashMap<String, SpawnableConfig> {
    HashMap::new()
}

fn default_launcher_hotkey() -> String {
    "Alt+Space".into()
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            hotkey: default_launcher_hotkey(),
            primary: None,
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

fn default_autostart() -> bool {
    true
}

fn default_snap_threshold() -> i32 {
    12
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShortcutEntry {
    pub n: String,
    pub t: String,
    pub d: String,
    pub target: String,
    #[serde(default)]
    pub cat: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
}

/// HTML `#versailles` inlines a list; sidecar `versailles.json` still uses a path.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ShortcutsSpec {
    List(Vec<ShortcutEntry>),
    Path(String),
}

impl Default for ShortcutsSpec {
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

/// `"shell"` in `#versailles`: how much of the Windows shell Versailles takes over.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShellConfig {
    /// Hide Explorer's taskbar (and its reserved strip) while Versailles runs.
    #[serde(default)]
    pub hide_taskbar: bool,
    /// Win-key combos to swallow without binding them (`"Win"`, `"Win+X"`, `"Copilot"`).
    #[serde(default)]
    pub suppress: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserConfig {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub shortcuts: ShortcutsSpec,
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
    #[serde(default)]
    pub shell: ShellConfig,
    /// Per-spawnable engine and overlay options keyed by `data-id`.
    #[serde(default = "default_spawnables")]
    pub spawnables: HashMap<String, SpawnableConfig>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            theme: default_theme(),
            shortcuts: ShortcutsSpec::default(),
            autostart: default_autostart(),
            snap_threshold: default_snap_threshold(),
            launcher: LauncherConfig::default(),
            api: ApiConfig::default(),
            desktop: DesktopConfig::default(),
            shell: ShellConfig::default(),
            spawnables: default_spawnables(),
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
    /// Show the HTML desktop on boot. Authoring stays in HTML; this is session-only.
    #[serde(default)]
    pub desktop_enabled: Option<bool>,
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
            desktop_enabled: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: String,
    pub theme: String,
    pub shortcuts: ShortcutsSpec,
    pub autostart: bool,
    pub api_enabled: bool,
    pub api_token: String,
    pub api_port: u16,
    #[serde(default)]
    pub api_bound_port: Option<u16>,
    pub snap_threshold: i32,
    pub launcher_hotkey: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_primary: Option<String>,
    pub session_widgets: Vec<SessionWidget>,
    pub active_layout: Option<String>,
    #[serde(default)]
    pub catalog: CatalogState,
    #[serde(default)]
    pub desktop: DesktopConfig,
    #[serde(default)]
    pub shell: ShellConfig,
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
            launcher_primary: user.launcher.primary.clone(),
            session_widgets: runtime.session_widgets,
            active_layout: runtime.active_layout,
            catalog: runtime.catalog,
            desktop: DesktopConfig {
                enabled: runtime.desktop_enabled.unwrap_or(user.desktop.enabled),
                page: user.desktop.page,
            },
            shell: user.shell,
        }
    }

    /// Re-apply the user part after `#versailles` / `versailles.json` changed on
    /// disk. Runtime state, the API port/enable flag and the page path are kept
    /// (the server is already bound; the page path scopes `/files`).
    pub fn apply_user_config(&mut self, user: UserConfig) {
        self.version = user.version;
        self.theme = user.theme;
        self.shortcuts = user.shortcuts;
        self.autostart = user.autostart;
        self.snap_threshold = user.snap_threshold;
        self.launcher_hotkey = user.launcher.hotkey;
        self.launcher_primary = user.launcher.primary;
        self.shell = user.shell;
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
                primary: self.launcher_primary.clone(),
            },
            api: ApiConfig {
                enabled: self.api_enabled,
                port: self.api_port,
            },
            desktop: self.desktop.clone(),
            shell: self.shell.clone(),
            spawnables: HashMap::new(),
        }
    }

    pub fn to_runtime_state(&self) -> RuntimeState {
        RuntimeState {
            api_token: self.api_token.clone(),
            api_bound_port: self.api_bound_port,
            session_widgets: self.session_widgets.clone(),
            active_layout: self.active_layout.clone(),
            catalog: self.catalog.clone(),
            desktop_enabled: Some(self.desktop.enabled),
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
    /// Set when `config.json` could not be read at boot (locked by OneDrive / AV):
    /// the app runs on in-memory defaults and must not overwrite the real file
    /// (token, session) with them.
    runtime_read_only: AtomicBool,
}

impl ConfigStore {
    /// Never fails: a folder that cannot be created is logged and the store
    /// keeps working in memory (writes report their own errors).
    pub fn new(widgets_root: &Path) -> Self {
        let root = widgets_root.join(".versailles");
        let legacy = widgets_root.join(".deck");
        if legacy.is_dir() && !root.exists() {
            if let Err(err) = fs::rename(&legacy, &root) {
                tracing::warn!("Could not migrate .deck → .versailles: {err}");
            }
        }
        if let Err(err) = fs::create_dir_all(root.join("layouts")) {
            tracing::error!("Cannot create {}: {err}", root.display());
        }
        Self {
            widgets_root: widgets_root.to_path_buf(),
            root,
            runtime_read_only: AtomicBool::new(false),
        }
    }

    /// Stop writing `config.json` for this session (see `runtime_read_only`).
    pub fn set_runtime_read_only(&self) {
        self.runtime_read_only.store(true, Ordering::SeqCst);
    }

    pub fn runtime_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn engines_runtime_path(&self) -> PathBuf {
        self.root.join("engines.json")
    }

    pub fn user_config_path(&self) -> PathBuf {
        self.widgets_root.join("versailles.json")
    }

    pub fn layouts_dir(&self) -> PathBuf {
        self.root.join("layouts")
    }

    /// Per-piece key-value stores (`storage` hook).
    pub fn storage_dir(&self) -> PathBuf {
        self.root.join("storage")
    }

    pub fn desktop_html_path(&self) -> PathBuf {
        self.widgets_root.join("desktop").join("index.html")
    }

    pub fn html_has_versailles_block(&self) -> bool {
        fs::read_to_string(self.desktop_html_path())
            .ok()
            .and_then(|html| crate::page::extract_versailles_json(&html).map(|s| !s.is_empty()))
            .unwrap_or(false)
    }

    pub fn load(&self) -> AppResult<AppConfig> {
        let user = self.load_user_config()?;
        let mut runtime = self.load_runtime_state()?;
        if runtime.desktop_enabled.is_none() {
            runtime.desktop_enabled = Some(user.desktop.enabled);
            let _ = self.save_runtime_state(&runtime);
        }
        Ok(AppConfig::from_parts(user, runtime))
    }

    pub fn load_user_config(&self) -> AppResult<UserConfig> {
        let html_path = self.desktop_html_path();
        if let Ok(html) = fs::read_to_string(&html_path) {
            if let Some(json) = crate::page::extract_versailles_json(&html) {
                match serde_json::from_str::<UserConfig>(json) {
                    Ok(cfg) => return Ok(cfg),
                    Err(err) => {
                        tracing::warn!(
                            "Invalid #versailles JSON in {} (line {}, column {}: {err}); trying sidecar versailles.json",
                            html_path.display(),
                            err.line(),
                            err.column()
                        )
                    }
                }
            }
        }

        let path = self.user_config_path();
        if !path.exists() {
            return Ok(UserConfig::default());
        }
        let raw = fs::read_to_string(&path)?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(UserConfig::default());
        }
        match serde_json::from_str::<UserConfig>(trimmed) {
            Ok(cfg) => Ok(cfg),
            Err(err) => {
                tracing::warn!("Invalid versailles.json ({err}); using defaults");
                Ok(UserConfig::default())
            }
        }
    }

    pub fn save_user_config(&self, config: &UserConfig) -> AppResult<()> {
        atomic_write(&self.user_config_path(), &serde_json::to_string_pretty(config)?)
    }

    /// Read `config.json`. Missing/empty → fresh defaults (best-effort save).
    /// Corrupt → back up to `config.json.corrupt-<unix-ts>`, salvage `apiToken`,
    /// reset the rest. An unreadable file (locked) is an error so boot can fall
    /// back to memory without clobbering it.
    pub fn load_runtime_state(&self) -> AppResult<RuntimeState> {
        let path = self.runtime_path();
        if !path.exists() {
            let cfg = RuntimeState::default();
            if let Err(err) = self.save_runtime_state(&cfg) {
                tracing::warn!("Could not create {}: {err}", path.display());
            }
            return Ok(cfg);
        }
        let raw = read_with_retry(&path)?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            let cfg = RuntimeState::default();
            if let Err(err) = self.save_runtime_state(&cfg) {
                tracing::warn!("Could not reset empty {}: {err}", path.display());
            }
            return Ok(cfg);
        }
        match serde_json::from_str::<RuntimeState>(trimmed) {
            Ok(cfg) => Ok(cfg),
            Err(err) => {
                let backup = backup_corrupt(&path);
                let mut cfg = RuntimeState::default();
                let salvaged = salvage_api_token(trimmed);
                if let Some(token) = salvaged.clone() {
                    cfg.api_token = token;
                }
                tracing::warn!(
                    "Invalid runtime config ({err}); backed up to {} and reset (API token {})",
                    backup
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "<backup failed>".into()),
                    if salvaged.is_some() { "kept" } else { "rotated" }
                );
                if let Err(err) = self.save_runtime_state(&cfg) {
                    tracing::warn!("Could not rewrite {}: {err}", path.display());
                }
                Ok(cfg)
            }
        }
    }

    pub fn save_runtime_state(&self, config: &RuntimeState) -> AppResult<()> {
        if self.runtime_read_only.load(Ordering::SeqCst) {
            tracing::debug!("config.json was unreadable at boot; not overwriting it");
            return Ok(());
        }
        atomic_write(&self.runtime_path(), &serde_json::to_string_pretty(config)?)
    }

    pub fn save_runtime_from_app(&self, config: &AppConfig) -> AppResult<()> {
        self.save_runtime_state(&config.to_runtime_state())
    }

    #[allow(dead_code)]
    pub fn save_user_from_app(&self, config: &AppConfig) -> AppResult<()> {
        // Never rewrite desktop/index.html. Sidecar versailles.json only if it already exists.
        if self.html_has_versailles_block() {
            return Ok(());
        }
        if self.user_config_path().exists() {
            return self.save_user_config(&config.to_user_config());
        }
        Ok(())
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

/// Read a file, retrying briefly: OneDrive / AV scanners hold short locks.
fn read_with_retry(path: &Path) -> AppResult<String> {
    let mut last = None;
    for attempt in 0..3 {
        match fs::read_to_string(path) {
            Ok(raw) => return Ok(raw),
            Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                // Not UTF-8: treat as corrupt content, not as a lock.
                return Ok(String::from_utf8_lossy(&fs::read(path)?).into_owned());
            }
            Err(err) => last = Some(err),
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }
    Err(last.map(AppError::from).unwrap_or_else(|| AppError::msg("read failed")))
}

/// Copy a corrupt file aside as `<name>.corrupt-<unix-ts>`; returns the copy.
pub(crate) fn backup_corrupt(path: &Path) -> Option<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = path.file_name()?.to_string_lossy().into_owned();
    let backup = path.with_file_name(format!("{name}.corrupt-{ts}"));
    match fs::copy(path, &backup) {
        Ok(_) => Some(backup),
        Err(err) => {
            tracing::warn!("Could not back up {}: {err}", path.display());
            None
        }
    }
}

/// Pull `"apiToken": "<value>"` out of a config that no longer parses, so a
/// corrupt file does not rotate the token scripts / the SDK already use.
fn salvage_api_token(raw: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        return value
            .get("apiToken")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|t| is_plausible_token(t))
            .map(str::to_string);
    }
    let at = raw.find("\"apiToken\"")?;
    let rest = raw[at + "\"apiToken\"".len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let token = rest[..end].trim();
    is_plausible_token(token).then(|| token.to_string())
}

fn is_plausible_token(t: &str) -> bool {
    (8..=256).contains(&t.len())
        && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// Write via a unique temp file (pid + counter, so concurrent writers never share
/// one), `sync_all`, then rename over the target. Renames are retried briefly
/// because OneDrive / AV can hold the destination open.
pub fn atomic_write(path: &Path, content: &str) -> AppResult<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| AppError::msg("Invalid config path"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("versailles"),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()
    })();
    if let Err(err) = written {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    let mut last = None;
    for attempt in 0..3 {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(err) => last = Some(err),
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    let _ = fs::remove_file(&tmp);
    Err(last.map(AppError::from).unwrap_or_else(|| AppError::msg("rename failed")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("versailles-cfg-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("desktop")).unwrap();
        dir
    }

    #[test]
    fn load_prefers_html_versailles_block() {
        let root = temp_root();
        let html = r#"<!doctype html><html><head>
<script type="application/json" id="versailles">
{ "autostart": false, "snapThreshold": 8, "api": { "enabled": true, "port": 47899 }, "shortcuts": [ { "n": "mail", "t": "web", "d": "Gmail", "target": "https://mail.google.com/", "cat": "personal" } ] }
</script>
</head><body></body></html>"#;
        fs::write(root.join("desktop").join("index.html"), html).unwrap();
        fs::write(
            root.join("versailles.json"),
            r#"{ "autostart": true, "api": { "port": 47831 }, "shortcuts": "shortcuts.json" }"#,
        )
        .unwrap();
        let store = ConfigStore::new(&root);
        let user = store.load_user_config().unwrap();
        assert!(!user.autostart);
        assert_eq!(user.api.port, 47899);
        match &user.shortcuts {
            ShortcutsSpec::List(list) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].n, "mail");
            }
            ShortcutsSpec::Path(_) => panic!("expected inline shortcuts"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shell_hide_taskbar_parses_and_defaults_off() {
        let user: UserConfig = serde_json::from_str(r#"{ "shell": { "hideTaskbar": true } }"#).unwrap();
        assert!(user.shell.hide_taskbar);
        let user: UserConfig = serde_json::from_str("{}").unwrap();
        assert!(!user.shell.hide_taskbar);
        let mut app = AppConfig::from_parts(UserConfig::default(), RuntimeState::default());
        app.apply_user_config(serde_json::from_str(r#"{ "shell": { "hideTaskbar": true } }"#).unwrap());
        assert!(app.shell.hide_taskbar);
    }

    #[test]
    fn load_falls_back_to_sidecar_when_html_has_no_block() {
        let root = temp_root();
        fs::write(
            root.join("desktop").join("index.html"),
            "<html><head></head><body></body></html>",
        )
        .unwrap();
        fs::write(
            root.join("versailles.json"),
            r#"{ "autostart": false, "api": { "port": 47001 }, "shortcuts": "shortcuts.json" }"#,
        )
        .unwrap();
        let store = ConfigStore::new(&root);
        let user = store.load_user_config().unwrap();
        assert!(!user.autostart);
        assert_eq!(user.api.port, 47001);
        match user.shortcuts {
            ShortcutsSpec::Path(p) => assert_eq!(p, "shortcuts.json"),
            ShortcutsSpec::List(_) => panic!("expected path sidecar"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_defaults_when_nothing_exists() {
        let root = temp_root();
        let store = ConfigStore::new(&root);
        let user = store.load_user_config().unwrap();
        assert!(user.autostart);
        assert_eq!(user.api.port, 47831);
        assert!(!store.user_config_path().exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_runtime_config_is_backed_up_and_token_salvaged() {
        let root = temp_root();
        let store = ConfigStore::new(&root);
        let token = "0b9d3f8e-1111-2222-3333-444455556666";
        fs::write(
            store.runtime_path(),
            format!("{{ \"apiToken\": \"{token}\", \"sessionWidgets\": [ {{ \"id\": "),
        )
        .unwrap();
        let cfg = store.load_runtime_state().unwrap();
        assert_eq!(cfg.api_token, token);
        let backups: Vec<_> = fs::read_dir(root.join(".versailles"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("config.json.corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1);
        // Rewritten file parses again and keeps the token.
        let again = store.load_runtime_state().unwrap();
        assert_eq!(again.api_token, token);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn salvage_handles_wrong_types_and_garbage() {
        assert_eq!(
            salvage_api_token(r#"{"apiToken":"abcdefgh-1234","sessionWidgets":5}"#).as_deref(),
            Some("abcdefgh-1234")
        );
        assert_eq!(salvage_api_token("not json at all"), None);
        assert_eq!(salvage_api_token(r#"{"apiToken": "x"}"#), None);
        assert_eq!(salvage_api_token(r#"{"apiToken": "abc def ghi jkl"#), None);
    }

    #[test]
    fn read_only_store_does_not_overwrite_runtime_file() {
        let root = temp_root();
        let store = ConfigStore::new(&root);
        fs::write(store.runtime_path(), r#"{ "apiToken": "keep-me-1234" }"#).unwrap();
        store.set_runtime_read_only();
        store.save_runtime_state(&RuntimeState::default()).unwrap();
        assert!(fs::read_to_string(store.runtime_path()).unwrap().contains("keep-me-1234"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_write_uses_unique_temp_files() {
        let root = temp_root();
        let path = root.join("x.json");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || atomic_write(&path, &format!("{{\"n\":{i}}}")))
            })
            .collect();
        for h in handles {
            // Concurrent renames may lose the race on Windows, but never corrupt.
            let _ = h.join().unwrap();
        }
        let raw = fs::read_to_string(&path).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&raw).is_ok(), "{raw}");
        let leftovers = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(root);
    }
}
