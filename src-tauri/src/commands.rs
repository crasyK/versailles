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
use crate::page::HookClass;
use serde::Serialize;
use tauri::{AppHandle, Manager, State, Webview};
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

/// Gate a hook on the *calling window*, not on a self-declared argument.
/// Spawn windows act as their piece; the wallpaper must name a piece via
/// `caller`; host windows get nothing. Returns the piece id the call acts as.
pub(crate) fn require_hook(
    state: &AppState,
    webview: &Webview,
    caller: Option<&str>,
    hook: &str,
    class: HookClass,
) -> AppResult<String> {
    let cat = crate::desktop::page_catalog(state);
    let invoker = crate::page::classify_invoker(webview.label(), &cat, |id| {
        state.registry.lock().unwrap().get(id).map(|w| {
            w.manifest
                .permissions
                .iter()
                .map(|p| p.hook_name().to_string())
                .collect()
        })
    });
    crate::page::authorize_hook(&cat, &invoker, caller, hook, class).map_err(|err| {
        tracing::warn!("{} ({}): {err}", webview.label(), hook);
        err
    })
}

/// Run `f` on the main thread from an async command and await its result.
/// Sync commands run on the UI thread; building a WebView2 window inside that
/// stack deadlocks on Windows, so window work always hops through here.
async fn on_main_thread<T, F>(app: &AppHandle, f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle) -> T + Send + 'static,
{
    let app2 = app.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&app2));
    })
    .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    rx.await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))
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
    webview: Webview,
    state: State<'_, AppState>,
    id: String,
    data: Option<serde_json::Value>,
    caller: Option<String>,
) -> AppResult<bool> {
    require_hook(&state, &webview, caller.as_deref(), "spawn", HookClass::Plain)?;
    // `spawn(id, data)`: hand the spawnable structured data. With data, an
    // open piece stays open and just receives it.
    if let Some(data) = data.as_ref() {
        let script = crate::bus::spawn_data_script(data)?;
        let target = crate::desktop::page_catalog(&state)
            .spawnable(&id)
            .map(|p| p.id.clone())
            .ok_or_else(|| crate::page::unknown_spawn(&id))?;
        let label = crate::window_manager::widget_label(&target);
        if let Some(window) = app.get_webview_window(&label) {
            if window.is_visible().unwrap_or(false) {
                let _ = window.eval(&script);
                return Ok(true);
            }
        }
        crate::window_manager::queue_init_script(&target, script.clone());
        let opened = toggle_slideout_on_main(&app, &id).await;
        crate::window_manager::clear_init_scripts(&target);
        let opened = opened?;
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.eval(&script);
        }
        persist_session_on_main(&app).await?;
        return Ok(opened);
    }
    let opened = toggle_slideout_on_main(&app, &id).await?;
    persist_session_on_main(&app).await?;
    Ok(opened)
}

async fn toggle_slideout_on_main(app: &AppHandle, id: &str) -> AppResult<bool> {
    // Never create WebView2 directly from the IPC/async stack — that hangs
    // Windows (same pattern as toggle_hotkey_piece / show_launcher).
    let app2 = app.clone();
    let id2 = id.to_string();
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

    rx.await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

async fn persist_session_on_main(app: &AppHandle) -> AppResult<()> {
    let app3 = app.clone();
    let (tx2, rx2) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx2.send(persist_session(&app3));
    })
    .map_err(|e| crate::error::AppError::msg(e.to_string()))?;
    rx2.await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

#[tauri::command]
pub fn list_open_widgets(state: State<'_, AppState>) -> AppResult<Vec<OpenWidgetState>> {
    Ok(state.window_manager.lock().unwrap().open_widgets())
}

