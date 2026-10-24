//! First run (#42): offer to set up the starter desktop, with consent.
//!
//! On a start without `<widgets root>/desktop/index.html` (the root respects
//! `VERSAILLES_ROOT`), and only until the user has answered once (the marker
//! `first-run-offered` in the app's local data folder, not in the synced
//! Documents folder), a small `welcome` window asks: "Set up the example
//! desktop" or "I'll bring my own". Closing the window without answering
//! asks again at the next start. Never in bench / e2e mode.
//!
//! Setting up copies the starter that ships inside the app
//! (`templates/starter`, embedded here) into the widgets root, one file at a
//! time with `create_new`: an existing file (a page, an `.sdk/` file) is
//! never overwritten, only skipped. Nothing else is written. The window has
//! its own capability (`capabilities/welcome.json`) with exactly these two
//! commands.
//!
//! The SDK keeps its own `include_str!` copy (`sdk::BUNDLED`): it is a
//! `const &str` that tests and `sdk::decide` use directly; the duplicate is
//! one 37 KB file.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use include_dir::{include_dir, Dir, DirEntry};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// `templates/starter` as shipped with this build.
static STARTER: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../templates/starter");

pub const LABEL: &str = "welcome";
const MARKER: &str = "first-run-offered";
/// Starter files that are notes for this repository, not part of a desktop.
const NOT_COPIED: &[&str] = &["README.md"];

fn marker_path() -> Option<PathBuf> {
    crate::crash::app_data_dir().map(|d| d.join(MARKER))
}

/// No page yet, and the user has not answered the offer.
pub fn should_offer(root: &Path, marker: Option<&Path>) -> bool {
    !root.join("desktop").join("index.html").exists() && marker.is_some_and(|m| !m.exists())
}

/// Open the welcome window when this start should offer the starter.
pub fn offer_if_needed(app: &AppHandle) {
    if crate::boot::is_bench() {
        return;
    }
    let Ok(root) = crate::registry::widgets_root() else {
        return;
    };
    if !should_offer(&root, marker_path().as_deref()) {
        return;
    }
    tracing::info!("first run: no {}; offering the starter desktop", root.join("desktop").join("index.html").display());
    if let Err(err) = open_window(app) {
        tracing::warn!("first run: welcome window failed: {err}");
    }
}

fn open_window(app: &AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.set_focus();
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("welcome.html".into()))
        .title("Welcome to Versailles")
        .inner_size(480.0, 420.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .center()
        .visible(true)
        .build()?;
    let _ = window.set_focus();
    Ok(())
}

/// What a setup wrote and what it left alone (paths relative to the root).
#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CopyReport {
    pub written: Vec<String>,
    pub skipped: Vec<String>,
}

/// Copy the embedded starter into `root` without overwriting anything.
pub fn copy_starter(root: &Path) -> std::io::Result<CopyReport> {
    copy_dir(&STARTER, root)
}

fn copy_dir(dir: &Dir<'_>, root: &Path) -> std::io::Result<CopyReport> {
    let mut report = CopyReport::default();
    let mut files = Vec::new();
    collect(dir, &mut files);
    for file in files {
        let rel = file.path();
        if NOT_COPIED.iter().any(|n| rel == Path::new(n)) {
            continue;
        }
        let shown = rel.to_string_lossy().replace('\\', "/");
        let target = root.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(mut out) => {
                out.write_all(file.contents())?;
                report.written.push(shown);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => report.skipped.push(shown),
            Err(err) => return Err(err),
        }
    }
    Ok(report)
}

fn collect<'a>(dir: &'a Dir<'a>, out: &mut Vec<&'a include_dir::File<'a>>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(d) => collect(d, out),
            DirEntry::File(f) => out.push(f),
        }
    }
}

fn write_marker(answer: &str) {
    let Some(path) = marker_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let stamp = chrono::Local::now().to_rfc3339();
    if let Err(err) = std::fs::write(&path, format!("{answer} {stamp}\n")) {
        tracing::warn!("first run: could not write {} ({err})", path.display());
    }
}

