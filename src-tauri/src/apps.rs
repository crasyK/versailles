//! Installed-app catalog for the action bar.
//!
//! Discovers Start Menu / Desktop / browser-app shortcuts, maps them to
//! short launcher names, and resolves GUI executables so we never launch
//! `.cmd` / `.bat` shims (those pop a console and keep it open — Cursor/VS Code).

use crate::config::CatalogState;
use crate::error::AppResult;
use crate::state::AppState;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

const CACHE_TTL: Duration = Duration::from_secs(20);

const SKIP_DIRS: &[&str] = &[
    "administrative tools",
    "accessibility",
    "startup",
    "windows powershell",
    "microsoft office tools",
    "visual studio 2022",
    "visual studio tools",
    "tex live 2025",
    "python 3.10",
    "python 3.11",
    "python 3.12",
    "python 3.13",
    "maintenance",
    "nvm for windows",
    "logi",
    "logi download assistant",
    "dell",
    "intel",
    "intel graphics software",
];

const SKIP_NAME_PARTS: &[&str] = &[
    "uninstall",
    "help",
    "release notes",
    "documentation",
    "manuals",
    "website",
    "readme",
    "command prompt",
    "module docs",
    "language preferences",
    "reset preferences",
    "installer",
    "download assistant",
    "skinned",
    "create usb",
    "git cmd",
    "git gui",
];

/// Well-known short names so "code" launches VS Code, "paint" launches Paint.NET, etc.
const PREFERRED_IDS: &[(&str, &str)] = &[
    ("visualstudiocode", "code"),
    ("paintdotnet", "paint"),
    ("paintnet", "paint"),
    ("microsoftpaint", "paint"),
    ("googlechrome", "chrome"),
    ("microsoftedge", "edge"),
    ("windowsterminal", "wt"),
    ("fileexplorer", "explorer"),
    ("vlcmediaplayer", "vlc"),
    ("7zipfilemanager", "7zip"),
    ("protonvpn", "protonvpn"),
    ("microsoftonenote", "onenote"),
    ("onenote", "onenote"),
];

const EXTRA_ALIASES: &[(&str, &[&str])] = &[
    ("code", &["vscode"]),
    ("paint", &["pdn", "paintdotnet"]),
    ("protonvpn", &["vpn", "proton"]),
    ("rexprint", &["rex"]),
    ("lightworks", &["lwks"]),
    ("blender", &["blender3d"]),
    ("onenote", &["note"]),
    ("chrome", &["googlechrome"]),
    ("wt", &["terminal", "windowsterminal"]),
    ("explorer", &["files", "fileexplorer"]),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub target: String,
    pub source: String,
    pub aliases: Vec<String>,
    pub fresh: bool,
    pub hidden: bool,
}

#[derive(Clone)]
struct RawApp {
    id: String,
    name: String,
    target: String,
    source: String,
    aliases: Vec<String>,
}

struct CatalogCache {
    at: Instant,
    apps: Vec<RawApp>,
}

static CACHE: Mutex<Option<CatalogCache>> = Mutex::new(None);

fn is_url_or_protocol(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ms-")
        || lower.starts_with("shell:")
        || lower.starts_with("steam:")
        || lower.starts_with("onenote:")
}

fn skip_dir(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SKIP_DIRS.iter().any(|s| n == *s)
}

fn skip_shortcut_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SKIP_NAME_PARTS.iter().any(|part| n.contains(part))
}

fn strip_noise(name: &str) -> String {
    let mut s = name.trim().to_string();
    if let Some(start) = s.rfind('(') {
        if s.ends_with(')') {
            s = s[..start].trim().to_string();
        }
    }
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() > 1 {
        if let Some(last) = parts.last() {
            if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return parts[..parts.len() - 1].join(" ");
            }
        }
    }
    s
}

