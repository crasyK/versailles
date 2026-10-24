//! Diagnostics bundle (#43): one zip a user can attach to a bug report.
//!
//! Tray menu "Copy diagnostics", or `versailles.exe --diagnostics` when the
//! app will not start. Writes `versailles-diagnostics-<yyyymmdd-hhmmss>.zip`
//! to the Downloads folder and copies its path to the clipboard. Nothing is
//! sent anywhere.
//!
//! Contents:
//! - `info.json`: host / SDK / WebView2 versions, the Windows build, monitors
//!   with their scale, the page's pieces with their hooks, and the
//!   `#versailles` config with secret-looking values replaced by
//!   `"<redacted>"` (see [`is_secret_key`]);
//! - `Versailles.log*`, `supervisor.log`, `taskbar.log`, `last-crash.txt`
//!   and `taskbar-restore.json`, whichever exist.
//!
//! Every text in the zip has the user's profile folder replaced by
//! `%USERPROFILE%` (the user name is part of it).

use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

const REDACTED: &str = "<redacted>";

/// `--diagnostics`: runs before the supervisor and Tauri, like
/// `--restore-taskbar`. `Some(exit code)` when the argument asked for it.
pub fn run_cli() -> Option<i32> {
    if std::env::args().nth(1)? != "--diagnostics" {
        return None;
    }
    // A GUI-subsystem exe has no console of its own: print into the one it
    // was started from, if any.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    #[cfg(windows)]
    unsafe {
        // Monitor scale is only reported per monitor to a DPI-aware process.
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    match write_bundle(None) {
        Ok(path) => {
            let shown = path.display().to_string();
            let _ = crate::clipboard::write_text(&shown);
            println!("Diagnostics written to {shown} (path copied to the clipboard).");
            Some(0)
        }
        Err(err) => {
            eprintln!("Diagnostics failed: {err}");
            Some(1)
        }
    }
}

/// Tray "Copy diagnostics": write the bundle off the main thread, copy its
/// path and show it in Explorer.
pub fn copy_from_tray(app: &tauri::AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("versailles-diagnostics".into())
        .spawn(move || match write_bundle(Some(&app)) {
            Ok(path) => {
                let shown = path.display().to_string();
                if let Err(err) = crate::clipboard::write_text(&shown) {
                    tracing::warn!("diagnostics: could not copy the path ({err})");
                }
                tracing::info!("diagnostics written to {shown}");
                #[cfg(windows)]
                {
                    let _ = std::process::Command::new("explorer")
                        .arg(format!("/select,{shown}"))
                        .spawn();
                }
            }
            Err(err) => tracing::error!("diagnostics failed: {err}"),
        });
    if let Err(err) = spawned {
        tracing::error!("diagnostics thread failed to start: {err}");
    }
}

/// Write the zip to the Downloads folder and return its path.
pub fn write_bundle(app: Option<&tauri::AppHandle>) -> Result<PathBuf, String> {
    let dir = dirs::download_dir()
        .or_else(dirs::desktop_dir)
        .unwrap_or_else(std::env::temp_dir);
    write_bundle_to(&dir, app)
}

fn write_bundle_to(dir: &Path, app: Option<&tauri::AppHandle>) -> Result<PathBuf, String> {
    let scrub = Scrubber::from_env();
    let now = chrono::Local::now();
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(format!(
        "versailles-diagnostics-{}.zip",
        now.format("%Y%m%d-%H%M%S")
    ));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let info = scrub.value(&info(app));
    let info = serde_json::to_vec_pretty(&info).map_err(|e| e.to_string())?;
    entries.push(("info.json".into(), info));
    for source in log_files() {
        let Ok(bytes) = std::fs::read(&source) else {
            continue;
        };
        let Some(name) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let text = scrub.text(&String::from_utf8_lossy(&bytes));
        entries.push((name, text.into_bytes()));
    }

    let stamp = zip::DateTime::from_date_and_time(
        now.format("%Y").to_string().parse().unwrap_or(1980),
        now.format("%m").to_string().parse().unwrap_or(1),
        now.format("%d").to_string().parse().unwrap_or(1),
        now.format("%H").to_string().parse().unwrap_or(0),
        now.format("%M").to_string().parse().unwrap_or(0),
        now.format("%S").to_string().parse().unwrap_or(0),
    )
    .unwrap_or_default();
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(stamp);
    let written = (|| -> zip::result::ZipResult<()> {
        let mut zip = zip::ZipWriter::new(file);
        for (name, bytes) in &entries {
            zip.start_file(name.as_str(), options)?;
            zip.write_all(bytes)?;
        }
        zip.finish()?;
        Ok(())
    })();
    if let Err(err) = written {
        let _ = std::fs::remove_file(&path);
        return Err(format!("cannot write {}: {err}", path.display()));
    }
    Ok(path)
}

/// Log and state files the bundle takes, whichever exist.
fn log_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = crate::crash::log_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut logs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    // `Versailles.log` and the rotated `Versailles_<date>.log`.
                    (name.starts_with("versailles") && name.ends_with(".log"))
                        || matches!(name.as_str(), "supervisor.log" | "taskbar.log" | "last-crash.txt")
                })
                .collect();
            logs.sort();
            out.extend(logs);
        }
    }
    if let Some(dir) = crate::crash::app_data_dir() {
        let restore = dir.join("taskbar-restore.json");
        if restore.is_file() {
            out.push(restore);
        }
    }
    out
}

