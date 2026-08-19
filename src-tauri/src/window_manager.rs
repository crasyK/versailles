use crate::config::{LayoutPlacement, LayoutTemplate, MonitorFingerprint, Position, SessionWidget};
use crate::error::{AppError, AppResult};
use crate::layout::{snap, Rect, SnapGuide, SnapResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWidgetState {
    pub id: String,
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub always_on_top: bool,
}

#[derive(Debug, Default)]
pub struct WindowManager {
    open: HashMap<String, OpenWidgetState>,
}

impl WindowManager {
    pub fn open_widgets(&self) -> Vec<OpenWidgetState> {
        let mut list: Vec<_> = self.open.values().cloned().collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    pub fn get(&self, id: &str) -> Option<&OpenWidgetState> {
        self.open.get(id)
    }

    pub fn rects_except(&self, id: &str) -> Vec<Rect> {
        self.open
            .values()
            .filter(|w| w.id != id)
            .map(|w| Rect {
                x: w.x,
                y: w.y,
                width: w.width as i32,
                height: w.height as i32,
            })
            .collect()
    }

    pub fn session_snapshot(&self) -> Vec<SessionWidget> {
        self.open
            .values()
            .map(|w| SessionWidget {
                id: w.id.clone(),
                position: Position { x: w.x, y: w.y },
                always_on_top: w.always_on_top,
            })
            .collect()
    }
}

pub const PREWARM_LABEL: &str = "prewarm";

pub fn widget_label(id: &str) -> String {
    format!("widget-{id}")
}

fn prewarm_window_slot() -> &'static Mutex<Option<WebviewWindow>> {
    static SLOT: OnceLock<Mutex<Option<WebviewWindow>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Spawn a hidden 1×1 WebView2 on a background task so the runtime is warm
/// before the first real widget window is built. Tauri v2 cannot repurpose the
/// prewarm `WebviewWindow` (label/URL/size are fixed at creation), so the slot
/// is closed after the first widget opens.
pub fn spawn_webview_prewarm(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let app_on_main = app.clone();
        if let Err(err) = app.run_on_main_thread(move || {
            if let Err(err) = ensure_prewarm_window(&app_on_main) {
                tracing::warn!("WebView2 prewarm failed: {err}");
            }
        }) {
            tracing::warn!("WebView2 prewarm schedule failed: {err}");
        }
    });
}

fn ensure_prewarm_window(app: &AppHandle) -> AppResult<()> {
    if app.get_webview_window(PREWARM_LABEL).is_some() {
        return Ok(());
    }
    if prewarm_window_slot().lock().unwrap().is_some() {
        return Ok(());
    }

    let url = WebviewUrl::External(
        url::Url::parse("about:blank").map_err(|e| AppError::msg(e.to_string()))?,
    );
    let window = WebviewWindowBuilder::new(app, PREWARM_LABEL, url)
        .title("")
        .inner_size(1.0, 1.0)
        .decorations(false)
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .resizable(false)
        .build()?;
    let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
        x: -32000,
        y: -32000,
    }));
    prewarm_window_slot().lock().unwrap().replace(window);
    Ok(())
}

fn dismiss_prewarm_window(app: &AppHandle) {
    let window = prewarm_window_slot()
        .lock()
        .unwrap()
        .take()
        .or_else(|| app.get_webview_window(PREWARM_LABEL));
    let Some(window) = window else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = app.run_on_main_thread(move || {
            let _ = window.close();
        });
    });
}

fn snap_generation() -> &'static Mutex<HashMap<String, u64>> {
    static MAP: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn snap_suppress_until() -> &'static Mutex<HashMap<String, std::time::Instant>> {
    static MAP: OnceLock<Mutex<HashMap<String, std::time::Instant>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn suppress_snap_for(id: &str, ms: u64) {
    let until = std::time::Instant::now() + Duration::from_millis(ms);
    snap_suppress_until()
        .lock()
        .unwrap()
        .insert(id.to_string(), until);
}

fn snap_is_suppressed(id: &str) -> bool {
    snap_suppress_until()
        .lock()
        .unwrap()
        .get(id)
        .map(|until| std::time::Instant::now() < *until)
        .unwrap_or(false)
}

fn close_generation() -> &'static Mutex<HashMap<String, u64>> {
    static MAP: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bump_close_generation(id: &str) -> u64 {
    let mut map = close_generation().lock().unwrap();
    let entry = map.entry(id.to_string()).or_insert(0);
    *entry += 1;
    *entry
}

fn widget_menu_handler_labels() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

fn ensure_widget_menu_handler(app: &AppHandle, window: &WebviewWindow, id: &str) {
    let label = widget_label(id);
    {
        let mut registered = widget_menu_handler_labels().lock().unwrap();
        if !registered.insert(label) {
            return;
        }
    }

    let app_handle = app.clone();
    window.on_menu_event(move |_window, event| {
        let eid = event.id().as_ref();
        if eid == "wlauncher" {
            let _ = show_launcher(&app_handle);
            return;
        }
        if let Some(rest) = eid.strip_prefix("wclose::") {
            let state = app_handle.state::<AppState>();
            let _ = close_widget_window(&app_handle, &state.window_manager, rest);
            persist_session(&app_handle);
            return;
        }
        if let Some(rest) = eid.strip_prefix("waot::") {
            let state = app_handle.state::<AppState>();
            let current = state
                .window_manager
                .lock()
                .unwrap()
                .get(rest)
                .map(|w| w.always_on_top)
                .unwrap_or(false);
            let _ = set_always_on_top(&app_handle, &state.window_manager, rest, !current);
            persist_session(&app_handle);
        }
    });
}

