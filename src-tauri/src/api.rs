//! Localhost runtime for widget HTML, weather, quotes, and now-playing.
//! Always on. No Bearer control surface.

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::media::{media_next, media_play_pause, media_previous};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use tauri::{AppHandle, Manager};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct RuntimeState {
    app: AppHandle,
}

/// Bind the localhost listener (with port fallback) and spawn the file server.
/// `/files` + `/health` always run so widgets can load.
/// Returns the port that was successfully bound.
pub fn start_runtime_server(app: AppHandle, config: &mut AppConfig) -> AppResult<u16> {
    let preferred = config.api_port;
    let (std_listener, port) = bind_with_fallback(preferred)?;
    if port != preferred {
        tracing::warn!("runtime port {preferred} busy; using {port}");
        config.api_port = port;
    }

    tauri::async_runtime::spawn(async move {
        let state = RuntimeState { app };

        let router = Router::new()
            .route(
                "/health",
                get(|| async { Json(json!({ "ok": true, "name": "versailles" })) }),
            )
            .route("/files/{*path}", get(serve_files))
            .route("/weather", get(weather))
            .route("/weather/text", get(weather_text))
            .route("/quote", get(quote))
            .route("/media/now", get(media_now))
            .route("/media/play-pause", post(media_toggle))
            .route("/media/next", post(media_skip_next))
            .route("/media/previous", post(media_skip_previous))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(err) => {
                tracing::error!("Failed to convert runtime listener: {err}");
                return;
            }
        };

        tracing::info!("Versailles localhost runtime on http://127.0.0.1:{port}");
        if let Err(err) = axum::serve(listener, router).await {
            tracing::error!("localhost runtime stopped: {err}");
        }
    });

    Ok(port)
}

fn bind_with_fallback(preferred: u16) -> AppResult<(std::net::TcpListener, u16)> {
    let start = if preferred == 0 { 47841 } else { preferred };
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
        "Could not bind Versailles localhost runtime near port {start}"
    )))
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

async fn media_now(State(state): State<RuntimeState>) -> Json<serde_json::Value> {
    let media = state.app.state::<AppState>().media.snapshot();
    Json(json!(media))
}

async fn media_toggle() -> Json<serde_json::Value> {
    match media_play_pause().await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(err) => Json(json!({ "ok": false, "error": err.to_string() })),
    }
}

async fn media_skip_next() -> Json<serde_json::Value> {
    match media_next().await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(err) => Json(json!({ "ok": false, "error": err.to_string() })),
    }
}

async fn media_skip_previous() -> Json<serde_json::Value> {
    match media_previous().await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(err) => Json(json!({ "ok": false, "error": err.to_string() })),
    }
}
