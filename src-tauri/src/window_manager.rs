use crate::config::{LayoutPlacement, LayoutTemplate, MonitorFingerprint, Position, SessionWidget};
use crate::error::{AppError, AppResult};
use crate::layout::{snap, Rect, SnapGuide, SnapResult};
use crate::placement::{self, Align, Anchor, DimMode};

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{
    menu::{Menu, MenuItem},
    AppHandle, Emitter, Listener, Manager, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

/// Position and size are both physical pixels (snap math runs against physical
/// monitor rects).
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

/// Physical outer size of a widget window; falls back to scaling the logical
/// spec size when the window cannot report it yet.
fn physical_size(window: &WebviewWindow, width: u32, height: u32) -> (u32, u32) {
    if let Ok(size) = window.outer_size() {
        if size.width > 0 && size.height > 0 {
            return (size.width, size.height);
        }
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    (
        (width as f64 * scale).round() as u32,
        (height as f64 * scale).round() as u32,
    )
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

/// Label of a `data-monitor="all"` bar's copy on display `n` (the copy on the
/// primary keeps `widget-<id>`). `page::classify_invoker` maps it back to `<id>`.
pub fn monitor_copy_label(id: &str, n: u32) -> String {
    format!("widget-{id}:{n}")
}

/// Open windows of page piece `id`: `widget-<id>` and its monitor copies.
pub fn piece_window_labels(app: &AppHandle, id: &str) -> Vec<String> {
    let mut labels: Vec<String> = app
        .webview_windows()
        .into_keys()
        .filter(|l| crate::page::label_piece_id(l).is_some_and(|p| p.eq_ignore_ascii_case(id)))
        .collect();
    labels.sort();
    labels
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

fn ensure_widget_menu_handler(app: &AppHandle, window: &WebviewWindow) {
    let label = window.label().to_string();
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
            if let Some(id) = hotkey_piece_id(&app_handle) {
                toggle_hotkey_piece(&app_handle, &id);
            }
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

pub(crate) fn persistable_session(app: &AppHandle) -> Vec<SessionWidget> {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let snap = state.window_manager.lock().unwrap().session_snapshot();
    snap.into_iter()
        .filter(|w| cat.spawnable(&w.id).is_none())
        .collect()
}

pub(crate) fn persist_session(app: &AppHandle) {
    let state = app.state::<AppState>();
    // persistable_session → page_catalog → read_desktop_html locks config.
    // Snapshot first: std Mutex is not reentrant, so holding it here deadlocks
    // the caller, then the API runtime, then the UI thread (WebView2 GPU kill).
    let session = persistable_session(app);
    let mut config = state.config.lock().unwrap();
    config.session_widgets = session;
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

/// Overlays are plain rectangles: drop any HWND region so the page owns its corners
/// (transparent window + CSS `border-radius` if a piece wants rounding).
#[cfg(windows)]
fn clip_round_window(window: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{SetWindowRgn, HRGN};

    let Ok(hwnd_raw) = window.hwnd() else {
        return;
    };
    unsafe {
        let _ = SetWindowRgn(HWND(hwnd_raw.0), Some(HRGN::default()), true);
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
    // rounded clip is overlay-only (see clip_round_window).

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

/// Raise a widget off the calling thread. Sync show/focus on the UI/IPC thread
/// can re-enter WebView2 and freeze the whole desktop.
/// Windows the host places itself (docked bars): the deferred raise shows
/// them without moving or focusing them.
fn quiet_labels() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Raise `label` without moving or focusing it.
pub fn mark_quiet(label: &str) {
    quiet_labels().lock().unwrap().insert(label.to_string());
}

fn defer_raise_widget(window: WebviewWindow, position: Option<Position>) {
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        let _ = app.run_on_main_thread(move || {
            let quiet = quiet_labels().lock().unwrap().contains(window.label());
            if let Some(pos) = position.filter(|_| !quiet) {
                let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
                    x: pos.x,
                    y: pos.y,
                }));
            }
            let _ = window.unminimize();
            let _ = window.show();
            if !quiet {
                let _ = window.set_focus();
            }
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
    resizable: bool,
    /// Centered overlays stay transparent; docked slide-outs are opaque.
    /// Transparent WebView2 plus GPU-heavy page surfaces can hang the process.
    transparent: bool,
}

fn resolve_open_spec(
    app: &AppHandle,
    id: &str,
    always_on_top: Option<bool>,
) -> AppResult<OpenSpec> {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);

    if always_on_top != Some(true) && cat.is_desktop_widget(id) {
        return Err(AppError::msg(format!(
            "'{id}' is on the desktop page. Edit Documents\\Widgets\\desktop\\index.html instead of opening a floating window."
        )));
    }

    if let Some(piece) = cat.spawnable(id) {
        let url_str = crate::desktop::surface_url(&state, "spawn", Some(&piece.id))
            .ok_or_else(|| AppError::msg("Desktop page URL unavailable"))?;
        let mut url = url::Url::parse(&url_str).map_err(|e| AppError::msg(e.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("versaillesWidgetId", &piece.id);
            pairs.append_pair("deckWidgetId", &piece.id);
        }
        let overlay = crate::page::piece_is_overlay(piece);
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
            resizable: overlay,
            transparent: overlay,
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
        resizable: false,
        transparent: true,
    })
}

fn page_surface_chrome_script(id: &str, opaque: bool, allow_drag: bool) -> String {
    let id_js = serde_json::to_string(id).unwrap_or_else(|_| "null".into());
    // Opaque windows need a backdrop, but it is only a fallback: `:where()` has
    // zero specificity, so any background rule on the page wins.
    let opaque_js = if opaque {
        r#"try{if(!document.getElementById('versailles-opaque-bg')){var b=document.createElement('style');b.id='versailles-opaque-bg';b.textContent=':where(html),:where(body){background:#f9fafb;}';(document.head||document.documentElement).appendChild(b);}}catch(e){}"#
    } else {
        ""
    };
    let skip_drag = if allow_drag {
        ""
    } else {
        "window.__VERSAILLES_NATIVE_CHROME__=true;window.__DECK_NATIVE_CHROME__=true;"
    };
    format!(
        r#"(function(){{
  window.__VERSAILLES_WIDGET_ID__ = {id_js};
  window.__DECK_WIDGET_ID__ = {id_js};
  {skip_drag}
  {opaque_js}
  function versaillesBindPageChrome() {{
    try {{
      if (window.__VERSAILLES_NATIVE_CHROME__ || window.__DECK_NATIVE_CHROME__) return;
      // The host re-runs this script once after show: bind (or wait) once.
      if (window.__VERSAILLES_CHROME_BINDING__) return;
      window.__VERSAILLES_CHROME_BINDING__ = true;
      var root = document.querySelector('.spawn-surface') || document.body;
      if (window.versailles && typeof window.versailles.enableNativeChrome === 'function') {{
        window.versailles.enableNativeChrome(root);
        return;
      }}
      var n = 0;
      var t = setInterval(function () {{
        if (window.versailles && typeof window.versailles.enableNativeChrome === 'function') {{
          clearInterval(t);
          window.versailles.enableNativeChrome(root);
        }} else if (++n > 40) {{
          clearInterval(t);
        }}
      }}, 100);
    }} catch (err) {{}}
  }}
  if (document.readyState === 'loading') {{
    document.addEventListener('DOMContentLoaded', versaillesBindPageChrome, {{ once: true }});
  }} else {{
    versaillesBindPageChrome();
  }}
}})();"#
    )
}

/// How a newly built window shows its first frame.
#[derive(Debug, Clone, Copy, Default)]
struct FirstFrame {
    /// Physical rect (and its monitor's scale) to build the window at, before
    /// it is ever shown. Otherwise the spec size at the open position.
    rect: Option<(Rect, f64)>,
    /// Stay hidden until the page marks `html[data-stage]`, at most
    /// [`STAGE_WAIT_MS`].
    wait_for_stage: bool,
}

pub fn open_widget_window(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    id: &str,
    position: Option<Position>,
    always_on_top: Option<bool>,
) -> AppResult<OpenWidgetState> {
    open_widget_window_as(app, manager, id, position, always_on_top, None, FirstFrame::default())
}

/// One window of a docked bar: its label (`widget-<id>`, or a monitor copy
/// `widget-<id>:<n>`) and the monitor it sits on. The page URL gets
/// `monitor=<n>&primary=0|1`, like a wallpaper's.
pub struct DockInstance {
    pub label: String,
    pub monitor: u32,
    pub primary: bool,
}

impl DockInstance {
    fn is_copy(&self, id: &str) -> bool {
        self.label != widget_label(id)
    }
}

/// `monitor=<n>&primary=0|1` on `url`, replacing earlier values.
pub fn with_monitor_params(url: &url::Url, monitor: u32, primary: bool) -> url::Url {
    let mut next = url.clone();
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "monitor" && k != "primary")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    next.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("monitor", &monitor.to_string())
        .append_pair("primary", if primary { "1" } else { "0" });
    next
}

/// Open (or raise) one window of docked bar `id`. A monitor copy is not
/// tracked by the window manager (no session, no snapping); it lives and dies
/// with `docks`.
pub fn open_dock_window(app: &AppHandle, id: &str, instance: DockInstance, position: Position) -> AppResult<()> {
    let state = app.state::<AppState>();
    open_widget_window_as(
        app,
        &state.window_manager,
        id,
        Some(position),
        Some(true),
        Some(instance),
        FirstFrame::default(),
    )
    .map(|_| ())
}

/// A docked bar's window moved to another monitor (or the primary changed):
/// reload its page with the new `monitor` / `primary` parameters.
pub fn retarget_dock_window(window: &WebviewWindow, monitor: u32, primary: bool) {
    if let Ok(url) = window.url() {
        let next = with_monitor_params(&url, monitor, primary);
        if next != url {
            let _ = window.navigate(next);
        }
    }
}

fn open_widget_window_as(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    id: &str,
    position: Option<Position>,
    always_on_top: Option<bool>,
    instance: Option<DockInstance>,
    first: FirstFrame,
) -> AppResult<OpenWidgetState> {
    static FIRST_OPEN_WIDGET_WINDOW: AtomicBool = AtomicBool::new(true);
    let is_first_open = FIRST_OPEN_WIDGET_WINDOW.swap(false, Ordering::Relaxed);

    // Cancel any pending deferred close for this id (close/reopen race).
    let _ = bump_close_generation(id);

    let mut spec = resolve_open_spec(app, id, always_on_top)?;
    if let Some(inst) = instance.as_ref() {
        spec.url = with_monitor_params(&spec.url, inst.monitor, inst.primary);
    }
    let id = spec.id.as_str();
    // Monitor copies stay out of the window manager (session, snapping, menus).
    let copy = instance.as_ref().is_some_and(|i| i.is_copy(id));

    let label = instance
        .as_ref()
        .map(|i| i.label.clone())
        .unwrap_or_else(|| widget_label(id));
    if copy {
        if let Some(existing) = app.get_webview_window(&label) {
            defer_raise_widget(existing.clone(), position.clone());
            let (width, height) = physical_size(&existing, spec.width, spec.height);
            let pos = position.unwrap_or(Position { x: 0, y: 0 });
            return Ok(OpenWidgetState {
                id: id.to_string(),
                label,
                x: pos.x,
                y: pos.y,
                width,
                height,
                always_on_top: true,
            });
        }
    }
    if let Some(existing) = app.get_webview_window(&label) {
        let (width, height) = physical_size(&existing, spec.width, spec.height);
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
    let pos = match first.rect {
        Some((rect, _)) => Position { x: rect.x, y: rect.y },
        None => clamp_position_to_monitors(app, raw_pos.x, raw_pos.y, spec.width, spec.height),
    };
    // Logical build size; a first-frame rect is applied in physical px below.
    let (build_w, build_h) = match first.rect {
        Some((rect, scale)) => {
            let s = if scale > 0.0 { scale } else { 1.0 };
            (rect.width as f64 / s, rect.height as f64 / s)
        }
        None => (spec.width as f64, spec.height as f64),
    };
    let on_top = spec.always_on_top;
    let url = spec.url.clone();
    let opacity = spec.opacity.clamp(0.05, 1.0);
    let radius = spec.border_radius;
    let chrome = if spec.page_surface {
        let allow_drag = {
            let state = app.state::<AppState>();
            let cat = crate::desktop::page_catalog(&state);
            let user = state
                .store
                .lock()
                .unwrap()
                .load_user_config()
                .unwrap_or_default();
            cat.spawnable(id)
                .map(|p| crate::page::piece_draggable(p, &user))
                .unwrap_or(true)
        };
        page_surface_chrome_script(id, !spec.transparent, allow_drag)
    } else {
        widget_host_chrome_script(id, opacity, radius)
    };
    // Queued scripts (a `page`-dim stage, spawn data) run before any page
    // script, so the first frame already has them.
    let inject = match pending_init_scripts().lock().unwrap().remove(&id.to_ascii_lowercase()) {
        Some(scripts) => format!("{chrome}\n{}", scripts.join("\n")),
        None => chrome.clone(),
    };

    // Session coords are physical (from Moved / outer_position). Builder.position
    // is logical — set physical after build so restore matches the saved layout.
    let window = match WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title("")
        .inner_size(build_w, build_h)
        .decorations(false)
        .transparent(spec.transparent)
        .always_on_top(on_top)
        .skip_taskbar(true)
        .resizable(spec.resizable)
        .visible(false)
        .focused(false)
        .shadow(false)
        .initialization_script(&crate::gesture::init_script())
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
    match first.rect {
        // Exact physical rect while still hidden: the first frame is final.
        Some((rect, _)) => apply_rect(&window, rect, true),
        None => {
            let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
                x: pos.x,
                y: pos.y,
            }));
        }
    }
    let _ = window.set_decorations(false);
    let _ = window.set_shadow(false);
    let _ = window.set_skip_taskbar(true);
    force_borderless(&window);
    if on_top {
        let _ = window.set_always_on_top(true);
    }

    let overlay = {
        let state = app.state::<AppState>();
        crate::desktop::page_catalog(&state).is_overlay(id)
    };
    // Docked bars are placed by the host: never snap them after a move.
    let no_snap = overlay || crate::docks::is_dock(app, id);
    let sync_id = id.to_string();
    let sync_label = label.clone();
    let sync_app = app.clone();
    window.on_window_event(move |event| {
        match event {
            tauri::WindowEvent::Moved(moved) => {
                if !copy {
                    let state = sync_app.state::<AppState>();
                    let mut mgr = state.window_manager.lock().unwrap();
                    if let Some(entry) = mgr.open.get_mut(&sync_id) {
                        entry.x = moved.x;
                        entry.y = moved.y;
                    }
                }
                // Ignore snap while restoring / applying programmatic moves —
                // otherwise startup Moved events rearrange the whole layout.
                if !no_snap && !snap_is_suppressed(&sync_id) {
                    schedule_snap_after_drag(sync_app.clone(), sync_id.clone());
                }
            }
            tauri::WindowEvent::Resized(_) => {
                if overlay {
                    if let Some(w) = sync_app.get_webview_window(&sync_label) {
                        clip_round_window(&w);
                    }
                }
            }
            tauri::WindowEvent::Focused(focused) => {
                if overlay && !*focused && !overlay_focus_is_guarded() {
                    let state = sync_app.state::<AppState>();
                    let user = state
                        .store
                        .lock()
                        .unwrap()
                        .load_user_config()
                        .unwrap_or_default();
                    let cat = crate::desktop::page_catalog(&state);
                    let dismiss = cat
                        .spawnable(&sync_id)
                        .map(|p| crate::page::piece_dismiss_on_blur(p, &user))
                        .unwrap_or(true);
                    if dismiss {
                        let _ = hide_launcher(&sync_app);
                    }
                }
            }
            tauri::WindowEvent::Destroyed => {
                hit_regions().lock().unwrap().remove(&sync_label);
                widget_menu_handler_labels().lock().unwrap().remove(&sync_label);
                if copy {
                    return;
                }
                let state = sync_app.state::<AppState>();
                let removed = state.window_manager.lock().unwrap().open.remove(&sync_id);
                if removed.is_some() {
                    let _ = sync_app.emit("widget://closed", &sync_id);
                    persist_session(&sync_app);
                }
            }
            _ => {}
        }
    });

    // Belt-and-suspenders: re-apply the chrome after show in case the first
    // paint raced init. Only the chrome (it is idempotent): queued scripts
    // fire `versailles:spawn` / `versailles:stage`, which must arrive once.
    {
        let delayed = window.clone();
        let reinject = chrome;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            if let Err(err) = delayed.eval(&reinject) {
                tracing::warn!("widget host chrome re-eval failed: {err}");
            }
        });
    }

    ensure_widget_menu_handler(app, &window);

    if first.wait_for_stage {
        show_when_staged(window.clone(), id, pos.clone());
    } else {
        defer_raise_widget(window.clone(), Some(pos.clone()));
    }
    force_borderless(&window);

    let (width, height) = physical_size(&window, spec.width, spec.height);
    let state = OpenWidgetState {
        id: id.to_string(),
        label,
        x: pos.x,
        y: pos.y,
        width,
        height,
        always_on_top: on_top,
    };

    if !copy {
        manager.lock().unwrap().open.insert(id.to_string(), state.clone());
        let _ = app.emit("widget://opened", &state);
    }
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
        MenuItem::with_id(app, "wlauncher", "Open overlay", true, None::<&str>)?;
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

