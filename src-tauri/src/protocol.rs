//! `/files/*` resolution for the localhost file server.
//!
//! Axum's `Path<String>` extractor has already percent-decoded the request path,
//! so nothing here decodes again (a second pass turned `%25a%C3%A9` into a
//! mid-char slice and aborted the whole process). Only `desktop/**`, `.sdk/**`
//! and folder widgets the registry recognizes are served; everything else in
//! `Documents\Widgets` (the host repo, archive, sidecars) is a 404.

use crate::error::{AppError, AppResult};
use crate::registry::widgets_root;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Folders (and single files) under the widgets root that `/files` may serve.
/// Entries are lowercase, `/`-separated, relative to the root, no trailing slash.
#[derive(Debug, Clone, Default)]
pub struct FileScope {
    dirs: Vec<String>,
    files: Vec<String>,
}

impl FileScope {
    /// `desktop/**` + `.sdk/**`, the configured desktop page (and its folder when
    /// it is not the root itself), plus every registered folder widget.
    pub fn new(page_rel: &str, widget_dirs: &[PathBuf], root: &Path) -> Self {
        let mut scope = Self {
            dirs: vec!["desktop".into(), ".sdk".into()],
            files: Vec::new(),
        };
        let page = normalize_rel(page_rel);
        if !page.is_empty() {
            if let Some((dir, _)) = page.rsplit_once('/') {
                scope.push_dir(dir);
            }
            scope.files.push(page);
        }
        for dir in widget_dirs {
            if let Ok(rel) = dir.strip_prefix(root) {
                scope.push_dir(&normalize_rel(&rel.to_string_lossy()));
            }
        }
        scope
    }

    fn push_dir(&mut self, dir: &str) {
        let dir = normalize_rel(dir);
        // Never let a widget/page entry widen the scope to the whole root or to
        // host-private state.
        if dir.is_empty() || first_segment(&dir) == ".versailles" {
            return;
        }
        if !self.dirs.contains(&dir) {
            self.dirs.push(dir);
        }
    }

    pub fn allows(&self, rel: &str) -> bool {
        let rel = normalize_rel(rel);
        if rel.is_empty() || first_segment(&rel) == ".versailles" {
            return false;
        }
        self.files.iter().any(|f| *f == rel)
            || self
                .dirs
                .iter()
                .any(|d| rel.len() > d.len() && rel.starts_with(d.as_str()) && rel.as_bytes()[d.len()] == b'/')
    }
}

fn first_segment(rel: &str) -> &str {
    rel.split('/').next().unwrap_or("")
}

/// Lowercase, `/`-separated, no leading/trailing slashes. Windows paths are
/// case-insensitive, so this is also the cache key.
fn normalize_rel(rel: &str) -> String {
    rel.replace('\\', "/")
        .trim_matches('/')
        .to_lowercase()
}

/// Validate an already-decoded request path before it touches the filesystem.
/// Rejects drive letters / ADS (`:`), UNC and rooted paths, `.`/`..` segments.
/// Returns the relative `PathBuf` to join onto the widgets root.
fn sanitize_request_path(decoded: &str) -> AppResult<PathBuf> {
    let trimmed = decoded.trim_start_matches('/');
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("favicon.ico") {
        return Err(AppError::msg("Not a widget file"));
    }
    if trimmed.contains(':') || trimmed.contains('\0') || trimmed.starts_with('\\') {
        return Err(AppError::msg("Forbidden path"));
    }
    let rel = PathBuf::from(trimmed.replace('/', "\\"));
    if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(AppError::msg("Forbidden path"));
    }
    Ok(rel)
}

/// Canonical widgets root, resolved at most once per distinct root path.
/// The root does not change at runtime; if it ever does, it is re-canonicalized.
fn canonical_root() -> AppResult<PathBuf> {
    static ROOT_CACHE: OnceLock<Mutex<Option<(PathBuf, PathBuf)>>> = OnceLock::new();
    let root = widgets_root()?;
    let cache = ROOT_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap();
        if let Some((cached_root, cached_canon)) = guard.as_ref() {
            if *cached_root == root {
                return Ok(cached_canon.clone());
            }
        }
    }
    let canon = root.canonicalize().map_err(|e| AppError::msg(e.to_string()))?;
    *cache.lock().unwrap() = Some((root, canon.clone()));
    Ok(canon)
}