fn persist_session(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut config = state.config.lock().unwrap();
    config.session_widgets = state.window_manager.lock().unwrap().session_snapshot();
    let _ = state.store.lock().unwrap().save_runtime_from_app(&config);
}

fn schedule_snap_after_drag(app: AppHandle, id: String) {
    if snap_is_suppressed(&id) {
        return;
    }
    let gen = {
        let mut map = snap_generation().lock().unwrap();
        let entry = map.entry(id.clone()).or_insert(0);
        *entry += 1;
        *entry
    };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(160));
        if snap_is_suppressed(&id) {
            return;
        }
        let current = snap_generation()
            .lock()
            .unwrap()
            .get(&id)
            .copied()
            .unwrap_or(0);
        if current != gen {
            return;
        }
        let app2 = app.clone();
        let id2 = id.clone();
        let _ = app.run_on_main_thread(move || {
            if snap_is_suppressed(&id2) {
                return;
            }
            let state = app2.state::<AppState>();
            let threshold = state.config.lock().unwrap().snap_threshold;
            let monitors = collect_monitor_rects(&app2);
            let (x, y, width, height) = {
                let mgr = state.window_manager.lock().unwrap();
                let Some(w) = mgr.get(&id2) else {
                    return;
                };
                (w.x, w.y, w.width as i32, w.height as i32)
            };
            let others = state.window_manager.lock().unwrap().rects_except(&id2);
            let candidate = Rect {
                x,
                y,
                width,
                height,
            };
            let result = snap(candidate, &others, &monitors, threshold);
            if result.x != x || result.y != y {
                let _ = apply_position(&app2, &state.window_manager, &id2, result.x, result.y);
            }
            let _ = set_guides_visible(&app2, false, &[]);
            persist_session(&app2);
            let _ = app2.emit(
                "layout://changed",
                state.window_manager.lock().unwrap().open_widgets(),
            );
        });
    });
}

/// Clip the HWND to a rounded rect matching the current outer size.
/// Must be re-run after every resize — a stale region keeps the old (small) clip.
#[cfg(windows)]
fn clip_round_window(window: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};

    let Ok(hwnd_raw) = window.hwnd() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    if size.width < 2 || size.height < 2 {
        return;
    }
    let hwnd = HWND(hwnd_raw.0);
    // Diameter = 2 × CSS radius (18px card) in physical px so the clip matches the panel.
    let diameter = ((36.0 * window.scale_factor().unwrap_or(1.0)).round() as i32).clamp(20, 72);
    unsafe {
        // GDI regions exclude the right/bottom edge — pass +1 or those sides lose the CSS border.
        let region = CreateRoundRectRgn(
            0,
            0,
            size.width as i32 + 1,
            size.height as i32 + 1,
            diameter,
            diameter,
        );
        if !region.is_invalid() {
            let _ = SetWindowRgn(hwnd, Some(region), true);
        }
    }
}

#[cfg(not(windows))]
fn clip_round_window(_window: &WebviewWindow) {}

#[cfg(windows)]
fn force_borderless(window: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    };

    // Keep existing titles (widgets). Only strip chrome that fights transparency.
    let _ = window.set_decorations(false);
    let _ = window.set_shadow(false);
    // Do NOT SetWindowRgn here — widgets need full transparent corners;
    // rounded clip is launcher-only (see ensure_launcher_window / show_launcher_ready).

    let Ok(hwnd_raw) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(hwnd_raw.0);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

#[cfg(not(windows))]
fn force_borderless(_window: &WebviewWindow) {}

fn defer_window_focus(window: WebviewWindow) {
    defer_raise_widget(window, None);
}

/// Raise a widget off the calling thread. Sync show/focus on the UI/IPC thread
/// can re-enter WebView2 and freeze the whole desktop.
fn defer_raise_widget(window: WebviewWindow, position: Option<Position>) {
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        let _ = app.run_on_main_thread(move || {
            if let Some(pos) = position {
                let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
                    x: pos.x,
                    y: pos.y,
                }));
            }
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        });
    });
}