const LAUNCHER_DIM_LABEL: &str = "launcher-dim";

fn overlay_focus_until() -> &'static Mutex<Instant> {
    static SLOT: OnceLock<Mutex<Instant>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Instant::now()))
}

fn arm_overlay_focus_guard() {
    *overlay_focus_until().lock().unwrap() = Instant::now() + Duration::from_millis(500);
}

/// Extend blur-dismiss guard from spawnable engines (resize, focus steal).
pub fn arm_overlay_focus_guard_ms(ms: u64) {
    let ms = ms.clamp(50, 5000);
    *overlay_focus_until().lock().unwrap() = Instant::now() + Duration::from_millis(ms);
}

fn overlay_focus_is_guarded() -> bool {
    Instant::now() < *overlay_focus_until().lock().unwrap()
}

pub fn hotkey_piece_id(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let primary = state.config.lock().unwrap().launcher_primary.clone();
    crate::desktop::page_catalog(&state)
        .hotkey_piece_with_primary(primary.as_deref())
        .map(|p| p.id.clone())
}

pub fn hotkey_overlay_visible(app: &AppHandle) -> bool {
    let Some(id) = hotkey_piece_id(app) else {
        return false;
    };
    app.get_webview_window(&widget_label(&id))
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// Physical rect + scale of the monitor under the cursor (overlays open there).
fn cursor_monitor(app: &AppHandle) -> Option<(Rect, f64)> {
    let window = app.webview_windows().into_values().next()?;
    let (left, top, width, height, scale) = launcher_monitor_bounds(&window);
    Some((
        Rect {
            x: left,
            y: top,
            width: width as i32,
            height: height as i32,
        },
        scale,
    ))
}

fn window_rect(window: &WebviewWindow) -> Option<Rect> {
    let pos = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    Some(Rect {
        x: pos.x,
        y: pos.y,
        width: size.width as i32,
        height: size.height as i32,
    })
}

/// Physical rect + scale of the monitor holding the window's centre.
fn window_monitor(window: &WebviewWindow, win: Rect) -> Option<(Rect, f64)> {
    let cx = win.x + win.width / 2;
    let cy = win.y + win.height / 2;
    window
        .available_monitors()
        .ok()?
        .into_iter()
        .map(|m| {
            let rect = Rect {
                x: m.position().x,
                y: m.position().y,
                width: m.size().width as i32,
                height: m.size().height as i32,
            };
            (rect, m.scale_factor())
        })
        .find(|(r, _)| cx >= r.x && cx < r.x + r.width && cy >= r.y && cy < r.y + r.height)
}

/// Physical rect of `piece` at its anchor + offset on `mon`. `size` overrides
/// the catalog size (`data-w` / `data-h`).
fn piece_anchor_rect(
    piece: &crate::page::PagePiece,
    mon: Rect,
    scale: f64,
    size: Option<(i32, i32)>,
) -> Rect {
    let px = |n: f64| (n * scale).round() as i32;
    let (width, height) =
        size.unwrap_or((px(piece.width as f64), px(piece.height as f64)));
    let (ox, oy) = crate::page::piece_offset(piece);
    let anchor = crate::page::piece_anchor(piece)
        .unwrap_or(Anchor::Grid(Align::Center, Align::Center));
    placement::anchor_rect(
        anchor,
        mon,
        width,
        height,
        (px(ox as f64), px(oy as f64)),
        crate::hotkeys::launcher_anchor(),
    )
}

/// Where an overlay goes on `mon` (the cursor monitor). On first open, or for
/// `c` / `center` / `overlay`, that is the piece's anchor at catalog size. A
/// window that `covers` its monitor (`page` dim, `opts.coverMonitor`) is the
/// monitor, from the first open on. Every other anchor keeps the page's
/// layout from the existing window (`cursor` keeps its size and follows the
/// cursor).
fn overlay_rect(
    window: Option<&WebviewWindow>,
    piece: &crate::page::PagePiece,
    covers: bool,
    mon: Rect,
    scale: f64,
) -> Rect {
    if covers {
        return mon;
    }
    let current = window.and_then(|w| window_rect(w).map(|r| (w, r)));
    let keeps = crate::page::piece_keeps_layout(piece) || page_owns_layout(&piece.id);
    match current {
        Some((window, win)) if keeps => {
            if crate::page::piece_anchor(piece) == Some(Anchor::Cursor) {
                return piece_anchor_rect(piece, mon, scale, Some((win.width, win.height)));
            }
            let (src, src_scale) = window_monitor(window, win).unwrap_or((mon, scale));
            let (h, v) = if crate::page::piece_keeps_layout(piece) {
                crate::page::piece_layout_axes(piece)
            } else {
                (Align::Center, Align::Center)
            };
            placement::keep_layout_rect(win, src, src_scale, mon, scale, h, v)
        }
        _ => piece_anchor_rect(piece, mon, scale, None),
    }
}

/// Move (and when asked, resize) a window to a physical rect.
fn apply_rect(window: &WebviewWindow, next: Rect, resize: bool) {
    // Position first: crossing a DPI boundary rescales the window.
    let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
        x: next.x,
        y: next.y,
    }));
    if resize {
        let _ = window.set_size(tauri::Size::Physical(PhysicalSize {
            width: next.width.max(1) as u32,
            height: next.height.max(1) as u32,
        }));
    }
}

