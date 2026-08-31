use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::media::{media_next, media_play_pause, media_previous};
use crate::state::AppState;
use crate::window_manager::{
    apply_layout_template, close_widget_window, collect_monitor_rects, move_widget,
    open_widget_window, show_launcher, OpenWidgetState,
};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::mpsc;
use tauri::{AppHandle, Manager};
use tower_http::cors::CorsLayer;

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

        let router = router.layer(CorsLayer::permissive()).with_state(state);

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

/// Window / WebView2 APIs must run on the Tauri main thread. Calling them from
/// the Axum worker deadlocks the process on Windows.
fn on_main_thread<T, F>(app: &AppHandle, f: F) -> Result<T, StatusCode>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|err| {
        tracing::error!("run_on_main_thread schedule failed: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    rx.recv().map_err(|_| {
        tracing::error!("run_on_main_thread result channel closed");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

async fn serve_files(Path(path): Path<String>) -> Response {
    match crate::protocol::read_widget_file(&path) {
        Ok((bytes, mime)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, crate::protocol::cache_control_for_mime(mime)),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
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
    let expected = format!("Bearer {token}");
    if header == expected {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
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
    })?;
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
    })?;
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
    })?;
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
    })?;
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
    let media = state.app.state::<AppState>().media.snapshot();
    Ok(Json(json!(media)))
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
    on_main_thread(&state.app, move || show_launcher(&app))?
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