fn info(app: Option<&tauri::AppHandle>) -> Value {
    let root = crate::registry::widgets_root_path().ok();
    let page = root.as_deref().map(read_page).unwrap_or_default();
    let mut info = json!({
        "createdAt": chrono::Local::now().to_rfc3339(),
        "host": {
            "version": env!("CARGO_PKG_VERSION"),
            "debug": cfg!(debug_assertions),
            "exe": std::env::current_exe().ok().map(|p| p.display().to_string()),
            "from": if app.is_some() { "tray" } else { "--diagnostics" },
        },
        "sdk": sdk_info(root.as_deref()),
        "webview2": tauri::webview_version().ok(),
        "windows": windows_version(),
        "monitors": monitors(),
        "widgetsRoot": root.as_ref().map(|r| r.display().to_string()),
        "page": {
            "path": page.path.as_ref().map(|p| p.display().to_string()),
            "found": page.html.is_some(),
            "configFrom": page.config_from,
            "configError": page.config_error,
        },
        "config": page.config.map(|mut c| { redact(&mut c); c }),
        "pieces": page.html.as_deref().map(pieces).unwrap_or_default(),
    });
    if let Some(app) = app {
        info["running"] = running(app);
    }
    info
}

fn sdk_info(root: Option<&Path>) -> Value {
    let desktop = root.and_then(|r| std::fs::read_to_string(r.join(crate::sdk::FILES_PATH)).ok());
    json!({
        "bundled": crate::sdk::bundled_version(),
        "desktop": crate::sdk::decide(desktop.as_deref(), crate::sdk::BUNDLED),
        "desktopMarker": desktop.as_deref().and_then(crate::sdk::marker),
        // What the start-up check did (only when the app is running).
        "atStart": crate::sdk::status(),
    })
}

/// State of the running app (tray only).
fn running(app: &tauri::AppHandle) -> Value {
    use tauri::Manager;
    let mut windows: Vec<String> = app.webview_windows().keys().cloned().collect();
    windows.sort();
    let state = app.state::<crate::state::AppState>();
    let (desktop, port) = {
        let config = state.config.lock().unwrap();
        (config.desktop.enabled, config.api_bound_port)
    };
    json!({
        "uptimeMs": crate::boot::elapsed_ms(),
        "supervised": crate::crash::supervised(),
        "desktopShown": desktop,
        "apiPort": port,
        "taskbarHidden": crate::taskbar::hiding(),
        "windows": windows,
    })
}

#[derive(Default)]
struct PageRead {
    path: Option<PathBuf>,
    html: Option<String>,
    config: Option<Value>,
    config_from: Option<&'static str>,
    config_error: Option<String>,
}