/// The piece's own box inside a monitor-sized (`page` dim) window, logical px.
#[derive(Debug, Clone, Copy, Serialize)]
struct StageRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn piece_stage(piece: &crate::page::PagePiece, mon: Rect, scale: f64) -> StageRect {
    let r = piece_anchor_rect(piece, mon, scale, None);
    let s = if scale > 0.0 { scale } else { 1.0 };
    StageRect {
        x: ((r.x - mon.x) as f64 / s).round(),
        y: ((r.y - mon.y) as f64 / s).round(),
        width: piece.width as f64,
        height: piece.height as f64,
    }
}

/// Script handing a `page`-dim overlay its box: `window.__VERSAILLES_STAGE__`,
/// `--piece-x/y/w/h` on `<html>`, and a `versailles:stage` window event.
/// Variables only — the page decides what to do with them. Safe to run as an
/// initialization script: before `<html>` exists it waits for it, so the
/// first paint already has the values.
fn stage_script(stage: Option<StageRect>) -> String {
    let json = serde_json::to_string(&stage).unwrap_or_else(|_| "null".into());
    format!(
        r#"(function(s){{try{{window.__VERSAILLES_STAGE__=s;var k={{'--piece-x':'x','--piece-y':'y','--piece-w':'width','--piece-h':'height'}};function apply(){{var d=document.documentElement;if(!d)return false;Object.keys(k).forEach(function(n){{if(s)d.style.setProperty(n,s[k[n]]+'px');else d.style.removeProperty(n);}});return true;}}if(!apply()){{var mo=new MutationObserver(function(){{if(apply())mo.disconnect();}});mo.observe(document,{{childList:true}});}}window.dispatchEvent(new CustomEvent('versailles:stage',{{detail:s}}));}}catch(e){{}}}})({json});"#
    )
}

