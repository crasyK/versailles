//! Installed-app catalog for the action bar.
//!
//! Discovers Start Menu / Desktop / browser-app shortcuts, maps them to
//! short launcher names, and resolves GUI executables so we never launch
//! `.cmd` / `.bat` shims (those pop a console and keep it open — Cursor/VS Code).
//!
//! Packaged apps (Settings, Calculator, Photos, Store installs) have no
//! shortcut file: they come from the shell's Applications folder
//! (`shell:AppsFolder`, `FOLDERID_AppsFolder`) by their AppUserModelID, are
//! launched through `IApplicationActivationManager` (or the shell), and get
//! their icons from `IShellItemImageFactory` like shortcuts do (#54).

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
    /// `"shortcut"` (a `.lnk` / `.url` file) or `"packaged"` (a Store /
    /// packaged app; `target` is `shell:AppsFolder\<aumid>`).
    pub kind: &'static str,
    /// AppUserModelID of a packaged app.
    pub aumid: Option<String>,
}

#[derive(Clone, Debug)]
struct RawApp {
    id: String,
    name: String,
    target: String,
    source: String,
    aliases: Vec<String>,
    kind: &'static str,
    aumid: Option<String>,
}

const SHORTCUT: &str = "shortcut";
const PACKAGED: &str = "packaged";
const APPS_FOLDER_PREFIX: &str = "shell:appsfolder\\";

struct CatalogCache {
    at: Instant,
    apps: Vec<RawApp>,
    /// Every AppUserModelID in the Applications folder (lowercase), packaged
    /// or not: what `shell:AppsFolder\…` may open.
    aumids: HashSet<String>,
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

/// URL handlers that turn "open this link" into code execution or credential
/// leaks (Follina `ms-msdt:`, `search-ms:` lures, Office `|u|` / `ofe|` URL forms,
/// App Installer). Matched on the scheme, case-insensitively.
const DENIED_SCHEMES: &[&str] = &[
    "ms-msdt",
    "search-ms",
    "search",
    "ms-officecmd",
    "ms-word",
    "ms-excel",
    "ms-powerpoint",
    "ms-visio",
    "ms-access",
    "ms-project",
    "ms-publisher",
    "ms-spd",
    "ms-infopath",
    "ms-officeapp",
    "ms-appinstaller",
    "ms-cxh",
    "ms-cxh-full",
];

/// `\\server\share`, `//server/share`, `\\?\UNC\…`, `\\.\device`: touching
/// these (even `Path::exists`) makes Windows authenticate over SMB and leak the
/// user's NTLM hash. Checked before any filesystem call.
fn is_unc_like(t: &str) -> bool {
    let t = t.trim_start();
    t.starts_with("\\\\") || t.starts_with("//") || t.starts_with("\\/") || t.starts_with("/\\")
}

/// Reason a launch target is refused, if it is. Pure; unit-tested below.
pub(crate) fn denied_target(t: &str) -> Option<&'static str> {
    let t = t.trim();
    if is_unc_like(t) {
        return Some("network (UNC) paths are not allowed");
    }
    let lower = t.to_ascii_lowercase();
    if let Some((scheme, rest)) = lower.split_once(':') {
        // Drive letters (`C:\…`) are paths, not schemes.
        let is_scheme = scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if is_scheme {
            if DENIED_SCHEMES.contains(&scheme) {
                return Some("this URL handler is not allowed");
            }
            if scheme.starts_with("ms-")
                && (rest.contains("|u|") || rest.contains("ofe|") || rest.contains("ofv|") || rest.contains("nft|"))
            {
                return Some("Office URL commands are not allowed");
            }
            if scheme == "file" {
                let after = rest.trim_start_matches(['/', '\\']);
                let slashes = rest.len() - after.len();
                // `file://host/…` (or `file:\\\\host`) reaches out over SMB; `file:///C:/…` is local.
                let host_form = slashes == 2 || slashes == 4 || (slashes >= 2 && !after.get(1..2).is_some_and(|c| c == ":"));
                if host_form && !after.starts_with("localhost/") {
                    return Some("file URLs with a host are not allowed");
                }
            }
        }
    }
    None
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
                kind: SHORTCUT,
                aumid: None,
            });
        }
    }
}