/// Read the page and its config the way the host does (`#versailles`,
/// else the sidecar `versailles.json`), without writing anything.
fn read_page(root: &Path) -> PageRead {
    let mut out = PageRead::default();
    let index = root.join("desktop").join("index.html");
    let html = std::fs::read_to_string(&index).ok();
    if let Some(json) = html.as_deref().and_then(crate::page::extract_versailles_json) {
        match serde_json::from_str::<Value>(json) {
            Ok(value) => {
                out.config = Some(value);
                out.config_from = Some("#versailles");
            }
            Err(err) => out.config_error = Some(format!("#versailles: {err}")),
        }
    }
    if out.config.is_none() {
        if let Ok(raw) = std::fs::read_to_string(root.join("versailles.json")) {
            match serde_json::from_str::<Value>(raw.trim()) {
                Ok(value) => {
                    out.config = Some(value);
                    out.config_from = Some("versailles.json");
                }
                Err(err) => out.config_error = Some(format!("versailles.json: {err}")),
            }
        }
    }
    // `desktop.page` moves the page (default `desktop/index.html`).
    let page_rel = out
        .config
        .as_ref()
        .and_then(|c| c.pointer("/desktop/page"))
        .and_then(Value::as_str)
        .map(str::to_string);
    match page_rel {
        Some(rel) if !rel.eq_ignore_ascii_case("desktop/index.html") => {
            let path = root.join(&rel);
            out.html = std::fs::read_to_string(&path).ok();
            out.path = Some(path);
        }
        _ => {
            out.html = html;
            out.path = Some(index);
        }
    }
    out
}

fn pieces(html: &str) -> Vec<Value> {
    crate::page::parse_page(html)
        .pieces
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "kind": p.kind,
                "hooks": p.hooks,
                "anchor": p.anchor,
                "monitor": p.monitor,
                "dock": p.dock,
                "hotkey": p.hotkey,
                "size": [p.width, p.height],
            })
        })
        .collect()
}

/// Whether a config key names a secret: `token`, `secret`, `password`,
/// `key` or `auth` in any case (`apiToken`, `api_key`, `Authorization`).
/// `hotkey` is a key binding, not a key, and stays readable.
pub fn is_secret_key(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.replace("hotkey", "").contains("key")
        || ["token", "secret", "password", "auth"].iter().any(|s| lower.contains(s))
}

/// Replace the value of every secret-looking key (at any depth) with
/// `"<redacted>"`.
pub fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if is_secret_key(key) {
                    *v = Value::String(REDACTED.into());
                } else {
                    redact(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

/// Replaces the user's profile folder (`C:\Users\<name>`) in any spelling
/// the files use: `\`, JSON-escaped `\\` and `/`, in any letter case.
pub struct Scrubber {
    needles: Vec<String>,
}

impl Scrubber {
    pub fn new(profile: Option<&str>) -> Self {
        let mut needles = Vec::new();
        if let Some(p) = profile.map(|p| p.trim_end_matches(['\\', '/'])).filter(|p| p.len() > 3) {
            let back = p.replace('/', "\\");
            needles.push(back.replace('\\', "\\\\"));
            needles.push(back.clone());
            needles.push(back.replace('\\', "/"));
        }
        Self { needles }
    }

    fn from_env() -> Self {
        let profile = std::env::var("USERPROFILE")
            .ok()
            .or_else(|| dirs::home_dir().map(|h| h.display().to_string()));
        Self::new(profile.as_deref())
    }

    pub fn text(&self, text: &str) -> String {
        let mut out = text.to_string();
        for needle in &self.needles {
            out = replace_ignore_ascii_case(&out, needle, "%USERPROFILE%");
        }
        out
    }

    pub fn value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.text(s)),
            Value::Array(items) => Value::Array(items.iter().map(|v| self.value(v)).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (self.text(k), self.value(v)))
                    .collect::<Map<String, Value>>(),
            ),
            other => other.clone(),
        }
    }
}