fn publish_stage(window: &WebviewWindow, stage: Option<StageRect>) {
    let _ = window.eval(&stage_script(stage));
}

/// Longest a cover-monitor overlay stays hidden on first open while its page
/// lays out its chrome. Older pages that never mark `html[data-stage]` show
/// after this.
const STAGE_WAIT_MS: u64 = 1500;

/// Emitted by [`stage_wait_script`] once the page marked `html[data-stage]`.
const STAGED_EVENT: &str = "overlay://staged";

/// Initialization script for a window held by [`show_when_staged`]: emits
/// [`STAGED_EVENT`] with `{ id }` once `<html>` carries `data-stage` (the
/// example bar sets it when its chrome box is in place). Adds no styles.
fn stage_wait_script(id: &str) -> String {
    let id_js = serde_json::to_string(id).unwrap_or_else(|_| "null".into());
    format!(
        r#"(function(id){{try{{if(window.__VERSAILLES_STAGE_WAIT__)return;window.__VERSAILLES_STAGE_WAIT__=true;function staged(){{var d=document.documentElement;if(!d||!d.hasAttribute('data-stage'))return false;var t=window.__TAURI_INTERNALS__;if(t&&typeof t.invoke==='function')Promise.resolve(t.invoke('plugin:event|emit',{{event:'{STAGED_EVENT}',payload:{{id:id}}}})).catch(function(){{}});return true;}}function watch(){{var d=document.documentElement;if(!d)return false;if(staged())return true;var mo=new MutationObserver(function(){{if(staged())mo.disconnect();}});mo.observe(d,{{attributes:true,attributeFilter:['data-stage']}});return true;}}if(!watch()){{var w=new MutationObserver(function(){{if(watch())w.disconnect();}});w.observe(document,{{childList:true}});}}}}catch(e){{}}}})({id_js});"#
    )
}

/// Whether a [`STAGED_EVENT`] payload names piece `id`.
fn staged_payload_is_for(payload: &str, id: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| v.get("id").and_then(|s| s.as_str()).map(str::to_string))
        .is_some_and(|got| got.eq_ignore_ascii_case(id))
}

/// Show a freshly built cover-monitor overlay once its page has laid out its
/// chrome, or after [`STAGE_WAIT_MS`]. A reveal / conceal in between (a newer
/// overlay generation) takes over, so a dismissed overlay does not pop up.
fn show_when_staged(window: WebviewWindow, id: &str, pos: Position) {
    use std::sync::Arc;
    let app = window.app_handle().clone();
    let gen = overlay_generation(id);
    let done = Arc::new(AtomicBool::new(false));
    let show = {
        let id = id.to_string();
        let done = done.clone();
        Arc::new(move |window: WebviewWindow| {
            if done.swap(true, Ordering::SeqCst) || overlay_generation(&id) != gen {
                return;
            }
            defer_raise_widget(window, Some(pos.clone()));
        })
    };
    let listener = {
        let show = show.clone();
        let window = window.clone();
        let id = id.to_string();
        app.listen_any(STAGED_EVENT, move |event| {
            if staged_payload_is_for(event.payload(), &id) {
                show(window.clone());
            }
        })
    };
    std::thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(STAGE_WAIT_MS) {
            if done.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        show(window);
        app.unlisten(listener);
    });
}

/// Scripts for a window about to be created, added by `open_widget_window`
/// to its initialization script so they run before any page script (keyed by
/// lower-case piece id). Used for the `page`-dim stage and spawn data.
fn pending_init_scripts() -> &'static Mutex<HashMap<String, Vec<String>>> {
    static MAP: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn queue_init_script(id: &str, script: String) {
    pending_init_scripts()
        .lock()
        .unwrap()
        .entry(id.to_ascii_lowercase())
        .or_default()
        .push(script);
}

/// Drop scripts that no window creation picked up (the window already existed).
pub fn clear_init_scripts(id: &str) {
    pending_init_scripts().lock().unwrap().remove(&id.to_ascii_lowercase());
}

/// Dim the monitor `mon` (physical rect) behind an overlay.
fn show_overlay_dim(app: &AppHandle, mon: Rect) {
    let _ = ensure_launcher_dim(app);
    if let Some(dim) = app.get_webview_window(LAUNCHER_DIM_LABEL) {
        let _ = dim.set_position(tauri::Position::Physical(PhysicalPosition { x: mon.x, y: mon.y }));
        let _ = dim.set_size(tauri::Size::Physical(PhysicalSize {
            width: mon.width.max(1) as u32,
            height: mon.height.max(1) as u32,
        }));
        let _ = dim.show();
    }
}

