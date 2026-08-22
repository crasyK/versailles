use crate::error::AppResult;
use crate::media::get_mouse_position;
use crate::window_manager::toggle_hotkey_piece;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Register a global shortcut for every spawnable with the `hotkey` hook.
///
/// Never clears existing shortcuts until a parse yields at least one binding.
/// Mid-save / empty reads of `desktop/index.html` used to call `unregister_all`
/// first and leave Alt+Space dead for the rest of the session.
pub fn register_page_hotkeys(app: &AppHandle) -> AppResult<()> {
    let bindings = {
        let state = app.state::<crate::state::AppState>();
        let cat = crate::desktop::page_catalog(&state);
        let fallback = state.config.lock().unwrap().launcher_hotkey.clone();
        cat.hotkey_bindings(&fallback)
    };

    if bindings.is_empty() {
        tracing::warn!(
            "no spawnable declared data-hooks=hotkey; keeping existing global hotkeys"
        );
        return Ok(());
    }

    let _ = app.global_shortcut().unregister_all();

    let mut registered = 0usize;
    for (accel, id) in bindings {
        let shortcut: Shortcut = match accel.parse() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("invalid data-hotkey '{accel}' on '{id}': {e}");
                continue;
            }
        };
        let app_handle = app.clone();
        let piece_id = id.clone();
        match app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            toggle_hotkey_piece(&app_handle, &piece_id);
        }) {
            Ok(()) => {
                registered += 1;
                tracing::info!("hotkey {accel} → {id}");
            }
            Err(e) => tracing::warn!("failed to register '{accel}' for '{id}': {e}"),
        }
    }

    if registered == 0 {
        tracing::warn!("hotkey parse produced bindings but none registered");
    }
    Ok(())
}

/// Prefer the monitor under the cursor when placing the overlay.
pub fn launcher_anchor() -> Option<(i32, i32)> {
    get_mouse_position().ok()
}