/// `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`: a package family name
/// (`<name>_<13-character publisher hash>`), `!`, an app id. Desktop apps in
/// the Applications folder have other ids (`Microsoft.Windows.Explorer`,
/// `{GUID}\path\app.exe`, `Chrome`).
pub(crate) fn is_packaged_aumid(aumid: &str) -> bool {
    let Some((family, app)) = aumid.split_once('!') else {
        return false;
    };
    let Some((name, hash)) = family.rsplit_once('_') else {
        return false;
    };
    !app.is_empty()
        && !name.is_empty()
        && hash.len() == 13
        && hash.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// An English short name from the package name, so `calc` finds Calculator
/// on a German Windows ("Rechner"): `Microsoft.WindowsCalculator` →
/// `calculator`, `Microsoft.Windows.Photos` → `photos`.
pub(crate) fn package_alias(aumid: &str) -> Option<String> {
    let family = aumid.split_once('!')?.0;
    let name = family.rsplit_once('_')?.0;
    let last = name.rsplit('.').next()?.to_ascii_lowercase();
    let short = last.strip_prefix("windows").filter(|s| s.len() >= 3).unwrap_or(&last);
    let short: String = short.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    (short.len() >= 3).then_some(short)
}

/// The packaged apps that are not already shortcuts: one with the same
/// name (case, spacing and version numbers aside) or the same target is a
/// duplicate.
fn merge_packaged(shortcuts: &[RawApp], packaged: Vec<(String, String)>) -> Vec<RawApp> {
    let names: HashSet<String> = shortcuts.iter().map(|a| slug(&a.name)).collect();
    let targets: HashSet<String> = shortcuts.iter().map(|a| a.target.to_ascii_lowercase()).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (name, aumid) in packaged {
        if !is_packaged_aumid(&aumid) || skip_shortcut_name(&name) {
            continue;
        }
        let slug_name = slug(&name);
        let target = format!("shell:AppsFolder\\{aumid}");
        if slug_name.len() < 2
            || names.contains(&slug_name)
            || targets.contains(&target.to_ascii_lowercase())
            || !seen.insert(aumid.to_ascii_lowercase())
        {
            continue;
        }
        let id = preferred_id(&slug_name);
        out.push(RawApp {
            aliases: aliases_for(&id, &name),
            id,
            name: strip_noise(&name),
            target,
            source: "apps-folder".into(),
            kind: PACKAGED,
            aumid: Some(aumid),
        });
    }
    out
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
        if let Some(alias) = app.aumid.as_deref().and_then(package_alias) {
            if !app.aliases.contains(&alias) {
                app.aliases.push(alias);
                app.aliases.sort();
            }
        }
        out.push(app);
    }
    out
}

fn scan_raw() -> Vec<RawApp> {
    scan().0
}

/// The catalog and the Applications folder's AppUserModelIDs, cached for
/// [`CACHE_TTL`].
fn scan() -> (Vec<RawApp>, HashSet<String>) {
    if let Ok(cache) = CACHE.lock() {
        if let Some(hit) = cache.as_ref() {
            if hit.at.elapsed() < CACHE_TTL {
                return (hit.apps.clone(), hit.aumids.clone());
            }
        }
    }
    let mut apps = Vec::new();
    for (root, source) in start_menu_roots() {
        if root.is_dir() {
            walk_shortcuts(&root, source, &mut apps);
        }
    }
    let folder = apps_folder_cached();
    let aumids: HashSet<String> = folder.iter().map(|(_, a)| a.to_ascii_lowercase()).collect();
    let packaged = merge_packaged(&apps, folder);
    apps.extend(packaged);
    let apps = dedupe(apps);
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some(CatalogCache {
            at: Instant::now(),
            apps: apps.clone(),
            aumids: aumids.clone(),
        });
    }
    (apps, aumids)
}

/// The Applications folder takes 0.2–0.4 s to list and changes only when
/// apps are installed: kept longer than the shortcut scan.
const APPS_FOLDER_TTL: Duration = Duration::from_secs(120);

fn apps_folder_cached() -> Vec<(String, String)> {
    static FOLDER: Mutex<Option<(Instant, Vec<(String, String)>)>> = Mutex::new(None);
    if let Some((at, list)) = FOLDER.lock().unwrap().as_ref() {
        if at.elapsed() < APPS_FOLDER_TTL {
            return list.clone();
        }
    }
    let list = imp::apps_folder().unwrap_or_else(|e| {
        tracing::debug!("catalog: Applications folder unavailable: {e}");
        Vec::new()
    });
    *FOLDER.lock().unwrap() = Some((Instant::now(), list.clone()));
    list
}