/// Monitor (physical rect + scale) a piece opens on: its `data-monitor`
/// (`primary` or a display number) when connected, else the cursor's.
fn piece_monitor(app: &AppHandle, piece: &crate::page::PagePiece) -> Option<(Rect, f64)> {
    let pinned = crate::monitors::parse_sel(piece.monitor.as_deref()).and_then(|sel| {
        let monitors = crate::monitors::list(app);
        crate::monitors::select(&monitors, sel).map(|m| (m.rect, m.scale))
    });
    pinned.or_else(|| cursor_monitor(app))
}

fn hide_overlay_dim(app: &AppHandle) {
    if let Some(dim) = app.get_webview_window(LAUNCHER_DIM_LABEL) {
        let _ = dim.hide();
    }
}

fn overlay_spawn_ids(app: &AppHandle) -> Vec<String> {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    cat.pieces
        .iter()
        .filter(|p| {
            p.kind == crate::page::PageKind::Spawnable && crate::page::piece_is_overlay(p)
        })
        .map(|p| p.id.clone())
        .collect()
}

/// `overlay://shown` payload. Every webview receives it; listeners filter on
/// `id`. `launcher://shown` keeps the bare seed string for older bar.js copies.
#[derive(Debug, Clone, Serialize)]
struct OverlayShown {
    id: String,
    seed: String,
    /// Only for `page` dim: the piece's box inside its monitor-sized window.
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<StageRect>,
}

/// `overlay://hidden` payload. `id` is `null` when only the backdrop was hidden
/// (no overlay was visible). `launcher://hidden` keeps sending `true`.
#[derive(Debug, Clone, Serialize)]
struct OverlayHidden {
    id: Option<String>,
}

fn emit_overlay_shown(app: &AppHandle, id: &str, stage: Option<StageRect>) {
    let payload = OverlayShown {
        id: id.to_string(),
        seed: take_launcher_seed().unwrap_or_default(),
        stage,
    };
    let _ = app.emit("overlay://shown", &payload);
    let _ = app.emit("launcher://shown", &payload.seed);
    let _ = app.emit("overlay://id", id);
}

fn emit_overlay_hidden(app: &AppHandle, id: Option<&str>) {
    let payload = OverlayHidden {
        id: id.map(str::to_string),
    };
    let _ = app.emit("overlay://hidden", &payload);
    let _ = app.emit("launcher://hidden", true);
}

pub fn reveal_overlay(
    app: &AppHandle,
    manager: &Mutex<WindowManager>,
    id: &str,
) -> AppResult<()> {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let piece = cat
        .spawnable(id)
        .ok_or_else(|| crate::page::unknown_spawn(id))?;
    let overlay = crate::page::piece_is_overlay(piece);
    let label = widget_label(id);
    arm_overlay_focus_guard();
    // Cancels a hide still waiting out its grace period.
    bump_overlay_generation(&piece.id);

    if crate::docks::is_dock(app, &piece.id) {
        // A docked bar is either up already or comes back on.
        if app.get_webview_window(&label).is_none() {
            crate::docks::toggle(app, &piece.id)?;
        }
        return Ok(());
    }

    if !overlay {
        // Hotkey slide-out: plain docked open / raise.
        let pos = dock_piece_slideout(app, piece);
        if let Some(window) = app.get_webview_window(&label) {
            suppress_snap_for(id, 1200);
            let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
                x: pos.x,
                y: pos.y,
            }));
            let _ = window.set_always_on_top(true);
            force_borderless(&window);
            defer_raise_widget(window.clone(), Some(pos));
            return Ok(());
        }
        open_widget_window(app, manager, id, Some(pos), Some(true))?;
        return Ok(());
    }

    let user = state
        .store
        .lock()
        .unwrap()
        .load_user_config()
        .unwrap_or_default();
    let dim = crate::page::piece_dim(piece, &user);
    let covers = crate::page::piece_covers_monitor(piece, &user);
    let Some((mon, scale)) = piece_monitor(app, piece) else {
        return Err(AppError::msg("no monitor to show the overlay on"));
    };
    let stage = (dim == DimMode::Page).then(|| piece_stage(piece, mon, scale));

    let existing = app.get_webview_window(&label);
    let next = overlay_rect(existing.as_ref(), piece, covers, mon, scale);
    let pos = Position {
        x: next.x,
        y: next.y,
    };

    if let Some(window) = existing {
        suppress_snap_for(id, 1200);
        // Kept layouts keep their size (Windows rescales across DPI); a
        // monitor-covering result or a centered overlay is sized.
        let resize = covers
            || !(crate::page::piece_keeps_layout(piece) || page_owns_layout(id))
            || next == mon;
        let resize = resize
            && window_rect(&window)
                .map_or(true, |r| r.width != next.width || r.height != next.height);
        apply_rect(&window, next, resize);
        sync_overlay_dim(app, mon, dim);
        clip_round_window(&window);
        let _ = window.set_always_on_top(true);
        force_borderless(&window);
        defer_raise_widget(window.clone(), Some(pos));
        publish_stage(&window, stage);
        emit_overlay_shown(app, id, stage);
        crate::push::to_piece(app, &piece.id, "shown", &serde_json::Value::Null);
        return Ok(());
    }

    if let Some(stage) = stage {
        queue_init_script(&piece.id, stage_script(Some(stage)));
    }
    // First open: a monitor-covering window is built at the monitor's rect
    // while hidden, so its first frame is already right. A `coverMonitor`
    // page also lays out its chrome before the window shows.
    let wait_for_stage = crate::page::piece_cover_monitor(piece, &user);
    if wait_for_stage {
        queue_init_script(&piece.id, stage_wait_script(&piece.id));
    }
    let first = FirstFrame {
        rect: covers.then_some((next, scale)),
        wait_for_stage,
    };
    let opened = open_widget_window_as(app, manager, id, Some(pos), Some(true), None, first);
    clear_init_scripts(&piece.id);
    opened?;
    if let Some(window) = app.get_webview_window(&label) {
        sync_overlay_dim(app, mon, dim);
        clip_round_window(&window);
    }
    emit_overlay_shown(app, id, stage);
    crate::push::to_piece(app, &piece.id, "shown", &serde_json::Value::Null);
    Ok(())
}

/// Ids whose window the page sized or anchored through the `layout` hook.
/// Their layout survives reveals even for `c` overlays.
fn page_layouts() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

fn page_owns_layout(id: &str) -> bool {
    page_layouts()
        .lock()
        .unwrap()
        .contains(&id.to_ascii_lowercase())
}

/// One `layout` hook call. Every field is optional; all apply to the piece's
/// own window. Sizes and offsets are logical px.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPatch {
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// Grid anchor (`tl` … `br`, `c`, `cursor`) on the window's current monitor.
    pub anchor: Option<String>,
    /// Inset from `anchor` (needs `anchor`).
    pub offset: Option<(i32, i32)>,
    pub always_on_top: Option<bool>,
    /// Absent: unchanged. `null`: clicks hit the whole window again. A list:
    /// only those rects (logical px, window-relative) take clicks; the rest
    /// of the window passes them through to whatever is below.
    #[serde(default, deserialize_with = "some_or_null")]
    pub hit_regions: Option<Option<Vec<HitRect>>>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl HitRect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// A present JSON field (even `null`) becomes `Some(..)`, so `null` and
/// "absent" differ.
fn some_or_null<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