/// Upper bound for the canonical-path cache; cleared wholesale past this.
const FILE_CACHE_MAX: usize = 256;

/// Cache of canonical file paths keyed by the normalized relative path so
/// repeated requests for the same asset skip the per-request `canonicalize()`.
/// The scope / `.versailles` checks still run on the cached value.
fn canonical_file(full: &Path, key: &str) -> AppResult<PathBuf> {
    static FILE_CACHE: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    let cache = FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(canon) = cache.lock().unwrap().get(key) {
        return Ok(canon.clone());
    }
    let canonical = full
        .canonicalize()
        .map_err(|e| AppError::msg(format!("Cannot resolve {key}: {e}")))?;
    let mut guard = cache.lock().unwrap();
    if guard.len() >= FILE_CACHE_MAX {
        guard.clear();
    }
    guard.insert(key.to_string(), canonical.clone());
    Ok(canonical)
}

/// Long-lived assets get a short shared cache; HTML always revalidates.
pub(crate) fn cache_control_for_mime(mime: &str) -> &'static str {
    match mime {
        "text/html; charset=utf-8" => "no-store, no-cache, must-revalidate",
        "text/css; charset=utf-8"
        | "text/javascript; charset=utf-8"
        | "image/png"
        | "image/jpeg"
        | "image/svg+xml"
        | "font/woff2" => "public, max-age=60",
        _ => "no-cache",
    }
}

/// Serve one file for `GET /files/{*path}`. `decoded` is the path as axum
/// extracted it (already percent-decoded once).
pub fn read_widget_file(decoded: &str, scope: &FileScope) -> AppResult<(Vec<u8>, &'static str)> {
    let root = widgets_root()?;
    let root_canon = canonical_root()?;
    let canonical = resolve_in_root(&root, &root_canon, decoded, scope)?;
    let bytes = fs::read(&canonical)?;
    Ok((bytes, mime_for(&canonical)))
}