/// Keep widget chrome (id + opacity + right-click menu) alive across navigations.
fn widget_host_chrome_script(id: &str, opacity: f64, radius: u32) -> String {
    let id_js = serde_json::to_string(id).unwrap_or_else(|_| "null".into());
    format!(
        r#"(function(){{
  window.__VERSAILLES_WIDGET_ID__ = {id_js};
  window.__DECK_WIDGET_ID__ = {id_js};
  function versaillesApplyHostChrome() {{
    try {{
      document.documentElement.style.opacity = '{opacity}';
      if (!document.getElementById('versailles-host-chrome')) {{
        var s = document.createElement('style');
        s.id = 'versailles-host-chrome';
        // Don't round/clip html/body — that cuts CSS shadows into a dirty silhouette.
        // Soften the drop shadow so transparent WebView2 edges stay clean.
        s.textContent = [
          'html,body{{background:transparent!important;overflow:hidden;}}',
          '.widget{{border-radius:{radius}px!important;',
          'box-shadow:0 10px 28px rgba(0,0,0,.22),0 2px 6px rgba(0,0,0,.12)!important;}}'
        ].join('');
        (document.head || document.documentElement).appendChild(s);
      }}
      if (window.__VERSAILLES_NATIVE_CHROME__ || window.__DECK_NATIVE_CHROME__) return;
      if (window.__VERSAILLES_HOST_MENU_BOUND__) return;
      window.__VERSAILLES_HOST_MENU_BOUND__ = true;
      document.addEventListener('contextmenu', function(e) {{
        if (window.__VERSAILLES_NATIVE_CHROME__ || window.__DECK_NATIVE_CHROME__) return;
        e.preventDefault();
        var wid = window.__VERSAILLES_WIDGET_ID__ || window.__DECK_WIDGET_ID__;
        if (!wid) return;
        var invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
          || (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
        if (typeof invoke !== 'function') return;
        Promise.resolve(invoke('popup_widget_menu', {{ id: wid }})).catch(function(){{}});
      }}, true);
    }} catch (err) {{}}
  }}
  if (document.readyState === 'loading') {{
    document.addEventListener('DOMContentLoaded', versaillesApplyHostChrome, {{ once: true }});
  }} else {{
    versaillesApplyHostChrome();
  }}
}})();"#
    )
}

fn clamp_position_to_monitors(app: &AppHandle, x: i32, y: i32, width: u32, height: u32) -> Position {
    let monitors = collect_monitor_rects(app);
    // During early startup monitors can be empty — trust the saved session coords.
    if monitors.is_empty() {
        return Position { x, y };
    }
    let w = width as i32;
    let h = height as i32;
    let intersects = monitors.iter().any(|m| {
        x + w > m.x && x < m.x + m.width && y + h > m.y && y < m.y + m.height
    });
    if intersects {
        return Position { x, y };
    }
    let primary = monitors.first().copied().unwrap_or(Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    });
    Position {
        x: primary.x + 40,
        y: primary.y + 40,
    }
}

struct OpenSpec {
    id: String,
    width: u32,
    height: u32,
    url: url::Url,
    always_on_top: bool,
    opacity: f64,
    border_radius: u32,
    default_position: Option<Position>,
    page_surface: bool,
}

fn resolve_open_spec(
    app: &AppHandle,
    id: &str,
    always_on_top: Option<bool>,
) -> AppResult<OpenSpec> {
    let state = app.state::<AppState>();
    let html = crate::desktop::read_desktop_html(&state);
    let cat = crate::page::parse_page(&html);

    if always_on_top != Some(true) && cat.is_desktop_widget(id) {
        return Err(AppError::msg(format!(
            "'{id}' is on the desktop page. Edit Documents\\Widgets\\desktop\\index.html instead of opening a floating window."
        )));
    }

    if let Some(piece) = cat.spawnable(id) {
        if piece.kind == crate::page::PageKind::ActionBar {
            return Err(AppError::msg(
                "The action bar is the Alt+Space overlay, not a spawn window.",
            ));
        }
        let url_str = crate::desktop::surface_url(&state, "spawn", Some(&piece.id))
            .ok_or_else(|| AppError::msg("Desktop page URL unavailable"))?;
        let mut url = url::Url::parse(&url_str).map_err(|e| AppError::msg(e.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("versaillesWidgetId", &piece.id);
            pairs.append_pair("deckWidgetId", &piece.id);
        }
        return Ok(OpenSpec {
            id: piece.id.clone(),
            width: piece.width,
            height: piece.height,
            url,
            always_on_top: true,
            opacity: 1.0,
            border_radius: 20,
            default_position: None,
            page_surface: true,
        });
    }

    let widget = {
        let registry = state.registry.lock().unwrap();
        registry
            .get(id)
            .cloned()
            .ok_or_else(|| crate::page::unknown_spawn(id))?
    };
    let port = {
        let config = state.config.lock().unwrap();
        config.api_bound_port.unwrap_or(config.api_port)
    };
    let mut url = crate::protocol::widget_http_url(&widget.entry_path, port).map_err(|err| {
        let _ = app.emit("widget://error", format!("Failed to open '{id}': {err}"));
        err
    })?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("versaillesWidgetId", id);
        pairs.append_pair("deckWidgetId", id);
    }
    Ok(OpenSpec {
        id: widget.manifest.id.clone(),
        width: widget.manifest.width,
        height: widget.manifest.height,
        url,
        always_on_top: always_on_top.unwrap_or(widget.manifest.always_on_top),
        opacity: widget.manifest.opacity.clamp(0.05, 1.0),
        border_radius: widget.manifest.border_radius,
        default_position: widget.manifest.default_position.clone(),
        page_surface: false,
    })
}

fn page_surface_chrome_script(id: &str) -> String {
    let id_js = serde_json::to_string(id).unwrap_or_else(|_| "null".into());
    format!(
        r#"(function(){{
  window.__VERSAILLES_WIDGET_ID__ = {id_js};
  window.__DECK_WIDGET_ID__ = {id_js};
}})();"#
    )
}