/// Apply a `layout` hook patch to piece `id`'s own window `label`
/// (`widget-<id>`, or the calling monitor copy of a bar).
pub fn apply_layout_patch(app: &AppHandle, id: &str, label: &str, patch: LayoutPatch) -> AppResult<()> {
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| AppError::msg(format!("layout: '{id}' has no window of its own")))?;
    if patch.offset.is_some() && patch.anchor.is_none() {
        return Err(AppError::msg("layout: offset needs an anchor"));
    }
    let anchor = match patch.anchor.as_deref() {
        Some(raw) => Some(placement::parse_anchor(Some(raw)).ok_or_else(|| {
            AppError::msg(format!("layout: '{raw}' is not an overlay anchor"))
        })?),
        None => None,
    };
    let mut owned = false;
    suppress_snap_for(id, 1200);

    if patch.width.is_some() || patch.height.is_some() {
        let scale = window.scale_factor().unwrap_or(1.0);
        let current = window
            .outer_size()
            .map(|s| s.to_logical::<f64>(scale))
            .map_err(|e| AppError::msg(e.to_string()))?;
        let clamp = |v: f64| v.clamp(1.0, 16384.0);
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: clamp(patch.width.unwrap_or(current.width)),
            height: clamp(patch.height.unwrap_or(current.height)),
        }));
        owned = true;
    }

    if let Some(anchor) = anchor {
        let win = window_rect(&window).ok_or_else(|| AppError::msg("layout: window has no rect"))?;
        let (mon, scale) = window_monitor(&window, win)
            .or_else(|| cursor_monitor(app))
            .ok_or_else(|| AppError::msg("layout: no monitor"))?;
        let (ox, oy) = patch.offset.unwrap_or((0, 0));
        let px = |n: i32| ((n as f64) * scale).round() as i32;
        let next = placement::anchor_rect(
            anchor,
            mon,
            win.width,
            win.height,
            (px(ox), px(oy)),
            crate::hotkeys::launcher_anchor(),
        );
        apply_rect(&window, next, false);
        owned = true;
    }

    if let Some(on) = patch.always_on_top {
        let _ = window.set_always_on_top(on);
    }

    if let Some(regions) = patch.hit_regions {
        set_hit_regions(&window, regions);
    }

    if owned {
        page_layouts().lock().unwrap().insert(id.to_ascii_lowercase());
    }
    Ok(())
}

/// Window label → its click-through hit regions (logical, window-relative).
fn hit_regions() -> &'static Mutex<HashMap<String, Vec<HitRect>>> {
    static MAP: OnceLock<Mutex<HashMap<String, Vec<HitRect>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Whether a cursor at physical `(cx, cy)` lands in one of `regions` of a
/// window at physical `win` with `scale`.
fn cursor_hits(regions: &[HitRect], win: Rect, scale: f64, cx: i32, cy: i32) -> bool {
    let s = if scale > 0.0 { scale } else { 1.0 };
    let x = (cx - win.x) as f64 / s;
    let y = (cy - win.y) as f64 / s;
    regions.iter().any(|r| r.contains(x, y))
}

/// Make a window click-through outside `regions` (`None` turns it off).
/// Windows has no per-pixel hit testing for a WebView2 window, so a watcher
/// polls the cursor and flips the window's ignore-cursor flag as it crosses a
/// region edge. While ignored, clicks reach the window below.
fn set_hit_regions(window: &WebviewWindow, regions: Option<Vec<HitRect>>) {
    let label = window.label().to_string();
    let Some(regions) = regions else {
        hit_regions().lock().unwrap().remove(&label);
        let _ = window.set_ignore_cursor_events(false);
        return;
    };
    let fresh = hit_regions()
        .lock()
        .unwrap()
        .insert(label.clone(), regions)
        .is_none();
    if !fresh {
        return; // A watcher is already running for this window.
    }
    let window = window.clone();
    std::thread::spawn(move || {
        let mut ignoring: Option<bool> = None;
        loop {
            let Some(regions) = hit_regions().lock().unwrap().get(&label).cloned() else {
                break;
            };
            if !window.is_visible().unwrap_or(false) {
                std::thread::sleep(Duration::from_millis(120));
                continue;
            }
            let hit = match (window_rect(&window), crate::media::get_mouse_position()) {
                (Some(win), Ok((cx, cy))) => {
                    let scale = window.scale_factor().unwrap_or(1.0);
                    cursor_hits(&regions, win, scale, cx, cy)
                }
                _ => true,
            };
            let ignore = !hit;
            if ignoring != Some(ignore) {
                if window.set_ignore_cursor_events(ignore).is_err() {
                    break; // Window is gone.
                }
                ignoring = Some(ignore);
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        let _ = window.set_ignore_cursor_events(false);
    });
}

/// Show the host dim only for `host` mode; `page` / `none` draw no backdrop.
fn sync_overlay_dim(app: &AppHandle, mon: Rect, dim: DimMode) {
    if dim == DimMode::Host {
        show_overlay_dim(app, mon);
    } else {
        hide_overlay_dim(app);
    }
}

/// How long a page gets to paint its closed state before its overlay hides.
/// Windows shows a hidden window's last frame again on the next show, so a
/// page that animates in should paint the animation's start pose first.
const HIDE_GRACE_MS: u64 = 60;

/// Bumped on every reveal / conceal of an overlay, so a delayed hide that a
/// newer reveal overtook does nothing.
fn overlay_generations() -> &'static Mutex<HashMap<String, u64>> {
    static MAP: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bump_overlay_generation(id: &str) -> u64 {
    let mut map = overlay_generations().lock().unwrap();
    let gen = map.entry(id.to_ascii_lowercase()).or_insert(0);
    *gen += 1;
    *gen
}

fn overlay_generation(id: &str) -> u64 {
    overlay_generations()
        .lock()
        .unwrap()
        .get(&id.to_ascii_lowercase())
        .copied()
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize)]
struct OverlayHiding {
    id: String,
}

fn conceal_overlay(app: &AppHandle, id: &str) -> AppResult<()> {
    let app = app.clone();
    let id = id.to_string();
    let gen = bump_overlay_generation(&id);
    // `overlay://hiding` + the `hiding` piece event, then a short grace so the
    // page can paint its closed state (see HIDE_GRACE_MS).
    let _ = app.emit("overlay://hiding", &OverlayHiding { id: id.clone() });
    crate::push::to_piece(&app, &id, "hiding", &serde_json::Value::Null);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(HIDE_GRACE_MS));
        if overlay_generation(&id) != gen {
            return; // Shown again meanwhile.
        }
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if overlay_generation(&id) != gen {
                return;
            }
            hide_overlay_dim(&handle);
            if let Some(window) = handle.get_webview_window(&widget_label(&id)) {
                let _ = window.hide();
            }
            emit_overlay_hidden(&handle, Some(&id));
            crate::push::to_piece(&handle, &id, "hidden", &serde_json::Value::Null);
        });
    });
    Ok(())
}

/// Toggle the spawnable bound to a `data-hotkey`. Deferred off the shortcut thread
/// so WebView2 is not created there (that hangs Windows).
pub fn toggle_hotkey_piece(app: &AppHandle, id: &str) {
    let app = app.clone();
    let id = id.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let state = handle.state::<AppState>();
            if let Err(err) = toggle_slideout_widget(&handle, &state.window_manager, &id) {
                tracing::warn!("hotkey spawn '{id}' failed: {err}");
            }
        });
    });
}

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
    // Never create WebView2 on the hotkey/IPC thread — that hangs Windows.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = ensure_launcher_dim(&handle);
            let Some(id) = hotkey_piece_id(&handle) else {
                tracing::warn!("no spawnable declared data-hooks=hotkey; overlay is idle");
                return;
            };
            let state = handle.state::<AppState>();
            if let Err(err) = reveal_overlay(&handle, &state.window_manager, &id) {
                tracing::warn!("overlay '{id}' failed to show: {err}");
            }
        });
    });
    Ok(())
}