/// ASCII case-insensitive replace (non-ASCII bytes must match exactly, so
/// UTF-8 boundaries are kept).
fn replace_ignore_ascii_case(haystack: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let hay = haystack.as_bytes();
    let pat = needle.as_bytes();
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    let mut i = 0;
    while i + pat.len() <= hay.len() {
        let end = i + pat.len();
        // `C:\Users\Marie` must not match inside `C:\Users\Marianne`.
        let boundary = hay
            .get(end)
            .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-'));
        if boundary && hay[i..end].eq_ignore_ascii_case(pat) {
            out.push_str(&haystack[last..i]);
            out.push_str(with);
            i += pat.len();
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&haystack[last..]);
    out
}

/// `DisplayVersion` (24H2), `CurrentBuild` and `UBR` from
/// `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`.
#[cfg(windows)]
fn windows_version() -> Value {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
    const KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    fn text(name: PCWSTR) -> Option<String> {
        let mut buf = [0u16; 256];
        let mut size = (buf.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
        };
        if !status.is_ok() {
            return None;
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
    let mut ubr = 0u32;
    let mut size = 4u32;
    let ubr = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            KEY,
            w!("UBR"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut ubr as *mut u32 as *mut _),
            Some(&mut size),
        )
    }
    .is_ok()
    .then_some(ubr);
    json!({
        "productName": text(w!("ProductName")),
        "displayVersion": text(w!("DisplayVersion")),
        "currentBuild": text(w!("CurrentBuild")),
        "ubr": ubr,
    })
}

#[cfg(not(windows))]
fn windows_version() -> Value {
    Value::Null
}

/// Every monitor with its rect, work area and scale (Win32, so the CLI
/// works without Tauri).
#[cfg(windows)]
fn monitors() -> Value {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    const MONITORINFOF_PRIMARY: u32 = 1;
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let out = &mut *(data.0 as *mut Vec<HMONITOR>);
        out.push(m);
        true.into()
    }
    let rect = |r: RECT| json!([r.left, r.top, r.right - r.left, r.bottom - r.top]);
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut handles as *mut _ as isize));
    }
    let list: Vec<Value> = handles
        .into_iter()
        .map(|m| unsafe {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            let ok = GetMonitorInfoW(m, &mut info as *mut _ as *mut _).as_bool();
            let (mut dx, mut dy) = (0u32, 0u32);
            let scale = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy)
                .is_ok()
                .then(|| dx as f64 / 96.0);
            if !ok {
                return json!({ "scale": scale });
            }
            let end = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
            json!({
                "name": String::from_utf16_lossy(&info.szDevice[..end]),
                "primary": info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                "rect": rect(info.monitorInfo.rcMonitor),
                "work": rect(info.monitorInfo.rcWork),
                "scale": scale,
            })
        })
        .collect();
    Value::Array(list)
}