pub fn open_widget_window(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    id: &str,
    position: Option<Position>,
    always_on_top: Option<bool>,
) -> AppResult<OpenWidgetState> {
    static FIRST_OPEN_WIDGET_WINDOW: AtomicBool = AtomicBool::new(true);
    let is_first_open = FIRST_OPEN_WIDGET_WINDOW.swap(false, Ordering::Relaxed);

    // Cancel any pending deferred close for this id (close/reopen race).
    let _ = bump_close_generation(id);

    let spec = resolve_open_spec(app, id, always_on_top)?;
    let id = spec.id.as_str();

    let label = widget_label(id);
    if let Some(existing) = app.get_webview_window(&label) {
        let width = spec.width;
        let height = spec.height;
        // Prefer an explicit restore position over wherever the window currently sits.
        let raw_pos = position
            .clone()
            .or_else(|| {
                existing
                    .outer_position()
                    .ok()
                    .map(|p| Position { x: p.x, y: p.y })
            })
            .or(spec.default_position.clone())
            .unwrap_or(Position { x: 40, y: 40 });
        let clamped = clamp_position_to_monitors(app, raw_pos.x, raw_pos.y, width, height);
        suppress_snap_for(id, 1200);
        // Always re-apply physical coords on restore so DPI / show don't drift.
        defer_raise_widget(existing.clone(), Some(clamped.clone()));

        let on_top = spec.always_on_top;
        let state = {
            let mut mgr = manager.lock().unwrap();
            if let Some(state) = mgr.open.get_mut(id) {
                state.x = clamped.x;
                state.y = clamped.y;
                state.clone()
            } else {
                let state = OpenWidgetState {
                    id: id.to_string(),
                    label: label.clone(),
                    x: clamped.x,
                    y: clamped.y,
                    width,
                    height,
                    always_on_top: on_top,
                };
                mgr.open.insert(id.to_string(), state.clone());
                state
            }
        };
        if is_first_open {
            dismiss_prewarm_window(app);
        }
        let _ = app.emit("widget://opened", &state);
        return Ok(state);
    }

    let raw_pos = position
        .or(spec.default_position.clone())
        .unwrap_or(Position { x: 40, y: 40 });
    let pos = clamp_position_to_monitors(app, raw_pos.x, raw_pos.y, spec.width, spec.height);
    let on_top = spec.always_on_top;
    let url = spec.url.clone();
    let opacity = spec.opacity.clamp(0.05, 1.0);
    let radius = spec.border_radius;
    let inject = if spec.page_surface {
        page_surface_chrome_script(id)
    } else {
        widget_host_chrome_script(id, opacity, radius)
    };

    // Session coords are physical (from Moved / outer_position). Builder.position
    // is logical — set physical after build so restore matches the saved layout.
    let window = match WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title("")
        .inner_size(spec.width as f64, spec.height as f64)
        .decorations(false)
        .transparent(true)
        .always_on_top(on_top)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .focused(false)
        .shadow(false)
        .initialization_script(&inject)
        .build()
    {
        Ok(w) => w,
        Err(err) => {
            let _ = app.emit("widget://error", format!("Failed to open '{id}': {err}"));
            return Err(err.into());
        }
    };
    if is_first_open {
        // Runtime is warm from concurrent prewarm; release the helper window.
        dismiss_prewarm_window(app);
    }

    suppress_snap_for(id, 1200);
    let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
        x: pos.x,
        y: pos.y,
    }));
    let _ = window.set_decorations(false);
    let _ = window.set_shadow(false);
    let _ = window.set_skip_taskbar(true);
    force_borderless(&window);
    if on_top {
        let _ = window.set_always_on_top(true);
    }

    let sync_id = id.to_string();
    let sync_app = app.clone();
    window.on_window_event(move |event| {
        match event {
            tauri::WindowEvent::Moved(moved) => {
                {
                    let state = sync_app.state::<AppState>();
                    let mut mgr = state.window_manager.lock().unwrap();
                    if let Some(entry) = mgr.open.get_mut(&sync_id) {
                        entry.x = moved.x;
                        entry.y = moved.y;
                    }
                }
                // Ignore snap while restoring / applying programmatic moves —
                // otherwise startup Moved events rearrange the whole layout.
                if !snap_is_suppressed(&sync_id) {
                    schedule_snap_after_drag(sync_app.clone(), sync_id.clone());
                }
            }
            tauri::WindowEvent::Destroyed => {
                let state = sync_app.state::<AppState>();
                let removed = state.window_manager.lock().unwrap().open.remove(&sync_id);
                widget_menu_handler_labels()
                    .lock()
                    .unwrap()
                    .remove(&widget_label(&sync_id));
                if removed.is_some() {
                    let _ = sync_app.emit("widget://closed", &sync_id);
                    persist_session(&sync_app);
                }
            }
            _ => {}
        }
    });

    // Belt-and-suspenders: re-apply after show in case the first paint raced init.
    let delayed = window.clone();
    let reinject = inject.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        if let Err(err) = delayed.eval(&reinject) {
            tracing::warn!("widget host chrome re-eval failed: {err}");
        }
    });

    ensure_widget_menu_handler(app, &window, id);

    // Re-assert physical position after show — Win32 often nudges new windows.
    defer_raise_widget(window.clone(), Some(pos.clone()));
    force_borderless(&window);

    let state = OpenWidgetState {
        id: id.to_string(),
        label,
        x: pos.x,
        y: pos.y,
        width: spec.width,
        height: spec.height,
        always_on_top: on_top,
    };

    manager.lock().unwrap().open.insert(id.to_string(), state.clone());
    let _ = app.emit("widget://opened", &state);
    Ok(state)
}