fn slug(name: &str) -> String {
    strip_noise(name)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn preferred_id(slug_name: &str) -> String {
    PREFERRED_IDS
        .iter()
        .find(|(from, _)| *from == slug_name)
        .map(|(_, to)| (*to).to_string())
        .unwrap_or_else(|| slug_name.to_string())
}

fn aliases_for(id: &str, name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push(id.to_string());
    let s = slug(name);
    if s != id && !s.is_empty() {
        out.push(s);
    }
    if let Some((_, extra)) = EXTRA_ALIASES.iter().find(|(key, _)| *key == id) {
        out.extend(extra.iter().map(|a| (*a).to_string()));
    }
    let first = strip_noise(name)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let weak_first = matches!(
        first.as_str(),
        "microsoft" | "google" | "windows" | "visual" | "adobe" | "intel" | "dell"
    );
    if first.len() >= 3
        && first != id
        && first.chars().all(|c| c.is_ascii_alphanumeric())
        && !weak_first
    {
        out.push(first);
    }
    out.sort();
    out.dedup();
    out
}

fn start_menu_roots() -> Vec<(PathBuf, &'static str)> {
    let mut roots = Vec::new();
    if let Some(roaming) = dirs::config_dir() {
        roots.push((
            roaming.join("Microsoft/Windows/Start Menu/Programs"),
            "start-menu",
        ));
        roots.push((
            roaming.join("Microsoft/Windows/Start Menu/Programs/Chrome Apps"),
            "chrome-app",
        ));
        roots.push((
            roaming.join("Microsoft/Windows/Start Menu/Programs/Brave Apps"),
            "chrome-app",
        ));
        roots.push((
            roaming.join("Microsoft/Windows/Start Menu/Programs/Edge Apps"),
            "chrome-app",
        ));
    }
    if let Some(prog) = std::env::var_os("ProgramData") {
        roots.push((
            PathBuf::from(prog).join("Microsoft/Windows/Start Menu/Programs"),
            "start-menu",
        ));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push((home.join("Desktop"), "desktop"));
    }
    if let Some(public) = std::env::var_os("PUBLIC") {
        roots.push((PathBuf::from(public).join("Desktop"), "desktop"));
    }
    roots
}

fn walk_shortcuts(root: &Path, source: &str, out: &mut Vec<RawApp>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                if !skip_dir(&name) {
                    stack.push(path);
                }
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext != "lnk" && ext != "url" {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&name)
                .to_string();
            if skip_shortcut_name(&stem) {
                continue;
            }
            let slug_name = slug(&stem);
            if slug_name.len() < 2 {
                continue;
            }
            let id = preferred_id(&slug_name);
            out.push(RawApp {
                id,
                name: strip_noise(&stem),
                target: path.to_string_lossy().into_owned(),
                source: source.to_string(),
                aliases: aliases_for(&preferred_id(&slug_name), &stem),
            });
        }
    }
}

fn dedupe(mut apps: Vec<RawApp>) -> Vec<RawApp> {
    let mut seen_targets = HashSet::new();
    let mut used_ids = HashSet::new();
    let mut out = Vec::new();
    apps.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    for mut app in apps {
        let target_key = app.target.to_ascii_lowercase();
        if !seen_targets.insert(target_key) {
            continue;
        }
        let mut id = app.id.clone();
        if !used_ids.insert(id.clone()) {
            let mut n = 2;
            loop {
                let candidate = format!("{id}{n}");
                if used_ids.insert(candidate.clone()) {
                    id = candidate;
                    break;
                }
                n += 1;
            }
            app.id = id.clone();
        }
        app.aliases = aliases_for(&app.id, &app.name);
        out.push(app);
    }
    out
}

fn scan_raw() -> Vec<RawApp> {
    if let Ok(cache) = CACHE.lock() {
        if let Some(hit) = cache.as_ref() {
            if hit.at.elapsed() < CACHE_TTL {
                return hit.apps.clone();
            }
        }
    }
    let mut apps = Vec::new();
    for (root, source) in start_menu_roots() {
        if root.is_dir() {
            walk_shortcuts(&root, source, &mut apps);
        }
    }
    let apps = dedupe(apps);
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some(CatalogCache {
            at: Instant::now(),
            apps: apps.clone(),
        });
    }
    apps
}

