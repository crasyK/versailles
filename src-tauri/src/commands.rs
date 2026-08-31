use crate::config::{LayoutTemplate, Position};
use crate::error::AppResult;
use crate::layout::{Rect, SnapResult};
use crate::media::{
    get_mouse_position, media_next, media_play_pause, media_previous, MediaInfo,
};
use crate::registry::RegistrySnapshot;
use crate::state::AppState;
use crate::window_manager::{
    apply_layout_template, build_layout_template, close_widget_window,
    collect_monitor_rects, hide_launcher, move_widget, open_widget_window,
    popup_widget_context_menu, set_always_on_top, set_guides_visible, show_launcher,
    toggle_slideout_widget, OpenWidgetState,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

fn persist_session(app: &AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    // Snapshot before locking: persistable_session re-locks config internally.
    let session = crate::window_manager::persistable_session(app);
    let mut config = state.config.lock().unwrap();
    config.session_widgets = session;
    let result = state.store.lock().unwrap().save_runtime_from_app(&config);
    result
}

fn require_hook(state: &State<'_, AppState>, caller: &Option<String>, hook: &str) -> AppResult<()> {
    let cat = crate::desktop::page_catalog(state);
    crate::page::enforce_caller_hook_catalog(&cat, caller.as_deref(), hook)
}

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
pub fn list_widgets(state: State<'_, AppState>) -> AppResult<RegistrySnapshot> {
    // In-memory snapshot only. The registry is scanned once at startup and
    // kept fresh by the file watcher, which emits "registry://changed".
    let mut snap = state.registry.lock().unwrap().snapshot();
    let cat = crate::desktop::page_catalog(&state);
    for widget in &mut snap.widgets {
        widget.embedded = cat.is_desktop_widget(&widget.manifest.id);
    }
    Ok(snap)
}

#[tauri::command]
pub async fn open_widget(
    app: AppHandle,
    id: String,
    position: Option<Position>,
    always_on_top: Option<bool>,
) -> AppResult<OpenWidgetState> {
    // Sync commands run on the UI thread. WebView2 window creation inside that
    // stack deadlocks (build waits for the event loop; the loop waits for us).
    // Hop from the async runtime onto the main thread instead.
    let app2 = app.clone();
    let id2 = id.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let state = app2.state::<AppState>();
        let result = open_widget_window(
            &app2,
            &state.window_manager,
            &id2,
            position,
            always_on_top,
        );
        let _ = tx.send(result);
    })
    .map_err(|e| crate::error::AppError::msg(e.to_string()))?;

    let opened = rx
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))??;

    let app3 = app.clone();
    let (tx2, rx2) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx2.send(persist_session(&app3));
    })
    .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    rx2.await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))??;

    Ok(opened)
}

#[tauri::command]
pub async fn close_widget(app: AppHandle, id: String) -> AppResult<()> {
    let app2 = app.clone();
    let id2 = id.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = {
            let state = app2.state::<AppState>();
            close_widget_window(&app2, &state.window_manager, &id2)
        };
        let _ = tx.send(result);
    })
    .map_err(|e| crate::error::AppError::msg(e.to_string()))?;

    rx.await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))??;

    let app3 = app.clone();
    let (tx2, rx2) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx2.send(persist_session(&app3));
    })
    .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    rx2.await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))??;

    Ok(())
}

#[tauri::command]
pub async fn toggle_slideout(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    caller: Option<String>,
) -> AppResult<bool> {
    require_hook(&state, &caller, "spawn")?;
    // Never create WebView2 directly from the IPC/async stack — that hangs
    // Windows (same pattern as toggle_hotkey_piece / show_launcher).
    let app2 = app.clone();
    let id2 = id.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let handle = app2.clone();
        let id3 = id2.clone();
        let _ = app2.run_on_main_thread(move || {
            let state = handle.state::<AppState>();
            let result = toggle_slideout_widget(
                &handle,
                &state.window_manager,
                &id3,
            );
            let _ = tx.send(result);
        });
    });

    let opened = rx
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))??;

    let app3 = app.clone();
    let (tx2, rx2) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx2.send(persist_session(&app3));
    })
    .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    rx2.await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))??;

    Ok(opened)
}

#[tauri::command]
pub fn list_open_widgets(state: State<'_, AppState>) -> AppResult<Vec<OpenWidgetState>> {
    Ok(state.window_manager.lock().unwrap().open_widgets())
}

#[tauri::command]
pub fn move_widget_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    x: i32,
    y: i32,
    disable_snap: Option<bool>,
    caller: Option<String>,
) -> AppResult<SnapResult> {
    require_hook(&state, &caller, "layout")?;
    let threshold = state.config.lock().unwrap().snap_threshold;
    let monitors = collect_monitor_rects(&app);
    let result = move_widget(
        &app,
        &state.window_manager,
        &id,
        x,
        y,
        disable_snap.unwrap_or(false),
        threshold,
        &monitors,
    )?;
    let _ = set_guides_visible(&app, !result.guides.is_empty(), &result.guides);
    persist_session(&app)?;
    Ok(result)
}

#[tauri::command]
pub fn set_widget_always_on_top(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    value: bool,
) -> AppResult<()> {
    set_always_on_top(&app, &state.window_manager, &id, value)?;
    persist_session(&app)?;
    Ok(())
}