pub fn popup_widget_context_menu(app: &AppHandle, id: &str) -> AppResult<()> {
    let label = widget_label(id);
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| AppError::msg(format!("Window for '{id}' missing")))?;

    let close =
        MenuItem::with_id(app, format!("wclose::{id}"), "Close widget", true, None::<&str>)?;
    let aot = MenuItem::with_id(
        app,
        format!("waot::{id}"),
        "Toggle always on top",
        true,
        None::<&str>,
    )?;
    let launcher_item =
        MenuItem::with_id(app, "wlauncher", "Open Versailles launcher", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&close, &aot, &launcher_item])?;

    window
        .popup_menu(&menu)
        .map_err(|e| AppError::msg(e.to_string()))?;
    Ok(())
}

pub fn close_widget_window(app: &AppHandle, manager: &Mutex<WindowManager>, id: &str) -> AppResult<()> {
    let label = widget_label(id);
    let gen = bump_close_generation(id);
    manager.lock().unwrap().open.remove(id);
    let _ = app.emit("widget://closed", id);
    if let Some(window) = app.get_webview_window(&label) {
        // Defer destroy so the sync command can return before WebView2 teardown.
        // Generation check cancels the close if open_widget_window raced in.
        // Only clear menu-handler tracking when the close actually runs — a
        // cancelled close must keep the existing on_menu_event registration.
        let app = app.clone();
        let id = id.to_string();
        let label_for_cleanup = label.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            let current = close_generation()
                .lock()
                .unwrap()
                .get(&id)
                .copied()
                .unwrap_or(0);
            if current != gen {
                return;
            }
            let id2 = id.clone();
            let _ = app.run_on_main_thread(move || {
                let current = close_generation()
                    .lock()
                    .unwrap()
                    .get(&id2)
                    .copied()
                    .unwrap_or(0);
                if current != gen {
                    return;
                }
                widget_menu_handler_labels()
                    .lock()
                    .unwrap()
                    .remove(&label_for_cleanup);
                let _ = window.close();
            });
        });
    } else {
        widget_menu_handler_labels().lock().unwrap().remove(&label);
    }
    Ok(())
}

pub fn move_widget(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    id: &str,
    x: i32,
    y: i32,
    disable_snap: bool,
    threshold: i32,
    monitors: &[Rect],
) -> AppResult<SnapResult> {
    let (candidate, others) = {
        let mgr = manager.lock().unwrap();
        let state = mgr
            .get(id)
            .ok_or_else(|| AppError::msg(format!("Widget '{id}' is not open")))?;
        let candidate = Rect {
            x,
            y,
            width: state.width as i32,
            height: state.height as i32,
        };
        (candidate, mgr.rects_except(id))
    };

    let result = if disable_snap {
        SnapResult {
            x,
            y,
            guides: Vec::new(),
        }
    } else {
        snap(candidate, &others, monitors, threshold)
    };

    apply_position(app, manager, id, result.x, result.y)?;
    let _ = app.emit("layout://guides", &result.guides);
    let _ = app.emit("layout://changed", manager.lock().unwrap().open_widgets());
    Ok(result)
}

pub fn apply_position(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    id: &str,
    x: i32,
    y: i32,
) -> AppResult<()> {
    let label = widget_label(id);
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| AppError::msg(format!("Window for '{id}' missing")))?;
    window.set_position(tauri::Position::Physical(PhysicalPosition { x, y }))?;

    suppress_snap_for(id, 400);
    if let Some(state) = manager.lock().unwrap().open.get_mut(id) {
        state.x = x;
        state.y = y;
    }
    Ok(())
}

pub fn set_always_on_top(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    id: &str,
    value: bool,
) -> AppResult<()> {
    let label = widget_label(id);
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| AppError::msg(format!("Window for '{id}' missing")))?;
    window.set_always_on_top(value)?;
    if let Some(state) = manager.lock().unwrap().open.get_mut(id) {
        state.always_on_top = value;
    }
    Ok(())
}

pub fn ensure_launcher_window(app: &AppHandle) -> AppResult<()> {
    if app.get_webview_window("launcher").is_some() {
        return Ok(());
    }

    // CSS expects a transparent native window so .cli's 18px card radius isn't boxed.
    let window = WebviewWindowBuilder::new(app, "launcher", WebviewUrl::App("launcher.html".into()))
        .title("Versailles Launcher")
        .inner_size(640.0, 420.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .focused(false)
        .shadow(false)
        .build()?;
    let _ = window.set_decorations(false);
    let _ = window.set_skip_taskbar(true);
    force_borderless(&window);
    clip_round_window(&window);
    // Keep the rounded clip in sync when the front-end grows into full terminal mode.
    let clip_win = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Resized(_) = event {
            clip_round_window(&clip_win);
        }
    });
    Ok(())
}