/// Hide dim + overlay spawnables off the calling invoke/hotkey thread.
/// Sync hide from a webview invoke re-enters WebView2 and can Application Hang.
pub fn hide_launcher(app: &AppHandle) -> AppResult<()> {
    let overlay_ids = overlay_spawn_ids(app);
    let mut hid_any = false;
    for id in overlay_ids {
        let visible = app
            .get_webview_window(&widget_label(&id))
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        if visible {
            hid_any = true;
            let _ = conceal_overlay(app, &id);
        }
    }
    if !hid_any {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                hide_overlay_dim(&handle);
                emit_overlay_hidden(&handle, None);
            });
        });
    }
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

/// Labels of the wallpaper windows that exist (`desktop`, `desktop-<n>`).
pub fn desktop_window_labels(app: &AppHandle) -> Vec<String> {
    let mut labels: Vec<String> = app
        .webview_windows()
        .into_keys()
        .filter(|l| crate::page::is_desktop_label(l))
        .collect();
    labels.sort();
    labels
}

fn pin_all_desktops(app: &AppHandle) {
    for label in desktop_window_labels(app) {
        if let Some(window) = app.get_webview_window(&label) {
            pin_desktop_above_shell(&window);
        }
    }
}

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
        let delays = [80u64, 250, 800, 2000, 5000].into_iter().chain(std::iter::repeat(4000));
        for delay_ms in delays {
            std::thread::sleep(Duration::from_millis(delay_ms));
            if desktop_window_labels(&app).is_empty() {
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || pin_all_desktops(&handle));
        }
    });
}

/// Fill `work` (a monitor's work area, physical) — never cover the native
/// taskbar. No exclusive fullscreen (hides the bar / can hang WebView2). No
/// Tauri always_on_bottom (vanishes behind the wallpaper).
fn place_desktop_window(window: &WebviewWindow, work: Rect) {
    let _ = window.set_always_on_bottom(false);
    let _ = window.set_position(tauri::Position::Physical(PhysicalPosition { x: work.x, y: work.y }));
    let _ = window.set_size(tauri::Size::Physical(PhysicalSize {
        width: work.width.max(1) as u32,
        height: work.height.max(1) as u32,
    }));
    pin_desktop_above_shell(window);
}

fn ensure_desktop_window_on(app: &AppHandle, label: &str, work: Option<Rect>) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(label) {
        match work {
            Some(work) => place_desktop_window(&window, work),
            None => {
                let (x, y, w, h) = desktop_work_area(&window);
                place_desktop_window(&window, Rect { x, y, width: w as i32, height: h as i32 });
            }
        }
        let _ = window.show();
        pin_desktop_above_shell(&window);
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("desktop.html".into()))
        .title("Versailles Desktop")
        .inner_size(1280.0, 720.0)
        .decorations(false)
        .transparent(false)
        .skip_taskbar(true)
        .visible(false)
        .shadow(false)
        .initialization_script(&crate::gesture::init_script())
        .build()?;
    let _ = window.set_skip_taskbar(true);
    let pin_win = window.clone();
    let _ = window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) | tauri::WindowEvent::Moved(_) = event {
            pin_desktop_above_shell(&pin_win);
        }
    });
    match work {
        Some(work) => place_desktop_window(&window, work),
        None => {
            let (x, y, w, h) = desktop_work_area(&window);
            place_desktop_window(&window, Rect { x, y, width: w as i32, height: h as i32 });
        }
    }
    window.show()?;
    pin_desktop_above_shell(&window);
    Ok(())
}

/// One wallpaper window per monitor that should have one: the primary
/// (`desktop`) plus every monitor a widget selects with `data-monitor`
/// (`desktop-<n>`). Windows for monitors no longer wanted are closed.
pub fn ensure_desktop_window(app: &AppHandle) -> AppResult<()> {
    let monitors = crate::monitors::list(app);
    if monitors.is_empty() {
        // Monitors unknown this early: the single wallpaper on its nearest monitor.
        ensure_desktop_window_on(app, crate::page::DESKTOP_LABEL, None)?;
    } else {
        let cat = {
            let state = app.state::<AppState>();
            crate::desktop::page_catalog(&state)
        };
        let selections = crate::monitors::widget_selections(&cat);
        let wanted = crate::monitors::wallpaper_numbers(&monitors, &selections);
        tracing::info!(
            "wallpapers on displays {wanted:?} (monitors {:?}, data-monitor {selections:?})",
            monitors.iter().map(|m| (m.number, m.primary)).collect::<Vec<_>>()
        );
        let mut keep = Vec::new();
        for m in monitors.iter().filter(|m| wanted.contains(&m.number)) {
            let label = crate::monitors::wallpaper_label(m);
            ensure_desktop_window_on(app, &label, Some(m.work))?;
            keep.push(label);
        }
        for label in desktop_window_labels(app) {
            if !keep.contains(&label) {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.close();
                }
            }
        }
    }
    start_desktop_zorder_keeper(app);
    crate::boot::mark_desktop_window();
    Ok(())
}

pub fn close_desktop_window(app: &AppHandle) -> AppResult<()> {
    for label in desktop_window_labels(app) {
        if let Some(window) = app.get_webview_window(&label) {
            window.close()?;
        }
    }
    Ok(())
}

/// A work area moved (a bar or the taskbar reserved space): re-fit the
/// wallpapers to their monitors' work areas.
pub fn refit_wallpapers(app: &AppHandle) {
    if !desktop_window_labels(app).is_empty() {
        if let Err(err) = ensure_desktop_window(app) {
            tracing::warn!("wallpapers not re-fitted: {err}");
        }
    }
}

/// Monitors were added, removed, resized or rescaled: re-place the
/// wallpapers and bring any window that ended up off-screen back on.
pub fn on_monitors_changed(app: &AppHandle) {
    if !desktop_window_labels(app).is_empty() {
        if let Err(err) = ensure_desktop_window(app) {
            tracing::warn!("wallpapers not re-placed: {err}");
        }
    }
    let monitors = collect_monitor_rects(app);
    for (label, window) in app.webview_windows() {
        if !label.starts_with("widget-") || !window.is_visible().unwrap_or(false) {
            continue;
        }
        let Some(r) = window_rect(&window) else {
            continue;
        };
        let on_screen = monitors.iter().any(|m| {
            r.x + r.width > m.x && r.x < m.x + m.width && r.y + r.height > m.y && r.y < m.y + m.height
        });
        if !on_screen {
            let next = clamp_position_to_monitors(app, r.x, r.y, r.width as u32, r.height as u32);
            let _ = window.set_position(tauri::Position::Physical(PhysicalPosition { x: next.x, y: next.y }));
        }
    }
    let _ = app.emit("desktop://layout", true);
    crate::docks::reconcile(app);
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
    match cursor_monitor(app) {
        Some((mon, scale)) => dock_slideout_on(mon, scale, width, height),
        None => Position { x: 40, y: 80 },
    }
}

