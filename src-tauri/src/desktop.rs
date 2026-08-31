//! Desktop shell layers (bottom → top):
//!   0 wallpaper / 1 surface  — opt-in HTML page (`Documents\\Widgets\\desktop\\index.html`)
//!   2 apps                   — native windows cover the page (including its HUD)
//!   3 overlay                — hotkey spawnable (Alt+Space)

use crate::error::AppResult;
use crate::page::PageCatalog;
use crate::protocol::widget_http_url;
use crate::state::AppState;
use crate::window_manager::{
    close_desktop_window, close_widget_window, ensure_desktop_window, set_launcher_seed,
    show_launcher,
};
use serde::Serialize;
use std::fs;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLayout {
    pub page_url: Option<String>,
}

pub fn page_path(state: &AppState) -> Option<std::path::PathBuf> {
    let page = state.config.lock().unwrap().desktop.page.clone();
    let root = crate::registry::widgets_root().ok()?;
    Some(root.join(page))
}

pub fn surface_url(state: &AppState, mode: &str, id: Option<&str>) -> Option<String> {
    let port = {
        let config = state.config.lock().unwrap();
        config.api_bound_port.unwrap_or(config.api_port)
    };
    let path = page_path(state)?;
    let mut url = widget_http_url(&path, port).ok()?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("m", mode);
        if let Some(id) = id.filter(|s| !s.is_empty()) {
            pairs.append_pair("id", id);
        }
    }
    Some(url.to_string())
}

pub fn page_url(state: &AppState) -> Option<String> {
    surface_url(state, "desktop", None)
}

pub fn refresh_page_cache(state: &AppState) {
    let page = state.config.lock().unwrap().desktop.page.clone();
    let html = load_page_file(&page);
    let cat = crate::page::parse_page(&html);
    *state.page_html.lock().unwrap() = html;
    *state.page_catalog.lock().unwrap() = cat;
}

fn load_page_file(page_rel: &str) -> String {
    crate::boot::count_html_read();
    let Ok(root) = crate::registry::widgets_root() else {
        return String::new();
    };
    fs::read_to_string(root.join(page_rel)).unwrap_or_default()
}

pub fn page_catalog(state: &AppState) -> PageCatalog {
    {
        let html = state.page_html.lock().unwrap();
        let cat = state.page_catalog.lock().unwrap();
        if !html.is_empty() || !cat.pieces.is_empty() {
            return cat.clone();
        }
    }
    refresh_page_cache(state);
    state.page_catalog.lock().unwrap().clone()
}

pub fn desktop_tray_label(visible: bool) -> &'static str {
    if visible {
        "Hide desktop page"
    } else {
        "Show desktop page"
    }
}

pub fn set_desktop_tray_label(app: &AppHandle, visible: bool) {
    let state = app.state::<AppState>();
    let guard = state.tray_desktop_item.lock().unwrap();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_text(desktop_tray_label(visible));
    }
}

/// Load the user HTML page in the always-on-bottom desktop window.
pub fn reveal_desktop_window(app: &AppHandle) -> AppResult<()> {
    let url = page_url(&app.state::<AppState>());
    ensure_desktop_window(app, url)?;
    close_page_embedded_windows(app);
    emit_layout(app);
    set_desktop_tray_label(app, true);
    Ok(())
}

pub fn hide_desktop_window(app: &AppHandle) -> AppResult<()> {
    close_desktop_window(app)?;
    set_desktop_tray_label(app, false);
    Ok(())
}

pub fn read_desktop_html(state: &AppState) -> String {
    {
        let cached = state.page_html.lock().unwrap();
        if !cached.is_empty() {
            return cached.clone();
        }
    }
    refresh_page_cache(state);
    state.page_html.lock().unwrap().clone()
}

fn close_page_embedded_windows(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cat = page_catalog(&state);
    let ids: Vec<String> = {
        let mgr = state.window_manager.lock().unwrap();
        mgr.open_widgets()
            .into_iter()
            .map(|w| w.id)
            .filter(|id| cat.is_desktop_widget(id))
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
    state.store.lock().unwrap().save_runtime_from_app(&config)
}

fn emit_layout(app: &AppHandle) {
    let layout = layout_from(&app.state::<AppState>());
    let _ = app.emit("desktop://layout", layout);
    crate::boot::mark_layout_emitted();
}

#[tauri::command]
pub fn get_desktop_layout(state: State<'_, AppState>) -> AppResult<DesktopLayout> {
    Ok(layout_from(&state))
}

#[tauri::command]
pub fn get_desktop_html(state: State<'_, AppState>) -> AppResult<String> {
    Ok(read_desktop_html(&state))
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
                let _ = hide_desktop_window(&handle);
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
            let _ = hide_desktop_window(&handle);
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