#[tauri::command]
pub async fn move_widget_cmd(
    app: AppHandle,
    webview: Webview,
    state: State<'_, AppState>,
    id: String,
    x: i32,
    y: i32,
    disable_snap: Option<bool>,
    caller: Option<String>,
) -> AppResult<SnapResult> {
    let acting = require_hook(&state, &webview, caller.as_deref(), "layout", HookClass::Plain)?;
    // `layout` moves the piece's own window, never another one.
    if !crate::page::same_piece(&crate::desktop::page_catalog(&state), &acting, &id) {
        return Err(crate::error::AppError::msg(format!(
            "layout: '{acting}' may only move its own window"
        )));
    }
    // Guides window creation must not run inside the IPC stack (WebView2 deadlock).
    on_main_thread(&app, move |app| -> AppResult<SnapResult> {
        let state = app.state::<AppState>();
        let threshold = state.config.lock().unwrap().snap_threshold;
        let monitors = collect_monitor_rects(app);
        let result = move_widget(
            app,
            &state.window_manager,
            &id,
            x,
            y,
            disable_snap.unwrap_or(false),
            threshold,
            &monitors,
        )?;
        let _ = set_guides_visible(app, !result.guides.is_empty(), &result.guides);
        persist_session(app)?;
        Ok(result)
    })
    .await?
}

/// `storage` hook: the caller's own key-value store.
#[tauri::command]
pub fn storage_op(
    webview: Webview,
    state: State<'_, AppState>,
    op: crate::storage::StorageOp,
    caller: Option<String>,
) -> AppResult<serde_json::Value> {
    let acting = require_hook(&state, &webview, caller.as_deref(), "storage", HookClass::Plain)?;
    let dir = state.store.lock().unwrap().storage_dir();
    crate::storage::apply(&dir, &acting, op)
}