/// Docked slide-out spot for a piece: on its `data-monitor`, else the cursor's.
fn dock_piece_slideout(app: &AppHandle, piece: &crate::page::PagePiece) -> Position {
    match piece_monitor(app, piece) {
        Some((mon, scale)) => dock_slideout_on(mon, scale, piece.width, piece.height),
        None => Position { x: 40, y: 80 },
    }
}

fn dock_slideout_on(mon: Rect, scale: f64, width: u32, height: u32) -> Position {
    let (left, top, mon_w, mon_h) = (mon.x, mon.y, mon.width as u32, mon.height as u32);
    // Monitor bounds are physical; piece width/height are logical (CSS px).
    let width = (width as f64 * scale).round() as i32;
    let height = (height as f64 * scale).round() as i32;
    // Keep slide-outs below the native top taskbar/hud area.
    let hud = (56.0 * scale).round() as i32;
    let gap = (14.0 * scale).round() as i32;
    let x = left + mon_w as i32 - width - gap;
    let y = top + hud + gap;
    let max_y = top + mon_h as i32 - height - gap;
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
    let cat = {
        let state = app.state::<AppState>();
        crate::desktop::page_catalog(&state)
    };
    let resolved = cat
        .spawnable(key)
        .map(|p| p.id.clone())
        .unwrap_or_else(|| key.to_string());

    if crate::docks::is_dock(app, &resolved) {
        return crate::docks::toggle(app, &resolved);
    }

    if cat.is_overlay(&resolved) {
        let label = widget_label(&resolved);
        let visible = app
            .get_webview_window(&label)
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        tracing::info!("toggle overlay '{resolved}': {}", if visible { "hide" } else { "show" });
        if visible {
            conceal_overlay(app, &resolved)?;
            return Ok(false);
        }
        reveal_overlay(app, manager, &resolved)?;
        return Ok(true);
    }

    let label = widget_label(&resolved);
    if app.get_webview_window(&label).is_some() {
        close_widget_window(app, manager, &resolved)?;
        return Ok(false);
    }
    let pos = if let Some(piece) = cat.spawnable(&resolved) {
        dock_piece_slideout(app, piece)
    } else {
        let state = app.state::<AppState>();
        let registry = state.registry.lock().unwrap();
        let widget = registry
            .get(&resolved)
            .ok_or_else(|| crate::page::unknown_spawn(&resolved))?;
        dock_slideout_position(app, widget.manifest.width, widget.manifest.height)
    };
    open_widget_window(app, manager, &resolved, Some(pos), Some(true))?;
    Ok(true)
}

pub fn collect_monitor_rects(app: &AppHandle) -> Vec<Rect> {
    let Some(window) = app.webview_windows().into_values().next() else {
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
    crate::monitors::list(app)
        .into_iter()
        .map(|m| MonitorFingerprint {
            width: m.rect.width.max(0) as u32,
            height: m.rect.height.max(0) as u32,
            scale_factor: m.scale,
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
    let cat = {
        let state = app.state::<AppState>();
        crate::desktop::page_catalog(&state)
    };
    let open_ids: Vec<String> = manager.lock().unwrap().open.keys().cloned().collect();
    for id in open_ids {
        if cat.is_desktop_widget(&id) || !layout.widgets.contains_key(&id) {
            close_widget_window(app, manager, &id)?;
        }
    }

    let mut opened = Vec::new();
    for (id, placement) in &layout.widgets {
        if cat.is_desktop_widget(id) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_windows_carry_their_monitor() {
        let url = url::Url::parse("http://127.0.0.1:47831/files/desktop/index.html?m=spawn&id=taskbar").unwrap();
        let on2 = with_monitor_params(&url, 2, false);
        assert_eq!(on2.query(), Some("m=spawn&id=taskbar&monitor=2&primary=0"));
        // Re-targeting replaces the old values instead of adding more.
        let on1 = with_monitor_params(&on2, 1, true);
        assert_eq!(on1.query(), Some("m=spawn&id=taskbar&monitor=1&primary=1"));
        // Tauri labels allow `a-zA-Z-/:_` (no `@`), so copies use `:`.
        let copy = monitor_copy_label("taskbar", 3);
        assert_eq!(copy, "widget-taskbar:3");
        assert!(copy.chars().all(|c| c.is_alphanumeric() || "-/:_".contains(c)));
    }

    #[test]
    fn layout_patch_tells_null_from_absent() {
        let p: LayoutPatch = serde_json::from_str(r#"{ "width": 300 }"#).unwrap();
        assert_eq!(p.hit_regions, None);
        let p: LayoutPatch = serde_json::from_str(r#"{ "hitRegions": null }"#).unwrap();
        assert_eq!(p.hit_regions, Some(None));
        let p: LayoutPatch = serde_json::from_str(
            r#"{ "hitRegions": [{ "x": 0, "y": 0, "width": 10, "height": 5 }] }"#,
        )
        .unwrap();
        assert_eq!(p.hit_regions.unwrap().unwrap().len(), 1);
    }

    #[test]
    fn overlay_generation_lets_a_newer_reveal_cancel_a_pending_hide() {
        let id = "gen-test-piece";
        let hide = bump_overlay_generation(id);
        assert_eq!(overlay_generation(id), hide);
        // A reveal before the grace period ends bumps it again…
        bump_overlay_generation("GEN-TEST-PIECE");
        // …so the pending hide sees a newer generation and skips.
        assert_ne!(overlay_generation(id), hide);
    }

    #[test]
    fn cursor_hits_in_logical_window_space() {
        let regions = [HitRect { x: 10.0, y: 10.0, width: 100.0, height: 40.0 }];
        let win = Rect { x: 1000, y: 500, width: 400, height: 200 };
        // 2x DPI: logical (10,10) is physical (1020,520).
        assert!(cursor_hits(&regions, win, 2.0, 1020, 520));
        assert!(cursor_hits(&regions, win, 2.0, 1219, 599));
        assert!(!cursor_hits(&regions, win, 2.0, 1019, 520));
        assert!(!cursor_hits(&regions, win, 2.0, 1220, 520));
        assert!(!cursor_hits(&[], win, 1.0, 1100, 550));
    }

    #[test]
    fn staged_event_names_its_piece() {
        assert!(staged_payload_is_for(r#"{"id":"action-bar"}"#, "action-bar"));
        assert!(staged_payload_is_for(r#"{"id":"Action-Bar"}"#, "action-bar"));
        assert!(!staged_payload_is_for(r#"{"id":"draw"}"#, "action-bar"));
        assert!(!staged_payload_is_for(r#"{}"#, "action-bar"));
        assert!(!staged_payload_is_for("null", "action-bar"));
        assert!(!staged_payload_is_for("not json", "action-bar"));
    }

    #[test]
    fn stage_wait_script_emits_the_staged_event_for_its_piece() {
        let js = stage_wait_script("action-bar");
        assert!(js.contains("'overlay://staged'"));
        assert!(js.contains(r#"("action-bar")"#));
        assert!(js.contains("data-stage"));
        // Values only: the host adds no styles to the page.
        assert!(!js.contains("style"));
        // An id cannot break out of the string literal.
        let js = stage_wait_script("x\");alert(1);//");
        assert!(js.contains(r#"("x\");alert(1);//")"#));
    }

    #[test]
    fn chrome_script_binds_once_across_the_re_eval() {
        let js = page_surface_chrome_script("draw", false, true);
        assert!(js.contains("__VERSAILLES_CHROME_BINDING__"));
    }
}