/// Only the welcome window may answer.
fn require_welcome(window: &WebviewWindow) -> AppResult<()> {
    if window.label() == LABEL {
        Ok(())
    } else {
        Err(AppError::msg("only the welcome window can do this"))
    }
}

/// "Set up the example desktop": copy the starter, remember the answer,
/// close the window and show the desktop.
#[tauri::command]
pub async fn welcome_setup(app: AppHandle, window: WebviewWindow) -> AppResult<CopyReport> {
    require_welcome(&window)?;
    let root = crate::registry::widgets_root()?;
    let report = copy_starter(&root)?;
    tracing::info!(
        "first run: set up the starter in {} ({} files written, {} kept: {:?})",
        root.display(),
        report.written.len(),
        report.skipped.len(),
        report.skipped
    );
    write_marker("setup");
    {
        let state = app.state::<AppState>();
        let mut config = state.config.lock().unwrap();
        if !config.desktop.enabled {
            config.desktop.enabled = true;
            let _ = state.store.lock().unwrap().save_runtime_from_app(&config);
        }
    }
    crate::desktop::refresh_page_cache(&app.state::<AppState>());
    // Window work on the main thread, outside this IPC call (the file
    // watcher then picks up the new page: config, hotkeys, docked bars).
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(LABEL) {
            let _ = window.close();
        }
        if let Err(err) = crate::desktop::reveal_desktop_window(&handle) {
            tracing::warn!("first run: desktop failed to open: {err}");
        }
    })?;
    Ok(report)
}

/// "I'll bring my own": remember the answer and close the window.
#[tauri::command]
pub async fn welcome_skip(app: AppHandle, window: WebviewWindow) -> AppResult<()> {
    require_welcome(&window)?;
    tracing::info!("first run: the user brings their own page");
    write_marker("skip");
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(LABEL) {
            let _ = window.close();
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("versailles-first-run-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn embeds_the_whole_starter() {
        for rel in ["desktop/index.html", "desktop/bar.js", ".sdk/versailles.js", ".sdk/xterm.js"] {
            assert!(STARTER.get_file(rel).is_some(), "{rel}");
        }
        // The SDK served and synced by the host is the same file.
        let sdk = STARTER.get_file(".sdk/versailles.js").unwrap().contents_utf8().unwrap();
        assert_eq!(crate::sdk::content_hash(sdk), crate::sdk::content_hash(crate::sdk::BUNDLED));
    }

    #[test]
    fn offers_only_without_a_page_and_until_answered() {
        let root = temp_root();
        let marker = root.join("marker");
        std::fs::create_dir_all(&root).unwrap();
        assert!(should_offer(&root, Some(&marker)));
        // No local data folder: never offer (the answer could not be kept).
        assert!(!should_offer(&root, None));
        std::fs::write(&marker, "skip").unwrap();
        assert!(!should_offer(&root, Some(&marker)));
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir_all(root.join("desktop")).unwrap();
        std::fs::write(root.join("desktop").join("index.html"), "mine").unwrap();
        assert!(!should_offer(&root, Some(&marker)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copies_without_overwriting() {
        let root = temp_root();
        std::fs::create_dir_all(root.join(".sdk")).unwrap();
        std::fs::write(root.join(".sdk").join("versailles.js"), "my own sdk").unwrap();
        let report = copy_starter(&root).unwrap();
        assert_eq!(report.skipped, vec![".sdk/versailles.js".to_string()]);
        assert!(report.written.contains(&"desktop/index.html".to_string()));
        assert!(report.written.contains(&".sdk/xterm.js".to_string()));
        assert!(!report.written.iter().any(|p| p == "README.md"));
        assert!(!root.join("README.md").exists());
        assert_eq!(std::fs::read_to_string(root.join(".sdk").join("versailles.js")).unwrap(), "my own sdk");
        assert_eq!(
            std::fs::read(root.join("desktop").join("index.html")).unwrap(),
            STARTER.get_file("desktop/index.html").unwrap().contents()
        );
        // A second run writes nothing.
        let again = copy_starter(&root).unwrap();
        assert!(again.written.is_empty());
        assert_eq!(again.skipped.len(), report.written.len() + report.skipped.len());
        let _ = std::fs::remove_dir_all(&root);
    }
}
