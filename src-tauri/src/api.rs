use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::media::{media_next, media_play_pause, media_previous};
use crate::state::AppState;
use crate::window_manager::{
    apply_layout_template, close_widget_window, collect_monitor_rects, move_widget,
    open_widget_window, show_launcher, OpenWidgetState,
};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tauri::{AppHandle, Manager};
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Clone)]
struct ApiState {
    app: AppHandle,
    token: String,
}

/// Bind the localhost listener (with port fallback) and spawn the HTTP server.
/// `/files` + `/health` always run so widgets can load even when control API is disabled.
/// Returns the port that was successfully bound.
pub fn start_api_server(app: AppHandle, config: &mut AppConfig) -> AppResult<u16> {
    let preferred = std::env::var("VERSAILLES_API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(config.api_port);
    let (std_listener, port) = bind_with_fallback(preferred)?;
    if port != preferred {
        tracing::warn!("API port {preferred} busy; using {port}");
    }
    config.api_bound_port = Some(port);

    let token = config.api_token.clone();
    let control_enabled = config.api_enabled;
    let bench = crate::boot::is_bench();
    tauri::async_runtime::spawn(async move {
        let state = ApiState { app, token };

        // Always-on routes for widget HTML/assets.
        let mut router = Router::new()
            .route(
                "/health",
                get(|| async { Json(json!({ "ok": true, "name": "versailles" })) }),
            )
            .route("/files/{*path}", get(serve_files))
            .route("/weather", get(weather))
            .route("/weather/text", get(weather_text))
            .route("/quote", get(quote));

        if bench {
            router = router
                .route("/debug/boot", get(debug_boot))
                .route("/debug/boot/nav", post(debug_boot_nav))
                .route("/debug/boot/loaded", post(debug_boot_loaded));
        }

        if control_enabled {
            router = router
                .route("/widgets", get(list_widgets))
                .route("/widgets/{id}/open", post(open_widget))
                .route("/widgets/{id}/close", post(close_widget_api))
                .route("/widgets/{id}/move", post(move_widget_api))
                .route("/layouts/{name}/apply", post(apply_layout))
                .route("/launcher/show", post(show_launcher_api))
                .route("/media/now", get(media_now))
                .route("/media/play-pause", post(media_toggle))
                .route("/media/next", post(media_skip_next))
                .route("/media/previous", post(media_skip_previous));
        }

        // Gate is the outermost layer: Host / Origin / Sec-Fetch checks run
        // before CORS answers a preflight or any handler runs.
        let router = router
            .layer(cors_layer(port))
            .layer(middleware::from_fn_with_state(port, request_gate))
            .with_state(state);

        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(err) => {
                tracing::error!("Failed to convert API listener: {err}");
                return;
            }
        };

        tracing::info!(
            "Versailles file server on http://127.0.0.1:{port} (control API {})",
            if control_enabled { "enabled" } else { "disabled" }
        );
        if let Err(err) = axum::serve(listener, router).await {
            tracing::error!("API server stopped: {err}");
        }
    });

    crate::boot::mark_api_bound();
    Ok(port)
}

fn bind_with_fallback(preferred: u16) -> AppResult<(std::net::TcpListener, u16)> {
    let start = if preferred == 0 { 47831 } else { preferred };
    for offset in 0u16..11 {
        let port = start.saturating_add(offset);
        match std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))) {
            Ok(listener) => {
                listener
                    .set_nonblocking(true)
                    .map_err(|e| AppError::msg(e.to_string()))?;
                return Ok((listener, port));
            }
            Err(err) => {
                tracing::debug!("bind {port} failed: {err}");
            }
        }
    }
    Err(AppError::msg(format!(
        "Could not bind Versailles API near port {start}"
    )))
}

/// Origins that may call the localhost server from a browser context: the file
/// server itself, the Tauri app origin (wallpaper shell, backdrop, guides) and,
/// in debug builds, the Vite dev server.
fn allowed_origins(port: u16) -> Vec<String> {
    let mut list = vec![
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        "http://tauri.localhost".to_string(),
        "https://tauri.localhost".to_string(),
        "tauri://localhost".to_string(),
    ];
    if cfg!(debug_assertions) {
        list.push("http://localhost:1420".to_string());
        list.push("http://127.0.0.1:1420".to_string());
    }
    list
}