const LAUNCHER_DIM_LABEL: &str = "launcher-dim";

pub fn ensure_launcher_dim(app: &AppHandle) -> AppResult<()> {
    if app.get_webview_window(LAUNCHER_DIM_LABEL).is_some() {
        return Ok(());
    }

    // Plain transparent overlay — do NOT apply Acrylic/Blur/Mica here.
    // WebView2 + fullscreen window effects hangs the UI thread on Windows (AppHangB1).
    let window = WebviewWindowBuilder::new(
        app,
        LAUNCHER_DIM_LABEL,
        WebviewUrl::App("launcher-dim.html".into()),
    )
    .title("Versailles Launcher Backdrop")
    .inner_size(800.0, 600.0)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .visible(false)
    .focused(false)
    .shadow(false)
    .build()?;
    let _ = window.set_decorations(false);
    let _ = window.set_skip_taskbar(true);
    Ok(())
}

/// Physical bounds of the monitor that should host the Action Bar + backdrop.
fn launcher_monitor_bounds(window: &WebviewWindow) -> (i32, i32, u32, u32, f64) {
    let cursor = crate::hotkeys::launcher_anchor();
    let monitors = window.available_monitors().unwrap_or_default();
    if let Some((cx, cy)) = cursor {
        for monitor in &monitors {
            let pos = monitor.position();
            let size = monitor.size();
            let left = pos.x;
            let top = pos.y;
            let right = left + size.width as i32;
            let bottom = top + size.height as i32;
            if cx >= left && cx < right && cy >= top && cy < bottom {
                return (left, top, size.width, size.height, monitor.scale_factor());
            }
        }
    }
    if let Ok(Some(monitor)) = window.current_monitor() {
        let pos = monitor.position();
        let size = monitor.size();
        return (pos.x, pos.y, size.width, size.height, monitor.scale_factor());
    }
    (0, 0, 1920, 1080, 1.0)
}

pub fn show_launcher(app: &AppHandle) -> AppResult<()> {
    // Never create WebView2 synchronously on the hotkey/IPC thread — that hangs Windows.
    // Prewarm owns creation; if somehow missing, create off-thread then show.
    if app.get_webview_window("launcher").is_none()
        || app.get_webview_window(LAUNCHER_DIM_LABEL).is_none()
    {
        tracing::warn!("launcher windows not ready; creating off hotkey thread");
        let app = app.clone();
        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let handle = app.clone();
            let tx1 = tx.clone();
            let _ = app.run_on_main_thread(move || {
                let _ = tx1.send(ensure_launcher_window(&handle));
            });
            let _ = rx.recv_timeout(Duration::from_secs(8));

            let handle = app.clone();
            let tx2 = tx;
            let _ = app.run_on_main_thread(move || {
                let _ = tx2.send(ensure_launcher_dim(&handle));
            });
            let _ = rx.recv_timeout(Duration::from_secs(8));

            let _ = show_launcher_ready(&app);
        });
        return Ok(());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = show_launcher_ready(&handle);
        });
    });
    Ok(())
}

fn show_launcher_ready(app: &AppHandle) -> AppResult<()> {
    let window = app
        .get_webview_window("launcher")
        .ok_or_else(|| AppError::msg("Launcher window missing"))?;

    let (left, top, width, height, scale) = launcher_monitor_bounds(&window);

    if let Some(dim) = app.get_webview_window(LAUNCHER_DIM_LABEL) {
        let _ = dim.set_position(tauri::Position::Physical(PhysicalPosition { x: left, y: top }));
        let _ = dim.set_size(tauri::Size::Physical(PhysicalSize { width, height }));
        let _ = dim.show();
    }

    let bar_w = (640.0 * scale) as i32;
    let x = left + (width as i32 - bar_w) / 2;
    let y = top + (120.0 * scale) as i32;
    window.set_position(tauri::Position::Physical(PhysicalPosition { x, y }))?;

    let _ = window.set_decorations(false);
    force_borderless(&window);
    clip_round_window(&window);
    let _ = window.show();
    force_borderless(&window);
    clip_round_window(&window);
    let _ = window.set_always_on_top(true);
    defer_window_focus(window);
    let seed = take_launcher_seed().unwrap_or_default();
    let _ = app.emit("launcher://shown", seed);
    Ok(())
}

/// Hide dim + launcher off the calling invoke/hotkey thread.
/// Sync hide from a webview invoke re-enters WebView2 and can Application Hang.
pub fn hide_launcher(app: &AppHandle) -> AppResult<()> {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(dim) = handle.get_webview_window(LAUNCHER_DIM_LABEL) {
                let _ = dim.hide();
            }
            if let Some(window) = handle.get_webview_window("launcher") {
                let _ = window.hide();
            }
            let _ = handle.emit("launcher://hidden", true);
        });
    });
    Ok(())
}

// NEVER await window hide/show/resize/focus from the launcher webview's own invoke path —
// that deadlocks WebView2 on Windows (Application Hang). Frontend uses setSize/hide;
// Rust hide_launcher is deferred. Do not reintroduce set_launcher_mode as a command.