#[tauri::command]
pub fn set_widget_opacity(app: AppHandle, id: String, opacity: f64) -> AppResult<()> {
    let label = crate::window_manager::widget_label(&id);
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| crate::error::AppError::msg("Widget window missing"))?;
    let opacity = opacity.clamp(0.05, 1.0);
    let script = format!("document.documentElement.style.opacity = '{opacity}';");
    window
        .eval(&script)
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn get_mouse_position_cmd(
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<[i32; 2]> {
    require_hook(&state, &caller, "mouse")?;
    let (x, y) = get_mouse_position()?;
    Ok([x, y])
}

#[tauri::command]
pub fn get_monitors(app: AppHandle) -> AppResult<Vec<Rect>> {
    Ok(collect_monitor_rects(&app))
}

#[tauri::command]
pub fn toggle_launcher(app: AppHandle) -> AppResult<()> {
    if crate::window_manager::hotkey_overlay_visible(&app) {
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
pub fn list_layouts(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    state.store.lock().unwrap().list_layouts()
}

#[tauri::command]
pub fn save_layout(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> AppResult<LayoutTemplate> {
    let manager = state.window_manager.lock().unwrap();
    let layout = build_layout_template(&app, &manager, &name);
    drop(manager);
    state.store.lock().unwrap().save_layout(&layout)?;
    let mut config = state.config.lock().unwrap();
    config.active_layout = Some(name);
    state.store.lock().unwrap().save_runtime_from_app(&config)?;
    Ok(layout)
}

#[tauri::command]
pub fn apply_layout(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> AppResult<Vec<OpenWidgetState>> {
    let layout = state.store.lock().unwrap().load_layout(&name)?;
    let opened = apply_layout_template(&app, &state.window_manager, &layout)?;
    // Snapshot before locking: persistable_session re-locks config internally.
    let session = crate::window_manager::persistable_session(&app);
    let mut config = state.config.lock().unwrap();
    config.active_layout = Some(name);
    config.session_widgets = session;
    state.store.lock().unwrap().save_runtime_from_app(&config)?;
    Ok(opened)
}

#[tauri::command]
pub fn load_layout(state: State<'_, AppState>, name: String) -> AppResult<LayoutTemplate> {
    state.store.lock().unwrap().load_layout(&name)
}

#[tauri::command]
pub async fn media_now(
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<MediaInfo> {
    crate::boot::count_media_now();
    require_hook(&state, &caller, "media")?;
    Ok(state.media.snapshot())
}

#[tauri::command]
pub async fn media_play_pause_cmd(
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<()> {
    require_hook(&state, &caller, "media")?;
    media_play_pause().await
}

#[tauri::command]
pub async fn media_next_cmd(state: State<'_, AppState>, caller: Option<String>) -> AppResult<()> {
    require_hook(&state, &caller, "media")?;
    media_next().await
}

#[tauri::command]
pub async fn media_previous_cmd(
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<()> {
    require_hook(&state, &caller, "media")?;
    media_previous().await
}

#[tauri::command]
pub fn popup_widget_menu(app: AppHandle, id: String) -> AppResult<()> {
    // Return immediately; popup on a deferred main-thread tick so the
    // sync IPC handler cannot deadlock WebView2 while showing the menu.
    tracing::info!("popup_widget_menu requested for {id}");
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(16));
        let app2 = app.clone();
        let id2 = id.clone();
        if let Err(err) = app.run_on_main_thread(move || {
            match popup_widget_context_menu(&app2, &id2) {
                Ok(()) => {
                    tracing::info!("popup_widget_menu shown for {id2}");
                }
                Err(err) => {
                    tracing::warn!("popup_widget_menu failed for {id2}: {err}");
                }
            }
        }) {
            tracing::warn!("popup_widget_menu schedule failed for {id}: {err}");
        }
    });
    Ok(())
}

#[tauri::command]
pub fn clear_guides(app: AppHandle) -> AppResult<()> {
    set_guides_visible(&app, false, &[])
}

#[tauri::command]
pub fn get_api_info(state: State<'_, AppState>) -> AppResult<ApiInfo> {
    let config = state.config.lock().unwrap();
    Ok(ApiInfo {
        enabled: config.api_enabled,
        port: config.api_bound_port.unwrap_or(config.api_port),
        token: config.api_token.clone(),
        base_url: format!(
            "http://127.0.0.1:{}",
            config.api_bound_port.unwrap_or(config.api_port)
        ),
    })
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

#[tauri::command]
pub fn arm_overlay_focus_guard(ms: Option<u64>) -> AppResult<()> {
    crate::window_manager::arm_overlay_focus_guard_ms(ms.unwrap_or(500));
    Ok(())
}

#[tauri::command]
pub fn focus_spawnable(app: AppHandle, id: String) -> AppResult<()> {
    let state = app.state::<AppState>();
    if crate::window_manager::hotkey_overlay_visible(&app) {
        let label = crate::window_manager::widget_label(&id);
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.set_focus();
        }
        return Ok(());
    }
    crate::window_manager::reveal_overlay(&app, &state.window_manager, &id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnableEngineContext {
    pub id: String,
    pub dismiss_on_blur: bool,
    pub opts: crate::config::SpawnableEngineOpts,
}

#[tauri::command]
pub fn get_spawnable_engine_context(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<SpawnableEngineContext> {
    let user = state.store.lock().unwrap().load_user_config()?;
    let cat = crate::desktop::page_catalog(&state);
    let piece = cat
        .spawnable(&id)
        .ok_or_else(|| crate::error::AppError::msg(format!("unknown spawnable '{id}'")))?;
    let dismiss = crate::page::piece_dismiss_on_blur(piece, &user);
    let opts = user
        .spawnables
        .get(&id.trim().to_ascii_lowercase())
        .or_else(|| user.spawnables.get(&id))
        .and_then(|s| s.opts.clone())
        .unwrap_or_default();
    Ok(SpawnableEngineContext {
        id: piece.id.clone(),
        dismiss_on_blur: dismiss,
        opts,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiInfo {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
    pub base_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub media_error: Option<String>,
    pub registry_errors: Vec<String>,
}
