//! Desktop shell layers (bottom → top):
//!   0 wallpaper / 1 surface  — opt-in HTML page (`Documents\\Widgets\\desktop\\index.html`)
//!   2 apps                   — native windows cover the page (including its HUD)
//!   3 Anywhere bar           — optional always-on-top strip; covers app title bars
//!   4 overlay                — action bar (Alt+Space)

use crate::error::AppResult;
use crate::protocol::widget_http_url;
use crate::state::AppState;
use crate::window_manager::{
    close_anywhere_window, close_desktop_window, close_widget_window, ensure_anywhere_window,
    ensure_desktop_window, set_launcher_seed, show_launcher,
};
use serde::Serialize;
use std::fs;
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
    close_page_embedded_windows(app);
    emit_layout(app);
    Ok(())
}

pub fn read_desktop_html(state: &AppState) -> String {
    let page = state.config.lock().unwrap().desktop.page.clone();
    let Ok(root) = crate::registry::widgets_root() else {
        return String::new();
    };
    fs::read_to_string(root.join(page)).unwrap_or_default()
}

pub fn html_embeds_widget(html: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    html.contains(&format!("/files/{id}/")) || html.contains(&format!("/files/legacy/{id}/"))
}

pub fn widget_is_on_desktop_page(state: &AppState, id: &str) -> bool {
    html_embeds_widget(&read_desktop_html(state), id)
}

fn close_page_embedded_windows(app: &AppHandle) {
    let state = app.state::<AppState>();
    let html = read_desktop_html(&state);
    let ids: Vec<String> = {
        let mgr = state.window_manager.lock().unwrap();
        mgr.open_widgets()
            .into_iter()
            .map(|w| w.id)
            .filter(|id| html_embeds_widget(&html, id))
            .collect()
    };
    for id in ids {
        let _ = close_widget_window(app, &state.window_manager, &id);
    }
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

fn persist_anywhere_bar(state: &State<'_, AppState>, enabled: bool) -> AppResult<()> {
    let mut config = state.config.lock().unwrap();
    if config.desktop.anywhere_bar == enabled {
        return Ok(());
    }
    config.desktop.anywhere_bar = enabled;
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
                let _ = close_anywhere_window(&handle);
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
            let _ = close_anywhere_window(&handle);
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
pub fn toggle_anywhere_bar(app: AppHandle, state: State<'_, AppState>) -> AppResult<bool> {
    let enable = app.get_webview_window("anywhere").is_none();
    let app2 = app.clone();
    std::thread::spawn(move || {
        let handle = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            if enable {
                let _ = ensure_anywhere_window(&handle);
            } else {
                let _ = close_anywhere_window(&handle);
            }
        });
    });
    persist_anywhere_bar(&state, enable)?;
    Ok(enable)
}

#[tauri::command]
pub fn open_anywhere_bar(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let app2 = app.clone();
    std::thread::spawn(move || {
        let handle = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            let _ = ensure_anywhere_window(&handle);
        });
    });
    persist_anywhere_bar(&state, true)
}

#[tauri::command]
pub fn close_anywhere_bar(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let app2 = app.clone();
    std::thread::spawn(move || {
        let handle = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            let _ = close_anywhere_window(&handle);
        });
    });
    persist_anywhere_bar(&state, false)
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