fn invalidate_cache() {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = None;
    }
}

fn unwrap_console_shim(path: &Path) -> PathBuf {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "cmd" && ext != "bat" {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut names = vec![
        format!("{stem}.exe"),
        format!("{}.exe", capitalize_ascii(&stem)),
    ];
    match stem.to_ascii_lowercase().as_str() {
        "cursor" => names.push("Cursor.exe".into()),
        "code" => names.push("Code.exe".into()),
        "wt" => names.push("WindowsTerminal.exe".into()),
        _ => {}
    }
    let mut dir = path.parent();
    for _ in 0..6 {
        let Some(d) = dir else { break };
        for name in &names {
            let candidate = d.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
        dir = d.parent();
    }
    path.to_path_buf()
}

fn capitalize_ascii(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn find_gui_on_path(name: &str) -> Option<PathBuf> {
    let exe_name = if name.to_ascii_lowercase().ends_with(".exe") {
        name.to_string()
    } else {
        format!("{name}.exe")
    };
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(&exe_name);
        if candidate.is_file() {
            return Some(unwrap_console_shim(&candidate));
        }
    }
    // Last resort: a .cmd shim on PATH, then walk up to the GUI exe.
    let cmd_name = format!("{name}.cmd");
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let candidate = dir.join(&cmd_name);
        if candidate.is_file() {
            let resolved = unwrap_console_shim(&candidate);
            if resolved != candidate {
                return Some(resolved);
            }
        }
    }
    None
}

fn find_in_raw<'a>(apps: &'a [RawApp], query: &str) -> Option<&'a RawApp> {
    let q = query.to_ascii_lowercase();
    apps.iter()
        .find(|a| a.id == q)
        .or_else(|| apps.iter().find(|a| a.aliases.iter().any(|al| al == &q)))
}

/// Resolve a launcher target to something ShellExecute can open without a console.
pub fn resolve_launch_target(target: &str) -> Result<String, String> {
    let t = target.trim();
    if t.is_empty() {
        return Err("cli_open: empty target".into());
    }
    if is_url_or_protocol(t) {
        return Ok(t.to_string());
    }
    let path = Path::new(t);
    if path.is_absolute() && path.exists() {
        return Ok(unwrap_console_shim(path).to_string_lossy().into_owned());
    }
    let apps = scan_raw();
    if let Some(hit) = find_in_raw(&apps, t) {
        let p = Path::new(&hit.target);
        if p.exists() {
            return Ok(unwrap_console_shim(p).to_string_lossy().into_owned());
        }
        return Ok(hit.target.clone());
    }
    if let Some(exe) = find_gui_on_path(t) {
        return Ok(exe.to_string_lossy().into_owned());
    }
    Err(format!(
        "refused to open '{t}': only http(s)/ms-/shell: URLs, existing paths, or installed apps are allowed"
    ))
}

fn decorate(apps: Vec<RawApp>, catalog: &CatalogState) -> Vec<CatalogEntry> {
    let seen: HashSet<String> = catalog.seen_ids.iter().cloned().collect();
    let hidden: HashSet<String> = catalog.hidden_ids.iter().cloned().collect();
    let first_run = seen.is_empty();
    apps.into_iter()
        .map(|a| {
            let is_hidden = hidden.contains(&a.id);
            CatalogEntry {
                fresh: !first_run && !seen.contains(&a.id) && !is_hidden,
                hidden: is_hidden,
                id: a.id,
                name: a.name,
                target: a.target,
                source: a.source,
                aliases: a.aliases,
            }
        })
        .collect()
}

#[tauri::command]
pub fn list_catalog(state: State<'_, AppState>) -> AppResult<Vec<CatalogEntry>> {
    let catalog = state.config.lock().unwrap().catalog.clone();
    let mut entries = decorate(scan_raw(), &catalog);
    entries.retain(|e| !e.hidden);
    Ok(entries)
}