pub fn ensure_guides_window(app: &AppHandle) -> AppResult<()> {
    if app.get_webview_window("guides").is_some() {
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, "guides", WebviewUrl::App("guides.html".into()))
        .title("Versailles Guides")
        .fullscreen(true)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .shadow(false)
        .build()?;
    let _ = window.set_ignore_cursor_events(true);
    force_borderless(&window);
    Ok(())
}

pub fn ensure_canvas_window(app: &AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("canvas") {
        window.show()?;
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, "canvas", WebviewUrl::App("canvas.html".into()))
        .title("Versailles Canvas")
        .fullscreen(true)
        .decorations(false)
        .transparent(true)
        .always_on_bottom(true)
        .skip_taskbar(true)
        .visible(false)
        .shadow(false)
        .build()?;
    window.show()?;
    Ok(())
}

pub fn close_canvas_window(app: &AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("canvas") {
        window.close()?;
    }
    Ok(())
}

/// Monitor work area (excludes the native taskbar). Full-monitor + HWND_BOTTOM
/// covers the bar then drops behind Progman — desk and taskbar "fight".
fn desktop_work_area(window: &WebviewWindow) -> (i32, i32, u32, u32) {
    #[cfg(windows)]
    {
        if let Ok(hwnd_raw) = window.hwnd() {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::Graphics::Gdi::{
                GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            };
            let hwnd = HWND(hwnd_raw.0);
            unsafe {
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut info).as_bool() {
                    let r = info.rcWork;
                    let width = (r.right - r.left).max(1) as u32;
                    let height = (r.bottom - r.top).max(1) as u32;
                    return (r.left, r.top, width, height);
                }
            }
        }
    }
    let (left, top, width, height, _scale) = launcher_monitor_bounds(window);
    (left, top, width, height)
}

#[cfg(windows)]
fn shell_desktop_hwnds() -> Vec<windows::Win32::Foundation::HWND> {
    use windows::core::w;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, FindWindowW, GetClassNameW};

    let mut out = Vec::new();
    if let Ok(hwnd) = unsafe { FindWindowW(w!("Progman"), None) } {
        out.push(hwnd);
    }
    unsafe extern "system" fn on_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) };
        if n > 0 {
            let class = String::from_utf16_lossy(&buf[..n as usize]);
            if class == "WorkerW" || class == "Progman" {
                list.push(hwnd);
            }
        }
        true.into()
    }
    let _ = unsafe { EnumWindows(Some(on_window), LPARAM(&mut out as *mut _ as isize)) };
    out
}

/// Sit just above the Windows wallpaper, below apps and the taskbar.
/// `always_on_bottom` alone sends the window behind Progman so it vanishes.
#[cfg(windows)]
fn pin_desktop_above_shell(window: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    let Ok(hwnd_raw) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(hwnd_raw.0);
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, flags);
        for shell in shell_desktop_hwnds() {
            if shell != hwnd {
                let _ = SetWindowPos(shell, Some(HWND_BOTTOM), 0, 0, 0, 0, flags);
            }
        }
    }
}

#[cfg(not(windows))]
fn pin_desktop_above_shell(_window: &WebviewWindow) {}

fn start_desktop_zorder_keeper(app: &AppHandle) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        for delay_ms in [80u64, 250, 800, 2000, 5000] {
            std::thread::sleep(Duration::from_millis(delay_ms));
            if app.get_webview_window("desktop").is_none() {
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(window) = handle.get_webview_window("desktop") {
                    pin_desktop_above_shell(&window);
                }
            });
        }
        loop {
            std::thread::sleep(Duration::from_secs(4));
            if app.get_webview_window("desktop").is_none() {
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(window) = handle.get_webview_window("desktop") {
                    pin_desktop_above_shell(&window);
                }
            });
        }
    });
}

fn place_desktop_window(window: &WebviewWindow) {
    // Work area only — never cover the native taskbar. No exclusive fullscreen
    // (hides the bar / can hang WebView2). No Tauri always_on_bottom (vanishes
    // behind the wallpaper).
    let (left, top, width, height) = desktop_work_area(window);
    let _ = window.set_always_on_bottom(false);
    let _ = window.set_position(tauri::Position::Physical(PhysicalPosition { x: left, y: top }));
    let _ = window.set_size(tauri::Size::Physical(PhysicalSize { width, height }));
    pin_desktop_above_shell(window);
}

pub fn ensure_desktop_window(app: &AppHandle, _page_url: Option<String>) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("desktop") {
        place_desktop_window(&window);
        let _ = window.show();
        pin_desktop_above_shell(&window);
        start_desktop_zorder_keeper(app);
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, "desktop", WebviewUrl::App("desktop.html".into()))
        .title("Versailles Desktop")
        .inner_size(1280.0, 720.0)
        .decorations(false)
        .transparent(false)
        .skip_taskbar(true)
        .visible(false)
        .shadow(false)
        .build()?;
    let _ = window.set_skip_taskbar(true);
    let pin_win = window.clone();
    let _ = window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) | tauri::WindowEvent::Moved(_) = event {
            pin_desktop_above_shell(&pin_win);
        }
    });
    place_desktop_window(&window);
    window.show()?;
    pin_desktop_above_shell(&window);
    start_desktop_zorder_keeper(app);
    Ok(())
}