/// The AppUserModelID in `shell:AppsFolder\<aumid>`, if `target` is one.
pub fn apps_folder_aumid(target: &str) -> Option<&str> {
    let t = target.trim();
    t.get(..APPS_FOLDER_PREFIX.len())
        .filter(|p| p.eq_ignore_ascii_case(APPS_FOLDER_PREFIX))
        .map(|_| t[APPS_FOLDER_PREFIX.len()..].trim_matches(['\\', '/']))
        .filter(|a| !a.is_empty())
}

/// `shell:AppsFolder\<aumid>` opens only an app that is in the Applications
/// folder: never an arbitrary id.
fn check_apps_folder(target: &str) -> Result<(), String> {
    let Some(aumid) = apps_folder_aumid(target) else {
        return Ok(());
    };
    if scan().1.contains(&aumid.to_ascii_lowercase()) {
        Ok(())
    } else {
        Err(format!("refused to open '{target}': no installed app has that id"))
    }
}

/// Number of items in the Applications folder and how many of them are
/// packaged (the live test reports these).
#[cfg(test)]
pub(crate) fn apps_folder_counts() -> (usize, usize) {
    let folder = imp::apps_folder().unwrap_or_default();
    let packaged = folder.iter().filter(|(_, a)| is_packaged_aumid(a)).count();
    (folder.len(), packaged)
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
    // Before any Path::exists(): UNC probes leak NTLM credentials.
    if let Some(reason) = denied_target(t) {
        return Err(format!("refused to open '{t}': {reason}"));
    }
    if is_url_or_protocol(t) {
        check_apps_folder(t)?;
        return Ok(t.to_string());
    }
    let path = Path::new(t);
    if path.is_absolute() && path.exists() {
        return Ok(unwrap_console_shim(path).to_string_lossy().into_owned());
    }
    let apps = scan_raw();
    if let Some(hit) = find_in_raw(&apps, t) {
        if denied_target(&hit.target).is_some() {
            return Err(format!("refused to open '{t}': shortcut points to a network path"));
        }
        if hit.kind == PACKAGED {
            return Ok(hit.target.clone());
        }
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
    // Packaged apps joined the catalog later (0.11.1): until one of them has
    // been seen, they are all known already, not dozens of "new" apps.
    let packaged_first_run = !apps.iter().any(|a| a.kind == PACKAGED && seen.contains(&a.id));
    apps.into_iter()
        .map(|a| {
            let is_hidden = hidden.contains(&a.id);
            let quiet = first_run || (a.kind == PACKAGED && packaged_first_run);
            CatalogEntry {
                fresh: !quiet && !seen.contains(&a.id) && !is_hidden,
                hidden: is_hidden,
                id: a.id,
                name: a.name,
                target: a.target,
                source: a.source,
                aliases: a.aliases,
                kind: a.kind,
                aumid: a.aumid,
            }
        })
        .collect()
}

/// Start Menu / Desktop walk off the UI thread (cold scans take a while).
async fn scan_raw_async() -> AppResult<Vec<RawApp>> {
    tauri::async_runtime::spawn_blocking(scan_raw)
        .await
        .map_err(|e| crate::error::AppError::msg(format!("catalog scan failed: {e}")))
}

fn visible_entries(state: &AppState, apps: Vec<RawApp>) -> Vec<CatalogEntry> {
    let catalog = state.config.lock().unwrap().catalog.clone();
    let mut entries = decorate(apps, &catalog);
    entries.retain(|e| !e.hidden);
    entries
}

#[tauri::command]
pub async fn list_catalog(state: State<'_, AppState>) -> AppResult<Vec<CatalogEntry>> {
    let apps = scan_raw_async().await?;
    Ok(visible_entries(&state, apps))
}

#[tauri::command]
pub async fn ack_catalog(state: State<'_, AppState>) -> AppResult<Vec<CatalogEntry>> {
    let apps = scan_raw_async().await?;
    let ids: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
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
    Ok(visible_entries(&state, apps))
}

#[tauri::command]
pub async fn hide_catalog_entry(
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
    let apps = scan_raw_async().await?;
    Ok(visible_entries(&state, apps))
}

/// Icons by what they were drawn from (`.lnk` path or `shell:AppsFolder\…`).
fn icon_cache() -> &'static Mutex<std::collections::HashMap<String, Option<String>>> {
    static CACHE: std::sync::OnceLock<Mutex<std::collections::HashMap<String, Option<String>>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn icon_for_id(id: &str) -> Option<String> {
    let id = id.trim().to_ascii_lowercase();
    let apps = scan_raw();
    let app = apps.iter().find(|a| a.id == id)?;
    // Never parse a network path (the shell would reach out over SMB).
    if denied_target(&app.target).is_some() {
        return None;
    }
    let key = app.target.to_ascii_lowercase();
    if let Some(hit) = icon_cache().lock().unwrap().get(&key) {
        return hit.clone();
    }
    let url = imp::shell_icon(&app.target, ICON_PX);
    icon_cache().lock().unwrap().insert(key, url.clone());
    url
}

const ICON_PX: i32 = 32;

/// A catalog app's icon as a 32×32 PNG data URL (`null` when it has none).
/// Same reach as `list_catalog`: only apps in the catalog, by id.
#[tauri::command]
pub async fn catalog_icon(id: String) -> AppResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || icon_for_id(&id))
        .await
        .map_err(|e| crate::error::AppError::msg(format!("catalog icon failed: {e}")))
}