#[cfg(not(windows))]
fn monitors() -> Value {
    Value::Array(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_keys_at_any_depth() {
        let mut config = json!({
            "autostart": true,
            "apiToken": "0b9d3f8e-1111",
            "launcher": { "hotkey": "Alt+Space" },
            "api": { "enabled": true, "port": 47831, "token": "abc" },
            "weather": { "apiKey": "k1", "api_key": "k2", "KEY": "k3", "keys": ["a"] },
            "mail": { "password": "p", "clientSecret": "s", "Authorization": "Bearer x" },
            "spawnables": { "bar": { "opts": { "hotkeys": ["Win+K"], "searchHf": "https://hf.co" } } },
            "shortcuts": [ { "n": "gh", "target": "https://github.com/", "authToken": "t" } ]
        });
        redact(&mut config);
        assert_eq!(config["autostart"], json!(true));
        assert_eq!(config["apiToken"], json!(REDACTED));
        assert_eq!(config["launcher"]["hotkey"], json!("Alt+Space"));
        assert_eq!(config["api"]["port"], json!(47831));
        assert_eq!(config["api"]["token"], json!(REDACTED));
        for k in ["apiKey", "api_key", "KEY", "keys"] {
            assert_eq!(config["weather"][k], json!(REDACTED), "{k}");
        }
        for k in ["password", "clientSecret", "Authorization"] {
            assert_eq!(config["mail"][k], json!(REDACTED), "{k}");
        }
        assert_eq!(config["spawnables"]["bar"]["opts"]["hotkeys"], json!(["Win+K"]));
        assert_eq!(config["shortcuts"][0]["target"], json!("https://github.com/"));
        assert_eq!(config["shortcuts"][0]["authToken"], json!(REDACTED));
    }

    #[test]
    fn secret_key_names() {
        for k in ["token", "apiToken", "SECRET", "passwordHash", "auth", "OAuth", "apikey", "privateKey", "hotkeyToken"] {
            assert!(is_secret_key(k), "{k}");
        }
        for k in ["hotkey", "hotkeys", "launcher", "autostart", "monitor", "page"] {
            assert!(!is_secret_key(k), "{k}");
        }
    }

    #[test]
    fn scrubs_the_profile_folder_in_every_spelling() {
        let s = Scrubber::new(Some(r"C:\Users\Marie"));
        assert_eq!(
            s.text(r"open C:\Users\Marie\Documents\Widgets\desktop\index.html"),
            r"open %USERPROFILE%\Documents\Widgets\desktop\index.html"
        );
        assert_eq!(s.text(r#"{"p":"C:\\Users\\Marie\\x"}"#), r#"{"p":"%USERPROFILE%\\x"}"#);
        assert_eq!(s.text("c:/users/MARIE/x and C:\\USERS\\marie"), "%USERPROFILE%/x and %USERPROFILE%");
        assert_eq!(s.text(r"C:\Users\Marianne\x"), r"C:\Users\Marianne\x");
        assert_eq!(s.text(r"C:\Users\Marie"), "%USERPROFILE%");
        assert_eq!(s.text("no paths here, ünïcode ok"), "no paths here, ünïcode ok");
        let v = s.value(&json!({ r"C:\Users\Marie\k": [r"C:\Users\Marie\a", 1] }));
        assert_eq!(v, json!({ r"%USERPROFILE%\k": [r"%USERPROFILE%\a", 1] }));
        // No profile: nothing to replace.
        assert_eq!(Scrubber::new(None).text(r"C:\Users\Marie"), r"C:\Users\Marie");
    }

    #[test]
    fn writes_a_zip_with_info_json() {
        let dir = std::env::temp_dir().join(format!("versailles-diag-zip-{}", uuid::Uuid::new_v4()));
        let path = write_bundle_to(&dir, None).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("versailles-diagnostics-") && name.ends_with(".zip"), "{name}");
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("info.json").unwrap(), &mut text).unwrap();
        let info: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(info["host"]["version"], json!(env!("CARGO_PKG_VERSION")));
        assert_eq!(info["host"]["from"], json!("--diagnostics"));
        assert!(info["monitors"].is_array());
        if let Ok(profile) = std::env::var("USERPROFILE") {
            assert!(!text.to_ascii_lowercase().contains(&profile.to_ascii_lowercase().replace('\\', "\\\\")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_config_and_pieces_without_writing() {
        let root = std::env::temp_dir().join(format!("versailles-diag-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("desktop")).unwrap();
        let html = r#"<html><head><script type="application/json" id="versailles">
{ "api": { "token": "t" }, "shell": { "hideTaskbar": true } }
</script></head><body>
<article class="widget" data-id="clock" data-hooks="media,storage"></article>
<template class="spawnable" data-id="bar" data-hooks="hotkey" data-hotkey="Alt+Space"></template>
</body></html>"#;
        std::fs::write(root.join("desktop").join("index.html"), html).unwrap();
        let page = read_page(&root);
        assert_eq!(page.config_from, Some("#versailles"));
        let mut config = page.config.unwrap();
        redact(&mut config);
        assert_eq!(config, json!({ "api": { "token": REDACTED }, "shell": { "hideTaskbar": true } }));
        let list = pieces(page.html.as_deref().unwrap());
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["id"], json!("clock"));
        assert_eq!(list[0]["hooks"], json!(["media", "storage"]));
        assert_eq!(list[1]["hotkey"], json!("Alt+Space"));
        let mut names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["desktop".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