/// `clipboard` hook: plain text on the system clipboard (`null` when none).
#[tauri::command]
pub async fn clipboard_read(
    webview: Webview,
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<Option<String>> {
    require_hook(&state, &webview, caller.as_deref(), "clipboard", HookClass::Plain)?;
    tokio::task::spawn_blocking(crate::clipboard::read_text)
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

#[tauri::command]
pub async fn clipboard_write(
    webview: Webview,
    state: State<'_, AppState>,
    text: String,
    caller: Option<String>,
) -> AppResult<()> {
    require_hook(&state, &webview, caller.as_deref(), "clipboard", HookClass::Plain)?;
    tokio::task::spawn_blocking(move || crate::clipboard::write_text(&text))
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

/// `power` hook: lock / sleep / sign out / restart / shut down.
#[tauri::command]
pub async fn power_action(
    webview: Webview,
    state: State<'_, AppState>,
    action: crate::power::PowerAction,
    caller: Option<String>,
) -> AppResult<()> {
    let acting = require_hook(&state, &webview, caller.as_deref(), "power", HookClass::Plain)?;
    tracing::info!("power: '{acting}' requested {action:?}");
    tokio::task::spawn_blocking(move || crate::power::run(action))
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

/// Read-only system hooks (`battery`, `network`, `focus`, `audio`, `notify`):
/// the current value. Changes / new toasts are pushed (see
/// `sensors::start_watcher`).
#[tauri::command]
pub async fn sense_now(
    webview: Webview,
    state: State<'_, AppState>,
    hook: String,
    caller: Option<String>,
) -> AppResult<serde_json::Value> {
    let hook = hook.trim().to_ascii_lowercase();
    if !crate::sensors::is_readable(&hook) {
        return Err(crate::error::AppError::msg(format!("'{hook}' is not a sensor hook")));
    }
    require_hook(&state, &webview, caller.as_deref(), &hook, HookClass::Plain)?;
    tokio::task::spawn_blocking(move || crate::sensors::read(&hook))
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

/// Quick settings (`radios`, `audio-control`, `display`, `power-mode`):
/// change one. Control class: only a spawn window of a piece that declares
/// the action's hook. Answers the hook's new state.
#[tauri::command]
pub async fn quick_set(
    webview: Webview,
    state: State<'_, AppState>,
    action: crate::quick::QuickAction,
    caller: Option<String>,
) -> AppResult<serde_json::Value> {
    let hook = action.hook();
    let acting = require_hook(&state, &webview, caller.as_deref(), hook, HookClass::Control)?;
    tracing::info!("quick settings: '{acting}' requested {}", action.describe());
    tokio::task::spawn_blocking(move || {
        crate::quick::apply(&action)?;
        crate::quick::read(hook)
    })
    .await
    .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

/// `windows` hook: the app windows a taskbar would show (top of the z-order
/// first). `icons` adds a 32×32 PNG data URL per window. Changes are pushed
/// (see `app_windows::start_watcher`).
#[tauri::command]
pub async fn windows_list(
    webview: Webview,
    state: State<'_, AppState>,
    icons: Option<bool>,
    caller: Option<String>,
) -> AppResult<Vec<crate::app_windows::WindowInfo>> {
    require_hook(&state, &webview, caller.as_deref(), "windows", HookClass::Plain)?;
    let icons = icons.unwrap_or(false);
    tokio::task::spawn_blocking(move || crate::app_windows::list(icons))
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

/// `windows` hook: one window's icon as a PNG data URL (`null` if it has none).
#[tauri::command]
pub async fn windows_icon(
    webview: Webview,
    state: State<'_, AppState>,
    id: i64,
    caller: Option<String>,
) -> AppResult<Option<String>> {
    require_hook(&state, &webview, caller.as_deref(), "windows", HookClass::Plain)?;
    tokio::task::spawn_blocking(move || crate::app_windows::icon(id))
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

/// `windows-control` hook: focus / minimize / restore / maximize / close /
/// move another app's window. Only windows `windows_list` would show; never
/// one of Versailles' own.
#[tauri::command]
pub async fn windows_control(
    webview: Webview,
    state: State<'_, AppState>,
    id: i64,
    action: crate::app_windows::Action,
    to: Option<crate::app_windows::MoveTo>,
    caller: Option<String>,
) -> AppResult<()> {
    let acting = require_hook(&state, &webview, caller.as_deref(), "windows-control", HookClass::Plain)?;
    tracing::debug!("windows: '{acting}' {action:?} {id}");
    tokio::task::spawn_blocking(move || crate::app_windows::control(id, action, to))
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
}

/// Every monitor (logical sizes, display numbers, primary flag) and the one
/// the calling window is on. Read-only; no hook needed.
#[tauri::command]
pub fn monitor_info(app: AppHandle, webview: Webview) -> AppResult<crate::monitors::MonitorReport> {
    let monitors = crate::monitors::list(&app);
    Ok(crate::monitors::MonitorReport {
        current: crate::monitors::of_window(&app, webview.label()),
        monitors: monitors.iter().map(crate::monitors::page_json).collect(),
    })
}

/// `bus` hook: relay a message to every open piece that declared `bus`.
/// Returns how many windows it reached.
#[tauri::command]
pub fn bus_publish(
    app: AppHandle,
    webview: Webview,
    state: State<'_, AppState>,
    topic: String,
    data: Option<serde_json::Value>,
    caller: Option<String>,
) -> AppResult<usize> {
    let acting = require_hook(&state, &webview, caller.as_deref(), "bus", HookClass::Plain)?;
    let cat = crate::desktop::page_catalog(&state);
    crate::bus::publish(&app, &cat, &acting, &topic, &data.unwrap_or(serde_json::Value::Null))
}

/// `layout` hook: resize, re-anchor, pin on top, or set click-through regions
/// on the caller's own window. See `LayoutPatch`.
#[tauri::command]
pub async fn layout_update(
    app: AppHandle,
    webview: Webview,
    state: State<'_, AppState>,
    patch: crate::window_manager::LayoutPatch,
    caller: Option<String>,
) -> AppResult<()> {
    let acting = require_hook(&state, &webview, caller.as_deref(), "layout", HookClass::Plain)?;
    // A bar's monitor copy changes its own window, not the primary one.
    let own = crate::page::label_piece_id(webview.label())
        .is_some_and(|p| crate::page::same_piece(&crate::desktop::page_catalog(&state), &acting, p));
    let label = if own {
        webview.label().to_string()
    } else {
        crate::window_manager::widget_label(&acting)
    };
    on_main_thread(&app, move |app| {
        crate::window_manager::apply_layout_patch(app, &acting, &label, patch)
    })
    .await?
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
    webview: Webview,
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<[i32; 2]> {
    require_hook(&state, &webview, caller.as_deref(), "mouse", HookClass::Plain)?;
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
pub async fn apply_layout(app: AppHandle, name: String) -> AppResult<Vec<OpenWidgetState>> {
    // Opens widget windows: hop to the main thread instead of the IPC stack.
    on_main_thread(&app, move |app| -> AppResult<Vec<OpenWidgetState>> {
        let state = app.state::<AppState>();
        let layout = state.store.lock().unwrap().load_layout(&name)?;
        let opened = apply_layout_template(app, &state.window_manager, &layout)?;
        // Snapshot before locking: persistable_session re-locks config internally.
        let session = crate::window_manager::persistable_session(app);
        let mut config = state.config.lock().unwrap();
        config.active_layout = Some(name);
        config.session_widgets = session;
        state.store.lock().unwrap().save_runtime_from_app(&config)?;
        Ok(opened)
    })
    .await?
}

#[tauri::command]
pub fn load_layout(state: State<'_, AppState>, name: String) -> AppResult<LayoutTemplate> {
    state.store.lock().unwrap().load_layout(&name)
}

#[tauri::command]
pub async fn media_now(
    webview: Webview,
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<MediaInfo> {
    crate::boot::count_media_now();
    require_hook(&state, &webview, caller.as_deref(), "media", HookClass::Plain)?;
    Ok(state.media.snapshot())
}

#[tauri::command]
pub async fn media_play_pause_cmd(
    webview: Webview,
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<()> {
    require_hook(&state, &webview, caller.as_deref(), "media", HookClass::Plain)?;
    media_play_pause().await
}

#[tauri::command]
pub async fn media_next_cmd(
    webview: Webview,
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<()> {
    require_hook(&state, &webview, caller.as_deref(), "media", HookClass::Plain)?;
    media_next().await
}

#[tauri::command]
pub async fn media_previous_cmd(
    webview: Webview,
    state: State<'_, AppState>,
    caller: Option<String>,
) -> AppResult<()> {
    require_hook(&state, &webview, caller.as_deref(), "media", HookClass::Plain)?;
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
pub async fn clear_guides(app: AppHandle) -> AppResult<()> {
    // May create the guides window — never inside the sync IPC stack.
    on_main_thread(&app, |app| set_guides_visible(app, false, &[])).await?
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
pub async fn focus_spawnable(app: AppHandle, id: String) -> AppResult<()> {
    // reveal_overlay may build the spawn window: hop off the IPC stack.
    on_main_thread(&app, move |app| -> AppResult<()> {
        let state = app.state::<AppState>();
        if crate::window_manager::hotkey_overlay_visible(app) {
            let label = crate::window_manager::widget_label(&id);
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.set_focus();
            }
            return Ok(());
        }
        crate::window_manager::reveal_overlay(app, &state.window_manager, &id)
    })
    .await?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnableEngineContext {
    pub id: String,
    pub dismiss_on_blur: bool,
    pub opts: crate::config::SpawnableEngineOpts,
    /// Versailles version.
    pub host_version: &'static str,
    /// Bundled SDK version and what the start-up check found on the desktop.
    pub sdk: crate::sdk::SdkStatus,
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
        host_version: env!("CARGO_PKG_VERSION"),
        sdk: crate::sdk::status().unwrap_or_else(|| crate::sdk::SdkStatus {
            bundled: crate::sdk::bundled_version().to_string(),
            desktop: crate::sdk::Decision::Missing,
            updated: false,
        }),
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
