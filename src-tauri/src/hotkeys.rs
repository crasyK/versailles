use crate::error::AppResult;
use crate::media::get_mouse_position;
use crate::window_manager::{hide_launcher, show_launcher};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn register_launcher_hotkey(app: &AppHandle, accelerator: &str) -> AppResult<()> {
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|e| crate::error::AppError::msg(format!("Invalid hotkey '{accelerator}': {e}")))?;

    // Clear previous registration if any.
    let _ = app.global_shortcut().unregister_all();

    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let visible = app_handle
                .get_webview_window("launcher")
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false);

            if visible {
                let _ = hide_launcher(&app_handle);
            } else {
                let _ = show_launcher(&app_handle);
            }
        })
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?;

    Ok(())
}

/// Prefer the monitor under the cursor when placing the launcher.
pub fn launcher_anchor() -> Option<(i32, i32)> {
    get_mouse_position().ok()
}
