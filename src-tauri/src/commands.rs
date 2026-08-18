use crate::error::AppResult;
use crate::media::{media_next, media_play_pause, media_previous, MediaInfo};
use crate::state::AppState;
use crate::window_manager::{hide_launcher, show_launcher};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

pub fn apply_autostart(app: &AppHandle, enabled: bool) -> AppResult<()> {
    let autostart = app.autolaunch();
    if enabled {
        autostart
            .enable()
            .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    } else {
        let _ = autostart.disable();
    }
    Ok(())
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppResult<crate::config::AppConfig> {
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
pub fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: crate::config::AppConfig,
) -> AppResult<crate::config::AppConfig> {
    let previous = state.config.lock().unwrap().clone();
    state.store.lock().unwrap().save(&config)?;
    *state.config.lock().unwrap() = config.clone();
    let _ = crate::hotkeys::register_launcher_hotkey(&app, &config.launcher_hotkey);
    if previous.autostart != config.autostart {
        let _ = apply_autostart(&app, config.autostart);
    }
    Ok(config)
}

#[tauri::command]
pub fn toggle_launcher(app: AppHandle) -> AppResult<()> {
    let visible = app
        .get_webview_window("launcher")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if visible {
        hide_launcher(&app)
    } else {
        show_launcher(&app)
    }
}

#[tauri::command]
pub fn dismiss_launcher(app: AppHandle) -> AppResult<()> {
    // Keep any background PTY alive — Alt+Space / continue reattaches.
    hide_launcher(&app)
}

#[tauri::command]
pub async fn media_now(state: State<'_, AppState>) -> AppResult<MediaInfo> {
    Ok(state.media.snapshot())
}

#[tauri::command]
pub async fn media_play_pause_cmd() -> AppResult<()> {
    media_play_pause().await
}

#[tauri::command]
pub async fn media_next_cmd() -> AppResult<()> {
    media_next().await
}

#[tauri::command]
pub async fn media_previous_cmd() -> AppResult<()> {
    media_previous().await
}

#[tauri::command]
pub fn get_runtime_status(state: State<'_, AppState>) -> AppResult<RuntimeStatus> {
    Ok(RuntimeStatus {
        media_error: state.media.last_error(),
        registry_errors: state.registry.lock().unwrap().snapshot().errors,
    })
}

#[tauri::command]
pub fn open_log_folder(app: AppHandle) -> AppResult<String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    }
    Ok(dir.display().to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub media_error: Option<String>,
    pub registry_errors: Vec<String>,
}
