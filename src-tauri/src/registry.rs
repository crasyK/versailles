use crate::config::Position;
use crate::error::{AppError, AppResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WidgetPermission {
    Media,
    Mouse,
    Hotkeys,
    Layout,
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WidgetManifest {
    pub id: String,
    pub title: String,
    #[serde(default = "default_entry")]
    pub entry: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub default_position: Option<Position>,
    #[serde(default)]
    pub always_on_top: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub border_radius: u32,
    #[serde(default)]
    pub permissions: Vec<WidgetPermission>,
}

fn default_entry() -> String {
    "index.html".into()
}

fn default_opacity() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredWidget {
    pub manifest: WidgetManifest,
    pub path: PathBuf,
    pub entry_path: PathBuf,
    #[serde(default)]
    pub legacy: bool,
    #[serde(default)]
    pub embedded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySnapshot {
    pub widgets: Vec<RegisteredWidget>,
    pub errors: Vec<String>,
}

pub struct WidgetRegistry {
    root: PathBuf,
    widgets: Vec<RegisteredWidget>,
    errors: Vec<String>,
}

impl WidgetRegistry {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            widgets: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Rescan the widgets tree from disk and replace the in-memory registry.
    /// Called at startup and by the file watcher, not on every IPC list call.
    pub fn scan(&mut self) -> AppResult<RegistrySnapshot> {
        self.widgets.clear();
        self.errors.clear();
        scan_dir(&self.root, &self.root, &mut self.widgets, &mut self.errors)?;
        self.widgets
            .sort_by(|a, b| a.manifest.title.cmp(&b.manifest.title));
        Ok(self.snapshot())
    }

    /// Return a clone of the current in-memory registry without scanning disk.
    pub fn snapshot(&self) -> RegistrySnapshot {
        RegistrySnapshot {
            widgets: self.widgets.clone(),
            errors: self.errors.clone(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&RegisteredWidget> {
        self.widgets.iter().find(|w| w.manifest.id == id)
    }

    pub fn list(&self) -> &[RegisteredWidget] {
        &self.widgets
    }
}

fn scan_dir(
    root: &Path,
    dir: &Path,
    widgets: &mut Vec<RegisteredWidget>,
    errors: &mut Vec<String>,
) -> AppResult<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            errors.push(format!("Cannot read {}: {err}", dir.display()));
            return Ok(());
        }
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if !path.is_dir() {
            continue;
        }

        // Never scan the Versailles app, the desktop HTML page, or internal state.
        if name.eq_ignore_ascii_case("app")
            || name.eq_ignore_ascii_case("desktop")
            || name.eq_ignore_ascii_case("legacy")
            || name.eq_ignore_ascii_case(".versailles")
            || name.eq_ignore_ascii_case(".deck")
            || name.starts_with('.')
        {
            continue;
        }

        // Archive holds reference material; only legacy-widgets are registered.
        if name.eq_ignore_ascii_case("archive") {
            let legacy_widgets = path.join("legacy-widgets");
            if legacy_widgets.is_dir() {
                scan_dir(root, &legacy_widgets, widgets, errors)?;
            }
            continue;
        }

        // Empty redirect folder after legacy move — skip.
        if name.eq_ignore_ascii_case("legacy") {
            continue;
        }

        let manifest_path = path.join("widget.json");
        if manifest_path.exists() {
            match load_widget(&path, &manifest_path) {
                Ok(mut widget) => {
                    widget.legacy = is_legacy_dir(root, &path);
                    widgets.push(widget);
                }
                Err(err) => errors.push(format!("{}: {err}", path.display())),
            }
        } else {
            scan_dir(root, &path, widgets, errors)?;
        }
    }

    Ok(())
}

fn load_widget(dir: &Path, manifest_path: &Path) -> AppResult<RegisteredWidget> {
    let raw = fs::read_to_string(manifest_path)?;
    let mut manifest: WidgetManifest = serde_json::from_str(&raw)?;
    if manifest.id.trim().is_empty() {
        return Err(AppError::msg("widget.json missing id"));
    }
    if manifest.width == 0 || manifest.height == 0 {
        return Err(AppError::msg("widget.json width/height must be > 0"));
    }
    if manifest.default_position.is_none() {
        manifest.default_position = Some(Position { x: 40, y: 40 });
    }

    let entry_path = dir.join(&manifest.entry);
    if !entry_path.exists() {
        return Err(AppError::msg(format!(
            "entry file missing: {}",
            manifest.entry
        )));
    }

    Ok(RegisteredWidget {
        manifest,
        path: dir.to_path_buf(),
        entry_path,
        legacy: false,
        embedded: false,
        error: None,
    })
}

fn is_legacy_dir(root: &Path, dir: &Path) -> bool {
    let Ok(rel) = dir.strip_prefix(root) else {
        return false;
    };
    let mut components = rel.components();
    let first = components.next();
    let second = components.next();
    match (first, second) {
        (Some(c), _) if c.as_os_str() == "legacy" => true,
        (Some(a), Some(b)) if a.as_os_str() == "archive" && b.as_os_str() == "legacy-widgets" => {
            true
        }
        _ => false,
    }
}

pub fn widgets_root() -> AppResult<PathBuf> {
    let docs = dirs::document_dir().ok_or_else(|| AppError::msg("Documents folder not found"))?;
    let root = docs.join("Widgets");
    fs::create_dir_all(&root)?;
    Ok(root)
}