/// Start a packaged app by its AppUserModelID (the caller has COM up).
#[cfg(windows)]
pub fn activate_packaged(aumid: &str) -> Result<(), String> {
    imp::activate(aumid)
}

#[cfg(windows)]
mod imp {
    use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{PROPERTYKEY, SIZE};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_LOCAL_SERVER,
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{
        ApplicationActivationManager, BHID_EnumItems, IApplicationActivationManager, IEnumShellItems,
        IShellItem, IShellItem2, IShellItemImageFactory, SHCreateItemFromParsingName, SHGetKnownFolderItem,
        AO_NONE, FOLDERID_AppsFolder, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIIGBF_ICONONLY,
    };

    /// `System.AppUserModel.ID`.
    const PKEY_AUMID: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };

    /// COM in a single-threaded apartment for shell objects, undone on drop
    /// when this call started it.
    struct Sta(bool);
    impl Sta {
        fn new() -> Self {
            Sta(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.is_ok())
        }
    }
    impl Drop for Sta {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    unsafe fn take(p: PWSTR) -> Option<String> {
        if p.is_null() {
            return None;
        }
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.filter(|s| !s.is_empty())
    }

    /// (display name, AppUserModelID) of every app in the Applications folder.
    pub fn apps_folder() -> Result<Vec<(String, String)>, String> {
        let _com = Sta::new();
        let err = |e: windows::core::Error| format!("AppsFolder: {e}");
        unsafe {
            let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).map_err(err)?;
            let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems).map_err(err)?;
            let mut out = Vec::new();
            loop {
                let mut batch: [Option<IShellItem>; 32] = Default::default();
                let mut got = 0u32;
                if items.Next(&mut batch, Some(&mut got)).is_err() || got == 0 {
                    break;
                }
                for item in batch.iter().take(got as usize).flatten() {
                    let Some(name) = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok().and_then(|p| take(p)) else {
                        continue;
                    };
                    let Some(aumid) = item
                        .cast::<IShellItem2>()
                        .ok()
                        .and_then(|i2| i2.GetString(&PKEY_AUMID).ok())
                        .and_then(|p| take(p))
                    else {
                        continue;
                    };
                    out.push((name, aumid));
                }
            }
            Ok(out)
        }
    }

    /// The shell's icon for a path or parsing name, `px`×`px`, as a PNG data URL.
    pub fn shell_icon(parsing: &str, px: i32) -> Option<String> {
        use base64::Engine;
        let _com = Sta::new();
        unsafe {
            let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(parsing), None).ok()?;
            let factory: IShellItemImageFactory = item.cast().ok()?;
            let bitmap = factory.GetImage(SIZE { cx: px, cy: px }, SIIGBF_ICONONLY).ok()?;
            let mut info = BITMAP::default();
            let ok = GetObjectW(
                HGDIOBJ(bitmap.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut info as *mut BITMAP as *mut _),
            ) != 0;
            let (w, h) = (info.bmWidth, info.bmHeight.abs());
            let mut pixels = vec![0u8; (w.max(0) * h * 4) as usize];
            let mut got = 0;
            if ok && w > 0 && h > 0 && w <= 256 && h <= 256 {
                let dc = CreateCompatibleDC(None);
                let mut bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h, // top-down
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                got = GetDIBits(dc, bitmap, 0, h as u32, Some(pixels.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
                let _ = DeleteDC(dc);
            }
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            if got == 0 {
                return None;
            }
            // Premultiplied BGRA → straight RGBA (no alpha at all: opaque).
            let rgba = crate::app_windows::bgra_to_rgba(&pixels, None);
            let png = crate::app_windows::png_rgba(w as u32, h as u32, &rgba);
            Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
        }
    }

    pub fn activate(aumid: &str) -> Result<(), String> {
        unsafe {
            let manager: IApplicationActivationManager =
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)
                    .map_err(|e| format!("activation: {e}"))?;
            manager
                .ActivateApplication(&HSTRING::from(aumid), PCWSTR::null(), AO_NONE)
                .map(|_| ())
                .map_err(|e| format!("activation of {aumid}: {e}"))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn apps_folder() -> Result<Vec<(String, String)>, String> {
        Ok(Vec::new())
    }
    pub fn shell_icon(_: &str, _: i32) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unc_before_touching_the_filesystem() {
        for t in [
            r"\\attacker\share\x.exe",
            r"\\?\UNC\attacker\share",
            r"\\.\pipe\x",
            "//attacker/share/x",
            r"\/attacker/share",
            "  \\\\attacker\\share",
        ] {
            assert!(denied_target(t).is_some(), "{t}");
            assert!(resolve_launch_target(t).is_err(), "{t}");
        }
    }

    #[test]
    fn refuses_dangerous_url_handlers() {
        for t in [
            "ms-msdt:/id PCWDiagnostic /skip force",
            "MS-MSDT:foo",
            "search-ms:query=x&crumb=location:\\\\attacker\\s",
            "search:query=x",
            "ms-officecmd:{\"id\":3}",
            "ms-word:ofe|u|https://attacker/x.docx",
            "ms-excel:ofv|u|https://attacker/x.xlsx",
            "ms-powerpoint:nft|u|https://attacker/x",
            "ms-appinstaller:?source=https://attacker/x.appinstaller",
            "ms-somethingnew:ofe|u|https://attacker/x",
            "file://attacker/share/x",
            "FILE:\\\\attacker\\share",
        ] {
            assert!(denied_target(t).is_some(), "{t}");
            assert!(resolve_launch_target(t).is_err(), "{t}");
        }
    }

    #[test]
    fn live_desktop_shortcut_targets_still_pass_the_filter() {
        // Every shortcut target in the user's `#versailles` block (and bar.js presets).
        for t in [
            "https://mail.google.com/mail/u/0/#inbox",
            "https://www.figma.com/files/team/907966426820539674/recents-and-sharing?fuid=907966424690731585",
            "http://homeserver-laptop-mark.tail186368.ts.net/#/",
            "https://owa.kit.edu/owa/#path=/mail",
            "ms-settings:",
            "ms-settings:display",
            "mailto:someone@example.com",
            "shell:Downloads",
            "steam://run/1",
            "onenote:",
            r"C:\Users\me\Downloads",
            r"C:\Users\me\Documents\Projects",
            "C:/Users/me/Pictures",
            "calc.exe",
            "cursor",
            "code",
            "notepad",
            "explorer",
            "taskmgr",
            "wt",
            "file:///C:/Users/me/a.txt",
        ] {
            assert!(denied_target(t).is_none(), "{t}");
        }
        assert!(resolve_launch_target("https://github.com/").is_ok());
        assert!(resolve_launch_target("ms-settings:").is_ok());
        assert!(resolve_launch_target("notepad").is_ok());
    }

    fn shortcut(name: &str, target: &str) -> RawApp {
        let id = preferred_id(&slug(name));
        RawApp {
            aliases: aliases_for(&id, name),
            id,
            name: name.into(),
            target: target.into(),
            source: "start-menu".into(),
            kind: SHORTCUT,
            aumid: None,
        }
    }

    #[test]
    fn recognizes_packaged_aumids() {
        assert!(is_packaged_aumid("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(is_packaged_aumid("windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"));
        assert!(!is_packaged_aumid("Microsoft.Windows.Explorer"));
        assert!(!is_packaged_aumid(r"{6D809377-6AF0-444B-8957-A3773F02200E}\Notepad++\notepad++.exe"));
        assert!(!is_packaged_aumid("Chrome"));
        assert!(!is_packaged_aumid("Foo_short!App"));
        assert!(!is_packaged_aumid("Foo_8wekyb3d8bbwe!"));
        assert_eq!(package_alias("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App").as_deref(), Some("calculator"));
        assert_eq!(package_alias("Microsoft.Windows.Photos_8wekyb3d8bbwe!App").as_deref(), Some("photos"));
        assert_eq!(package_alias("Microsoft.ScreenSketch_8wekyb3d8bbwe!App").as_deref(), Some("screensketch"));
        assert_eq!(package_alias("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App").as_deref(), Some("terminal"));
        assert_eq!(package_alias("Chrome"), None);
    }

    #[test]
    fn packaged_apps_dedupe_against_shortcuts() {
        let shortcuts = vec![
            shortcut("Spotify", r"C:\Start\Spotify.lnk"),
            shortcut("Visual Studio Code", r"C:\Start\Visual Studio Code.lnk"),
            shortcut("WhatsApp shortcut", "shell:AppsFolder\\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"),
        ];
        let folder = vec![
            ("Rechner".to_string(), "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".to_string()),
            // Same name as a shortcut (the Store build of an app you also have).
            ("Spotify".to_string(), "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify".to_string()),
            // Same target as a shortcut.
            ("WhatsApp".to_string(), "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App".to_string()),
            // Desktop apps are shortcuts already.
            ("Explorer".to_string(), "Microsoft.Windows.Explorer".to_string()),
            // Listed twice.
            ("Rechner".to_string(), "microsoft.windowscalculator_8wekyb3d8bbwe!App".to_string()),
            ("Get Help".to_string(), "Microsoft.GetHelp_8wekyb3d8bbwe!App".to_string()),
        ];
        let merged = merge_packaged(&shortcuts, folder);
        assert_eq!(merged.len(), 1, "{merged:?}");
        let calc = &merged[0];
        assert_eq!(calc.id, "rechner");
        assert_eq!(calc.kind, PACKAGED);
        assert_eq!(calc.target, "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App");
        assert_eq!(calc.aumid.as_deref(), Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        // dedupe() adds the English package alias.
        let all = dedupe(shortcuts.into_iter().chain(merged).collect());
        let calc = all.iter().find(|a| a.id == "rechner").unwrap();
        assert!(calc.aliases.iter().any(|a| a == "calculator"), "{:?}", calc.aliases);
    }

    #[test]
    fn packaged_apps_are_not_all_new_at_once() {
        let mut calc = shortcut("Rechner", "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App");
        calc.kind = PACKAGED;
        let mut photos = shortcut("Fotos", "shell:AppsFolder\\Microsoft.Windows.Photos_8wekyb3d8bbwe!App");
        photos.kind = PACKAGED;
        let code = shortcut("Visual Studio Code", r"C:\Start\Code.lnk");
        let newer = shortcut("Blender", r"C:\Start\Blender.lnk");
        let seen = |ids: &[&str]| CatalogState {
            seen_ids: ids.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let fresh = |entries: Vec<CatalogEntry>| -> Vec<String> {
            entries.into_iter().filter(|e| e.fresh).map(|e| e.id).collect()
        };
        let apps = vec![calc.clone(), photos.clone(), code.clone(), newer.clone()];
        // Upgrading: shortcuts seen before, packaged apps never seen: only the new shortcut is new.
        assert_eq!(fresh(decorate(apps.clone(), &seen(&["code"]))), ["blender"]);
        // Once packaged apps were acknowledged, a new one is flagged.
        assert_eq!(fresh(decorate(apps.clone(), &seen(&["code", "blender", "rechner"]))), ["fotos"]);
        // First run ever: nothing is new.
        assert!(fresh(decorate(apps, &seen(&[]))).is_empty());
    }

    #[test]
    fn apps_folder_targets_need_a_known_id() {
        assert_eq!(apps_folder_aumid("shell:AppsFolder\\Foo_8wekyb3d8bbwe!App"), Some("Foo_8wekyb3d8bbwe!App"));
        assert_eq!(apps_folder_aumid("SHELL:appsfolder\\X"), Some("X"));
        assert_eq!(apps_folder_aumid("shell:Downloads"), None);
        assert_eq!(apps_folder_aumid("shell:AppsFolder\\"), None);
        let err = resolve_launch_target("shell:AppsFolder\\Not.A.Real.App_0123456789abc!Nope").unwrap_err();
        assert!(err.contains("no installed app"), "{err}");
        assert!(resolve_launch_target("shell:Downloads").is_ok());
    }

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