/// Pure request gate (DNS rebinding + cross-site requests). Unit-tested below.
///
/// - `Host` must be `127.0.0.1:<port>` or `localhost:<port>`.
/// - A present `Origin` must be allowlisted.
/// - Without `Origin`, a `Sec-Fetch-Site: cross-site|same-site` request is refused unless
///   it is a navigation to `/files/*` (the wallpaper shell iframes the desktop
///   page cross-site; an attacker `<img>` / no-cors fetch is not a navigation).
pub(crate) fn request_allowed(
    port: u16,
    path: &str,
    host: Option<&str>,
    origin: Option<&str>,
    sec_fetch_site: Option<&str>,
    sec_fetch_mode: Option<&str>,
) -> bool {
    let Some(host) = host.map(str::trim) else {
        return false;
    };
    let host_ok = host.eq_ignore_ascii_case(&format!("127.0.0.1:{port}"))
        || host.eq_ignore_ascii_case(&format!("localhost:{port}"));
    if !host_ok {
        return false;
    }
    if let Some(origin) = origin {
        let origin = origin.trim();
        return allowed_origins(port)
            .iter()
            .any(|o| o.eq_ignore_ascii_case(origin));
    }
    // `same-site` counts as foreign too: another loopback port is the same "site".
    let foreign = sec_fetch_site
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("cross-site") || v.eq_ignore_ascii_case("same-site")
        })
        .unwrap_or(false);
    if !foreign {
        return true;
    }
    let navigate = sec_fetch_mode
        .map(|v| v.trim().eq_ignore_ascii_case("navigate"))
        .unwrap_or(false);
    navigate && path.starts_with("/files/")
}

async fn request_gate(State(port): State<u16>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    // A non-ASCII Origin is present-but-unknown: map it to a value that never matches.
    let origin = headers
        .get(header::ORIGIN)
        .map(|v| v.to_str().unwrap_or("invalid"));
    let allowed = request_allowed(
        port,
        req.uri().path(),
        text("host"),
        origin,
        text("sec-fetch-site"),
        text("sec-fetch-mode"),
    );
    if !allowed {
        tracing::debug!(
            "refused {} {} (host={:?} origin={:?})",
            req.method(),
            req.uri().path(),
            text("host"),
            origin
        );
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    next.run(req).await
}

/// Reflect only allowlisted origins (the gate already refused the rest).
fn cors_layer(port: u16) -> CorsLayer {
    let origins: Vec<HeaderValue> = allowed_origins(port)
        .into_iter()
        .filter_map(|o| HeaderValue::from_str(&o).ok())
        .collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
}

/// Window / WebView2 APIs must run on the Tauri main thread. Calling them from
/// the Axum worker deadlocks the process on Windows. Awaits a oneshot so the
/// tokio worker is never parked on a blocking `recv()`.
async fn on_main_thread<T, F>(app: &AppHandle, f: F) -> Result<T, StatusCode>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|err| {
        tracing::error!("run_on_main_thread schedule failed: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    rx.await.map_err(|_| {
        tracing::error!("run_on_main_thread result channel closed");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// Folders `/files` may serve right now: desktop page + `.sdk` + registered folder widgets.
fn current_file_scope(app: &AppHandle) -> crate::protocol::FileScope {
    let state = app.state::<AppState>();
    let page = state.config.lock().unwrap().desktop.page.clone();
    let dirs: Vec<std::path::PathBuf> = state
        .registry
        .lock()
        .unwrap()
        .list()
        .iter()
        .map(|w| w.path.clone())
        .collect();
    let root = crate::registry::widgets_root().unwrap_or_default();
    crate::protocol::FileScope::new(&page, &dirs, &root)
}

async fn serve_files(State(state): State<ApiState>, Path(path): Path<String>) -> Response {
    let scope = current_file_scope(&state.app);
    // `path` is already percent-decoded by axum; never decode it again.
    match crate::protocol::read_widget_file(&path, &scope) {
        Ok((bytes, mime)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, crate::protocol::cache_control_for_mime(mime)),
            ],
            bytes,
        )
            .into_response(),
        // No SDK on the desktop: serve the one this host ships.
        Err(_) if path == crate::sdk::FILES_PATH => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            crate::sdk::BUNDLED,
        )
            .into_response(),
        Err(err) => {
            tracing::debug!("/files/{path}: {err}");
            (StatusCode::NOT_FOUND, "not found").into_response()
        }
    }
}

