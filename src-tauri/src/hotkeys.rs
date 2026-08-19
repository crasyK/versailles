use crate::error::AppResult;
use crate::media::get_mouse_position;
use crate::window_manager::{hide_launcher, hotkey_overlay_visible, show_launcher};
use tauri::AppHandle;
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
            if hotkey_overlay_visible(&app_handle) {
                let _ = hide_launcher(&app_handle);
            } else {
                let _ = show_launcher(&app_handle);
            }
        })
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?;

    Ok(())
}

/// Prefer the monitor under the cursor when placing the overlay.
pub fn launcher_anchor() -> Option<(i32, i32)> {
    get_mouse_position().ok()
}