fn resolve_in_root(
    root: &Path,
    root_canon: &Path,
    decoded: &str,
    scope: &FileScope,
) -> AppResult<PathBuf> {
    let rel = sanitize_request_path(decoded)?;
    let key = normalize_rel(&rel.to_string_lossy());
    // Scope check before any filesystem call; repeated on the canonical path
    // below so junctions/symlinks cannot leave the allowed folders.
    if !scope.allows(&key) {
        return Err(AppError::msg("Path outside served folders"));
    }

    let full = root.join(&rel);
    let canonical = canonical_file(&full, &key)?;
    let Ok(canon_rel) = canonical.strip_prefix(root_canon) else {
        return Err(AppError::msg("Path escapes widgets root"));
    };
    if !scope.allows(&canon_rel.to_string_lossy()) {
        return Err(AppError::msg("Path outside served folders"));
    }
    if !canonical.is_file() {
        return Err(AppError::msg("Not a file"));
    }
    Ok(canonical)
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "woff2" => "font/woff2",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// Reliable widget URL for WebView2: plain localhost HTTP from Versailles's API.
pub fn widget_http_url(entry_path: &Path, port: u16) -> AppResult<url::Url> {
    let root = widgets_root()?;
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    let entry_canon = entry_path
        .canonicalize()
        .map_err(|e| AppError::msg(e.to_string()))?;
    let rel = entry_canon
        .strip_prefix(&root_canon)
        .map_err(|_| AppError::msg("Widget is outside Documents\\Widgets"))?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    let mut url = url::Url::parse(&format!("http://127.0.0.1:{port}/files/"))
        .map_err(|e| AppError::msg(e.to_string()))?;
    // Push segments so names with `%`, `#`, `?` or spaces are encoded exactly once.
    url.path_segments_mut()
        .map_err(|_| AppError::msg("cannot build widget URL"))?
        .pop_if_empty()
        .extend(rel.split('/'));
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("versailles-files-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(dir.join("desktop").join("fonts")).unwrap();
            fs::create_dir_all(dir.join(".sdk")).unwrap();
            fs::create_dir_all(dir.join(".versailles")).unwrap();
            fs::create_dir_all(dir.join("app").join(".git")).unwrap();
            fs::create_dir_all(dir.join("archive").join("legacy-widgets").join("dice")).unwrap();
            fs::write(dir.join("desktop").join("index.html"), "<html></html>").unwrap();
            fs::write(dir.join("desktop").join("café é.css"), "body{}").unwrap();
            fs::write(dir.join("desktop").join("a+b.js"), "1").unwrap();
            fs::write(dir.join("desktop").join("%aé"), "pct").unwrap();
            fs::write(dir.join(".sdk").join("versailles.js"), "sdk").unwrap();
            fs::write(dir.join(".versailles").join("config.json"), "{}").unwrap();
            fs::write(dir.join("app").join(".git").join("config"), "x").unwrap();
            fs::write(dir.join("shortcuts.json"), "{}").unwrap();
            fs::write(dir.join("archive").join("legacy-widgets").join("dice").join("index.html"), "d").unwrap();
            Self(dir)
        }

        fn scope(&self) -> FileScope {
            FileScope::new(
                "desktop/index.html",
                &[self.0.join("archive").join("legacy-widgets").join("dice")],
                &self.0,
            )
        }

        fn resolve(&self, decoded: &str) -> AppResult<PathBuf> {
            let canon = self.0.canonicalize().unwrap();
            resolve_in_root(&self.0, &canon, decoded, &self.scope())
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn serves_desktop_sdk_and_registered_widgets() {
        let t = TempRoot::new();
        assert!(t.resolve("desktop/index.html").is_ok());
        assert!(t.resolve("/desktop/index.html").is_ok());
        assert!(t.resolve("DESKTOP/Index.HTML").is_ok());
        assert!(t.resolve(".sdk/versailles.js").is_ok());
        assert!(t.resolve("archive/legacy-widgets/dice/index.html").is_ok());
        // Non-ASCII and `+` are literal file-name characters (no form decoding).
        assert!(t.resolve("desktop/café é.css").is_ok());
        assert!(t.resolve("desktop/a+b.js").is_ok());
        assert!(t.resolve("desktop/a b.js").is_err());
    }

    #[test]
    fn hostile_percent_inputs_never_panic_or_double_decode() {
        let t = TempRoot::new();
        // `/files/%25a%C3%A9` reaches us as `%aé` (decoded once by axum).
        assert!(t.resolve("desktop/%aé").is_ok());
        for raw in [
            "%25a%C3%A9",
            "%aé",
            "desktop/%25a%C3%A9",
            "desktop/%4",
            "desktop/%",
            "desktop/%%",
            "desktop/%%%",
            "desktop/é%",
            "desktop/%é",
            "desktop/%C3",
            "desktop/%2e%2e/app/.git/config",
            "desktop/\u{1F600}%",
        ] {
            assert!(t.resolve(raw).is_err(), "{raw} should 404");
        }
    }

    #[test]
    fn rejects_everything_outside_scope() {
        let t = TempRoot::new();
        for raw in [
            "",
            "favicon.ico",
            "shortcuts.json",
            "app/.git/config",
            "archive/legacy-widgets/../../shortcuts.json",
            "desktop/../shortcuts.json",
            "desktop/../app/.git/config",
            ".versailles/config.json",
            ".VERSAILLES/config.json",
            "desktop/..\\.versailles\\config.json",
            "\\\\server\\share\\desktop\\index.html",
            "//server/share/desktop/index.html",
            "C:/Windows/win.ini",
            "desktop/index.html::$DATA",
            "desktop/index.html\0",
            "desktop",
            "archive/legacy-widgets",
        ] {
            assert!(t.resolve(raw).is_err(), "{raw:?} should 404");
        }
    }

    #[test]
    fn scope_never_widens_to_root() {
        let root = PathBuf::from(r"C:\W");
        let scope = FileScope::new("index.html", &[root.clone(), root.join(".versailles")], &root);
        assert!(scope.allows("index.html"));
        assert!(scope.allows("desktop/x.css"));
        assert!(!scope.allows("shortcuts.json"));
        assert!(!scope.allows(".versailles/config.json"));
        assert!(!scope.allows("desktopx/a"));
    }

    #[test]
    fn widget_url_encodes_segments_once() {
        // Same segment push widget_http_url uses (it needs the real widgets root).
        let mut url = url::Url::parse("http://127.0.0.1:1/files/").unwrap();
        url.path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .extend("desktop/%aé #?.css".split('/'));
        assert_eq!(url.path(), "/files/desktop/%25a%C3%A9%20%23%3F.css");
    }
}