#[derive(Deserialize)]
struct WeatherQuery {
    location: Option<String>,
    save: Option<String>,
}

fn weather_save_flag(raw: Option<&str>) -> bool {
    matches!(raw, Some("1") | Some("true") | Some("yes"))
}

async fn lookup_weather(q: WeatherQuery) -> Result<crate::weather::WeatherPayload, String> {
    crate::weather::lookup(crate::weather::WeatherQuery {
        location: q.location,
        save: weather_save_flag(q.save.as_deref()),
    })
    .await
}

async fn weather(Query(q): Query<WeatherQuery>) -> Response {
    match lookup_weather(q).await {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::weather::error_json(err)),
        )
            .into_response(),
    }
}

async fn quote() -> Response {
    match crate::quote::lookup().await {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::quote::error_json(err)),
        )
            .into_response(),
    }
}

async fn weather_text(Query(q): Query<WeatherQuery>) -> Response {
    match lookup_weather(q).await {
        Ok(payload) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            crate::weather::plain_text(&payload),
        )
            .into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err).into_response(),
    }
}

fn authorize(headers: &HeaderMap, token: &str) -> Result<(), StatusCode> {
    let header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if bearer_matches(header, token) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// `Authorization: Bearer <token>` check. An empty configured token never
/// matches (a hand-edited `"apiToken": ""` must not open the API), and the
/// comparison takes the same time wherever the first difference is.
pub(crate) fn bearer_matches(header: &str, token: &str) -> bool {
    if token.trim().is_empty() {
        return false;
    }
    let Some(given) = header.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_eq(given.as_bytes(), token.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn list_widgets(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    let app_state = state.app.state::<AppState>();
    let registered = {
        let registry = app_state.registry.lock().unwrap();
        registry.list().to_vec()
    };
    let open = app_state.window_manager.lock().unwrap().open_widgets();
    Ok(Json(json!({
        "registered": registered,
        "open": open,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveBody {
    x: i32,
    y: i32,
    #[serde(default)]
    disable_snap: bool,
}

async fn open_widget(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    let app = state.app.clone();
    let result = on_main_thread(&state.app, move || {
        let app_state = app.state::<AppState>();
        open_widget_window(&app, &app_state.window_manager, &id, None, None)
    }).await?;
    match result {
        Ok(opened) => {
            let _ = persist_session(&state.app);
            Ok(Json(json!(opened)))
        }
        Err(err) => Ok(Json(json!({ "error": err.to_string() }))),
    }
}

async fn close_widget_api(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    let app = state.app.clone();
    let id_resp = id.clone();
    let result = on_main_thread(&state.app, move || {
        let app_state = app.state::<AppState>();
        close_widget_window(&app, &app_state.window_manager, &id)
    }).await?;
    match result {
        Ok(()) => {
            let _ = persist_session(&state.app);
            Ok(Json(json!({ "ok": true, "id": id_resp })))
        }
        Err(err) => Ok(Json(json!({ "error": err.to_string() }))),
    }
}

async fn move_widget_api(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    let app = state.app.clone();
    let result = on_main_thread(&state.app, move || {
        let app_state = app.state::<AppState>();
        let threshold = app_state.config.lock().unwrap().snap_threshold;
        let monitors = collect_monitor_rects(&app);
        move_widget(
            &app,
            &app_state.window_manager,
            &id,
            body.x,
            body.y,
            body.disable_snap,
            threshold,
            &monitors,
        )
    }).await?;
    match result {
        Ok(result) => {
            let _ = persist_session(&state.app);
            Ok(Json(json!(result)))
        }
        Err(err) => Ok(Json(json!({ "error": err.to_string() }))),
    }
}

async fn apply_layout(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    let app = state.app.clone();
    let result = on_main_thread(&state.app, move || -> AppResult<Vec<OpenWidgetState>> {
        let app_state = app.state::<AppState>();
        let layout = app_state.store.lock().unwrap().load_layout(&name)?;
        apply_layout_template(&app, &app_state.window_manager, &layout)
    }).await?;
    match result {
        Ok(opened) => {
            let _ = persist_session(&state.app);
            Ok(Json(json!(opened)))
        }
        Err(err) => {
            if err.to_string().contains("not found") || err.to_string().contains("No such") {
                Err(StatusCode::NOT_FOUND)
            } else {
                Ok(Json(json!({ "error": err.to_string() })))
            }
        }
    }
}

async fn media_now(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    let media = state.app.state::<AppState>().media.clone();
    // No page may be keeping the snapshot fresh (see `media::POLL_UNSEEN`).
    crate::media::refresh_if_stale(&state.app, &media).await;
    Ok(Json(json!(media.snapshot())))
}

async fn media_toggle(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    match media_play_pause().await {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(err) => Ok(Json(json!({ "ok": false, "error": err.to_string() }))),
    }
}

async fn media_skip_next(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    match media_next().await {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(err) => Ok(Json(json!({ "ok": false, "error": err.to_string() }))),
    }
}

async fn media_skip_previous(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    match media_previous().await {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(err) => Ok(Json(json!({ "ok": false, "error": err.to_string() }))),
    }
}

async fn show_launcher_api(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&headers, &state.token)?;
    let app = state.app.clone();
    on_main_thread(&state.app, move || show_launcher(&app)).await?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

fn persist_session(app: &AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    // Snapshot before locking: persistable_session re-locks config internally.
    let session = crate::window_manager::persistable_session(app);
    let mut config = state.config.lock().unwrap();
    config.session_widgets = session;
    let result = state.store.lock().unwrap().save_runtime_from_app(&config);
    result
}

async fn debug_boot() -> Json<crate::boot::BootSnapshot> {
    Json(crate::boot::snapshot())
}

async fn debug_boot_nav() -> StatusCode {
    crate::boot::count_iframe_nav();
    StatusCode::NO_CONTENT
}

async fn debug_boot_loaded() -> StatusCode {
    crate::boot::mark_wallpaper_loaded();
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT: u16 = 47831;

    fn ok(path: &str, host: &str, origin: Option<&str>, site: Option<&str>, mode: Option<&str>) -> bool {
        request_allowed(PORT, path, Some(host), origin, site, mode)
    }

    #[test]
    fn bearer_token_must_match_exactly_and_never_be_empty() {
        let token = "0f8c2a4e-5b1d-4f7a-9c3e-2d6b8a1f4e70";
        assert!(bearer_matches(&format!("Bearer {token}"), token));
        assert!(!bearer_matches(token, token));
        assert!(!bearer_matches(&format!("bearer {token}"), token));
        assert!(!bearer_matches(&format!("Bearer {token}x"), token));
        assert!(!bearer_matches(&format!("Bearer {}", &token[..35]), token));
        assert!(!bearer_matches("Bearer ", token));
        assert!(!bearer_matches("", token));
        // A blank configured token locks the API instead of opening it.
        assert!(!bearer_matches("Bearer ", ""));
        assert!(!bearer_matches("Bearer  ", " "));
        assert!(!bearer_matches("Bearer", ""));
    }

    #[test]
    fn host_header_must_be_loopback_with_bound_port() {
        assert!(ok("/health", "127.0.0.1:47831", None, None, None));
        assert!(ok("/health", "localhost:47831", None, None, None));
        assert!(ok("/health", "LOCALHOST:47831", None, None, None));
        // DNS rebinding: attacker hostname resolving to 127.0.0.1.
        assert!(!ok("/health", "evil.example:47831", None, None, None));
        assert!(!ok("/health", "127.0.0.1", None, None, None));
        assert!(!ok("/health", "127.0.0.1:47832", None, None, None));
        assert!(!ok("/health", "127.0.0.1:47831.evil.example", None, None, None));
        assert!(!request_allowed(PORT, "/health", None, None, None, None));
    }

    #[test]
    fn origin_must_be_allowlisted() {
        let host = "127.0.0.1:47831";
        for origin in [
            "http://127.0.0.1:47831",
            "http://localhost:47831",
            "http://tauri.localhost",
            "https://tauri.localhost",
            "tauri://localhost",
        ] {
            assert!(ok("/weather", host, Some(origin), Some("cross-site"), Some("cors")), "{origin}");
        }
        for origin in [
            "https://evil.example",
            "null",
            "http://127.0.0.1:47832",
            "http://tauri.localhost.evil.example",
            "invalid",
        ] {
            assert!(!ok("/weather", host, Some(origin), None, None), "{origin}");
        }
        assert_eq!(
            ok("/weather", host, Some("http://localhost:1420"), None, None),
            cfg!(debug_assertions)
        );
    }

    #[test]
    fn cross_site_without_origin_only_for_file_navigation() {
        let host = "127.0.0.1:47831";
        // Wallpaper shell (tauri origin) iframes the desktop page: cross-site navigation.
        assert!(ok("/files/desktop/index.html", host, None, Some("cross-site"), Some("navigate")));
        // Attacker <img> / fetch(no-cors) / navigation to side-effect routes.
        assert!(!ok("/files/desktop/index.html", host, None, Some("cross-site"), Some("no-cors")));
        assert!(!ok("/weather", host, None, Some("cross-site"), Some("navigate")));
        assert!(!ok("/quote", host, None, Some("cross-site"), None));
        assert!(!ok("/files/desktop/index.html", host, None, Some("same-site"), Some("no-cors")));
        // Same-origin subresources, direct navigation, curl / node (no Sec-Fetch).
        assert!(ok("/files/.sdk/versailles.js", host, None, Some("same-origin"), Some("no-cors")));
        assert!(ok("/files/desktop/index.html", host, None, Some("none"), Some("navigate")));
        assert!(ok("/weather", host, None, None, None));
    }

    /// Real axum stack (gate + CORS + wildcard route): the path reaches the
    /// handler decoded exactly once, and hostile encodings never panic.
    #[tokio::test]
    async fn router_decodes_once_and_gates_requests() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = Router::new()
            .route("/files/{*path}", get(|Path(p): Path<String>| async move { p }))
            .layer(cors_layer(port))
            .layer(middleware::from_fn_with_state(port, request_gate));
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{port}");

        let echo = |path: &'static str| {
            let client = client.clone();
            let url = format!("{base}{path}");
            async move {
                let res = client.get(url).send().await.unwrap();
                (res.status().as_u16(), res.text().await.unwrap_or_default())
            }
        };
        assert_eq!(echo("/files/%25a%C3%A9").await, (200, "%aé".to_string()));
        assert_eq!(echo("/files/desktop/a+b%20c.js").await, (200, "desktop/a+b c.js".to_string()));
        // Invalid UTF-8 / truncated escapes: rejected or passed literally, never a panic.
        for path in ["/files/%C3", "/files/%4", "/files/%%", "/files/%E9t%C3%A9"] {
            let (status, _) = echo(path).await;
            assert!(status == 200 || status == 400, "{path}: {status}");
        }

        let rebind = client
            .get(format!("{base}/files/x"))
            .header("host", format!("evil.example:{port}"))
            .send()
            .await
            .unwrap();
        assert_eq!(rebind.status().as_u16(), 403);
        let foreign = client
            .get(format!("{base}/files/x"))
            .header("origin", "https://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(foreign.status().as_u16(), 403);
        let tauri = client
            .get(format!("{base}/files/x"))
            .header("origin", "http://tauri.localhost")
            .send()
            .await
            .unwrap();
        assert_eq!(tauri.status().as_u16(), 200);
        assert_eq!(
            tauri.headers().get("access-control-allow-origin").and_then(|v| v.to_str().ok()),
            Some("http://tauri.localhost")
        );
        // Server is still alive after all of the above.
        assert_eq!(echo("/files/ok").await.0, 200);
    }
}