pub fn close_desktop_window(app: &AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("desktop") {
        window.close()?;
    }
    Ok(())
}

fn launcher_seed_slot() -> &'static Mutex<Option<String>> {
    static SEED: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SEED.get_or_init(|| Mutex::new(None))
}

pub fn set_launcher_seed(seed: String) {
    let value = seed.trim().to_string();
    *launcher_seed_slot().lock().unwrap() = if value.is_empty() {
        None
    } else {
        Some(value)
    };
}

fn take_launcher_seed() -> Option<String> {
    launcher_seed_slot().lock().unwrap().take()
}

pub fn dock_slideout_position(app: &AppHandle, width: u32, height: u32) -> Position {
    let window = app.webview_windows().into_values().next();
    let Some(window) = window else {
        return Position { x: 40, y: 80 };
    };
    let (left, top, mon_w, mon_h, scale) = launcher_monitor_bounds(&window);
    // Keep slide-outs below the native top taskbar/hud area.
    let hud = (56.0 * scale).round() as i32;
    let gap = (14.0 * scale).round() as i32;
    let x = left + mon_w as i32 - width as i32 - gap;
    let y = top + hud + gap;
    let max_y = top + mon_h as i32 - height as i32 - gap;
    Position {
        x,
        y: y.min(max_y).max(top + hud + gap),
    }
}

pub fn toggle_slideout_widget(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    id: &str,
) -> AppResult<bool> {
    let key = id.trim();
    if key.eq_ignore_ascii_case("action-bar") || key.eq_ignore_ascii_case("action_bar") {
        let visible = app
            .get_webview_window("launcher")
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        if visible {
            hide_launcher(app)?;
            return Ok(false);
        }
        show_launcher(app)?;
        return Ok(true);
    }

    let label = widget_label(id);
    if app.get_webview_window(&label).is_some() {
        close_widget_window(app, manager, id)?;
        return Ok(false);
    }
    let (width, height) = {
        let state = app.state::<AppState>();
        let cat = crate::desktop::page_catalog(&state);
        if let Some(piece) = cat.spawnable(id) {
            (piece.width, piece.height)
        } else {
            let registry = state.registry.lock().unwrap();
            let widget = registry
                .get(id)
                .ok_or_else(|| crate::page::unknown_spawn(id))?;
            (widget.manifest.width, widget.manifest.height)
        }
    };
    let pos = dock_slideout_position(app, width, height);
    open_widget_window(app, manager, id, Some(pos), Some(true))?;
    Ok(true)
}

pub fn collect_monitor_rects(app: &AppHandle) -> Vec<Rect> {
    let Some(window) = app
        .webview_windows()
        .into_values()
        .next()
        .or_else(|| app.get_webview_window("main"))
    else {
        return vec![Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
    };

    match window.available_monitors() {
        Ok(monitors) => monitors
            .into_iter()
            .map(|m| {
                let pos = m.position();
                let size = m.size();
                Rect {
                    x: pos.x,
                    y: pos.y,
                    width: size.width as i32,
                    height: size.height as i32,
                }
            })
            .collect(),
        Err(_) => vec![Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }],
    }
}

pub fn monitor_fingerprints(app: &AppHandle) -> Vec<MonitorFingerprint> {
    collect_monitor_rects(app)
        .into_iter()
        .map(|r| MonitorFingerprint {
            width: r.width.max(0) as u32,
            height: r.height.max(0) as u32,
            scale_factor: 1.0,
        })
        .collect()
}

pub fn build_layout_template(app: &AppHandle, manager: &WindowManager, name: &str) -> LayoutTemplate {
    let mut widgets = HashMap::new();
    for state in manager.open_widgets() {
        widgets.insert(
            state.id,
            LayoutPlacement {
                x: state.x,
                y: state.y,
                monitor_id: "primary".into(),
            },
        );
    }
    LayoutTemplate {
        name: name.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        monitors: monitor_fingerprints(app),
        widgets,
    }
}

pub fn apply_layout_template(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    layout: &LayoutTemplate,
) -> AppResult<Vec<OpenWidgetState>> {
    let html = {
        let state = app.state::<AppState>();
        crate::desktop::read_desktop_html(&state)
    };
    let open_ids: Vec<String> = manager.lock().unwrap().open.keys().cloned().collect();
    for id in open_ids {
        if crate::desktop::html_embeds_widget(&html, &id) || !layout.widgets.contains_key(&id) {
            close_widget_window(app, manager, &id)?;
        }
    }

    let mut opened = Vec::new();
    for (id, placement) in &layout.widgets {
        if crate::desktop::html_embeds_widget(&html, id) {
            continue;
        }
        let state = open_widget_window(
            app,
            manager,
            id,
            Some(Position {
                x: placement.x,
                y: placement.y,
            }),
            None,
        )?;
        opened.push(state);
    }
    Ok(opened)
}

pub fn set_guides_visible(app: &AppHandle, visible: bool, guides: &[SnapGuide]) -> AppResult<()> {
    ensure_guides_window(app)?;
    if let Some(window) = app.get_webview_window("guides") {
        if visible && !guides.is_empty() {
            window.show()?;
        } else {
            window.hide()?;
        }
    }
    let _ = app.emit("layout://guides", guides);
    Ok(())
}
