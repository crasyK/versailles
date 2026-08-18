use crate::error::{AppError, AppResult};
use crate::registry::widgets_root;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::http::{header::CONTENT_TYPE, Request, Response, StatusCode};

pub fn serve_widget_request(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match resolve_and_read(request.uri().path()) {
        Ok((bytes, mime)) => Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, mime)
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", cache_control_for_mime(mime))
            .body(bytes)
            .unwrap_or_else(|_| not_found()),
        Err(err) => {
            tracing::warn!("widget protocol: {err}");
            not_found()
        }
    }
}

pub fn read_widget_file(rel_path: &str) -> AppResult<(Vec<u8>, &'static str)> {
    resolve_and_read(rel_path)
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/plain")
        .body(b"not found".to_vec())
        .unwrap()
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

/// Cache of canonical file paths keyed by `root.join(decoded)` so repeated
/// requests for the same asset skip the per-request `canonicalize()` syscall.
/// The `starts_with(root)` / `.deck` security checks still run on the cached value.
fn canonical_file(full: &Path, decoded: &str) -> AppResult<PathBuf> {
    static FILE_CACHE: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();
    let cache = FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(canon) = cache.lock().unwrap().get(full) {
        return Ok(canon.clone());
    }
    let canonical = full
        .canonicalize()
        .map_err(|e| AppError::msg(format!("Cannot resolve {decoded}: {e}")))?;
    cache.lock().unwrap().insert(full.to_path_buf(), canonical.clone());
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

fn resolve_and_read(uri_path: &str) -> AppResult<(Vec<u8>, &'static str)> {
    let trimmed = uri_path
        .trim_start_matches('/')
        .trim_start_matches("files/");
    // Ignore query/favicon noise
    let trimmed = trimmed.split('?').next().unwrap_or(trimmed);
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("favicon.ico") {
        return Err(AppError::msg("Not a widget file"));
    }
    let decoded = urlencoding_decode(trimmed);

    // Never expose Deck private state (contains API bearer token).
    let first = decoded.split(['/', '\\']).next().unwrap_or("");
    if first.eq_ignore_ascii_case(".deck") {
        return Err(AppError::msg("Forbidden path"));
    }

    let root = widgets_root()?;
    let full = root.join(&decoded);

    let canonical = canonical_file(&full, &decoded)?;
    let root_canon = canonical_root()?;
    if !canonical.starts_with(&root_canon) {
        return Err(AppError::msg("Path escapes widgets root"));
    }
    // Defense in depth: block anything that resolves into .deck.
    if canonical
        .strip_prefix(&root_canon)
        .ok()
        .and_then(|rel| rel.components().next())
        .map(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".deck"))
        .unwrap_or(false)
    {
        return Err(AppError::msg("Forbidden path"));
    }
    if !canonical.is_file() {
        return Err(AppError::msg("Not a file"));
    }

    let bytes = fs::read(&canonical)?;
    Ok((bytes, mime_for(&canonical)))
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

fn urlencoding_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = &input[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v as char);
                    i += 3;
                    continue;
                }
                out.push('%');
                i += 1;
            }
            b'+' => {
                out.push(' ');
                i += 1;
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

/// Reliable widget URL for WebView2: plain localhost HTTP from Deck's API.
pub fn widget_http_url(entry_path: &PathBuf, port: u16) -> AppResult<url::Url> {
    let root = widgets_root()?;
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    let entry_canon = entry_path
        .canonicalize()
        .map_err(|e| AppError::msg(e.to_string()))?;
    let rel = entry_canon
        .strip_prefix(&root_canon)
        .map_err(|_| AppError::msg("Widget is outside Documents\\Widgets"))?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    let raw = format!("http://127.0.0.1:{port}/files/{rel}");
    url::Url::parse(&raw).map_err(|e| AppError::msg(e.to_string()))
}