#[tauri::command]
pub fn ack_catalog(state: State<'_, AppState>) -> AppResult<Vec<CatalogEntry>> {
    let ids: Vec<String> = scan_raw().into_iter().map(|a| a.id).collect();
    {
        let mut config = state.config.lock().unwrap();
        let mut set: HashSet<String> = config.catalog.seen_ids.iter().cloned().collect();
        let mut changed = false;
        for id in &ids {
            if set.insert(id.clone()) {
                changed = true;
            }
        }
        if changed {
            config.catalog.seen_ids = set.into_iter().collect();
            config.catalog.seen_ids.sort();
            state.store.lock().unwrap().save_runtime_from_app(&config)?;
        }
    }
    list_catalog(state)
}

#[tauri::command]
pub fn hide_catalog_entry(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<Vec<CatalogEntry>> {
    let id = id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return Err(crate::error::AppError::msg("hide: missing app id"));
    }
    {
        let mut config = state.config.lock().unwrap();
        if !config.catalog.hidden_ids.iter().any(|h| h == &id) {
            config.catalog.hidden_ids.push(id);
            config.catalog.hidden_ids.sort();
            state.store.lock().unwrap().save_runtime_from_app(&config)?;
        }
    }
    invalidate_cache();
    list_catalog(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_versions_and_parentheses() {
        assert_eq!(strip_noise("Blender 5.0"), "Blender");
        assert_eq!(strip_noise("lightworks 2025.2"), "lightworks");
        assert_eq!(strip_noise("REXprint (x64)"), "REXprint");
    }

    #[test]
    fn preferred_names() {
        assert_eq!(preferred_id(&slug("Visual Studio Code")), "code");
        assert_eq!(preferred_id(&slug("Paint.NET")), "paint");
        assert_eq!(slug("Paint.NET"), "paintnet");
        assert_eq!(preferred_id(&slug("Proton VPN")), "protonvpn");
        assert_eq!(preferred_id(&slug("Cursor")), "cursor");
        assert_eq!(preferred_id(&slug("OneNote")), "onenote");
    }

    #[test]
    fn skips_uninstallers() {
        assert!(skip_shortcut_name("Uninstall Lightworks"));
        assert!(skip_shortcut_name("Git CMD"));
        assert!(!skip_shortcut_name("REXprint"));
        assert!(!skip_shortcut_name("Cursor"));
    }

    #[test]
    fn unwraps_cursor_and_code_shims() {
        let cursor_cmd = PathBuf::from(r"C:\Program Files\cursor\resources\app\bin\cursor.cmd");
        if cursor_cmd.exists() {
            let exe = unwrap_console_shim(&cursor_cmd);
            assert!(
                exe.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("Cursor.exe")),
                "shim resolved to {}",
                exe.display()
            );
        }
        let code_cmd = PathBuf::from(r"C:\Program Files\Microsoft VS Code\bin\code.cmd");
        if code_cmd.exists() {
            let exe = unwrap_console_shim(&code_cmd);
            assert!(
                exe.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("Code.exe")),
                "shim resolved to {}",
                exe.display()
            );
        }
    }

    #[test]
    fn catalog_includes_installed_apps() {
        let apps = scan_raw();
        let ids: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
        for expected in ["cursor", "code", "onenote", "blender", "rexprint", "protonvpn", "lightworks", "paint"]
        {
            assert!(
                ids.iter().any(|id| id == expected),
                "missing {expected} in {ids:?}"
            );
        }
        let cursor = apps.iter().find(|a| a.id == "cursor").unwrap();
        assert!(
            cursor.target.to_ascii_lowercase().ends_with(".lnk"),
            "cursor should launch via Start Menu shortcut, got {}",
            cursor.target
        );
    }

    #[test]
    fn resolve_cursor_does_not_use_cmd_shim() {
        let resolved = resolve_launch_target("cursor").expect("cursor should resolve");
        let lower = resolved.to_ascii_lowercase();
        assert!(!lower.ends_with(".cmd") && !lower.ends_with(".bat"), "{resolved}");
        assert!(
            lower.ends_with(".lnk") || lower.ends_with("cursor.exe"),
            "{resolved}"
        );
    }
}
