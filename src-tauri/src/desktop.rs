//! Desktop shell layers (bottom → top):
//!   0 wallpaper / 1 surface  — HTML page (`Documents\\Widgets\\desktop\\index.html`)
//!   2 apps                   — native windows cover the page
//!   3 overlay                — action bar (Alt+Space)

use crate::error::AppResult;
use crate::protocol::widget_http_url;
use crate::state::AppState;
use crate::window_manager::{
    close_desktop_window, ensure_desktop_window, set_launcher_seed, show_launcher,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLayout {
    pub page_url: Option<String>,
}

pub fn page_url(state: &AppState) -> Option<String> {
    let (port, page) = {
        let config = state.config.lock().unwrap();
        (config.api_port, config.desktop.page.clone())
    };
    let root = crate::registry::widgets_root().ok()?;
    let path = root.join(page);
    widget_http_url(&path, port).ok().map(|u| u.to_string())
}

/// Load the user HTML page in the always-on-bottom desktop window.
pub fn reveal_desktop_window(app: &AppHandle) -> AppResult<()> {
    let url = page_url(&app.state::<AppState>());
    ensure_desktop_window(app, url)?;
    emit_layout(app);
    Ok(())
}

fn layout_from(state: &AppState) -> DesktopLayout {
    DesktopLayout {
        page_url: page_url(state),
    }
}

fn persist_desktop_enabled(state: &State<'_, AppState>, enabled: bool) -> AppResult<()> {
    let mut config = state.config.lock().unwrap();
    if config.desktop.enabled == enabled {
        return Ok(());
    }
    config.desktop.enabled = enabled;
    state.store.lock().unwrap().save(&config)
}

fn emit_layout(app: &AppHandle) {
    let layout = layout_from(&app.state::<AppState>());
    let _ = app.emit("desktop://layout", layout);
}

#[tauri::command]
pub fn get_desktop_layout(state: State<'_, AppState>) -> AppResult<DesktopLayout> {
    Ok(layout_from(&state))
}

#[tauri::command]
pub fn toggle_desktop_surface(app: AppHandle, state: State<'_, AppState>) -> AppResult<bool> {
    let enable = app.get_webview_window("desktop").is_none();
    persist_desktop_enabled(&state, enable)?;
    let app2 = app.clone();
    std::thread::spawn(move || {
        let handle = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            if enable {
                let _ = reveal_desktop_window(&handle);
            } else {
                let _ = close_desktop_window(&handle);
            }
            emit_layout(&handle);
        });
    });
    Ok(enable)
}

#[tauri::command]
pub fn open_desktop_surface(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    persist_desktop_enabled(&state, true)?;
    let app2 = app.clone();
    std::thread::spawn(move || {
        let handle = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            let _ = reveal_desktop_window(&handle);
            emit_layout(&handle);
        });
    });
    Ok(())
}

#[tauri::command]
pub fn close_desktop_surface(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let app2 = app.clone();
    std::thread::spawn(move || {
        let handle = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            let _ = close_desktop_window(&handle);
            emit_layout(&handle);
        });
    });
    persist_desktop_enabled(&state, false)
}

#[tauri::command]
pub fn show_launcher_seeded(app: AppHandle, seed: String) -> AppResult<()> {
    set_launcher_seed(seed);
    show_launcher(&app)
}

#[tauri::command]
pub fn shell_show_desktop() -> AppResult<()> {
    crate::cli::show_desktop().map_err(crate::error::AppError::msg)
}
