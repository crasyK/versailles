//! Docked surfaces: spawnables with `data-anchor="top|bottom|left|right"`.
//! They stay on screen (no dim, no blur dismiss) and open by themselves when
//! the page declares them.
//!
//! `data-dock`:
//! - `reserve` (default): a Windows AppBar, like the taskbar. Maximized apps
//!   stop at the bar's edge.
//! - `float`: always on top inside the work area, no space reserved.
//! - `auto-hide`: a `data-peek` px strip (default 4) at the edge; the bar
//!   slides out while the cursor is at that edge or on the bar.
//!
//! `data-monitor="all"` opens one window per monitor: `widget-<id>` on the
//! primary, `widget-<id>:<n>` on display n. Every copy acts as piece `<id>`
//! and loads with `monitor=<n>`; monitor changes add and remove copies.

use crate::error::AppResult;
use crate::layout::Rect;
use crate::monitors::MonitorInfo;
use crate::page::PagePiece;
use crate::state::AppState;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockMode {
    Reserve,
    Float,
    AutoHide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dock {
    pub edge: Edge,
    pub mode: DockMode,
    /// Logical px still showing while an auto-hide bar is tucked away.
    pub peek: u32,
}

pub fn parse_edge(anchor: Option<&str>) -> Option<Edge> {
    match anchor?.trim().to_ascii_lowercase().as_str() {
        "top" => Some(Edge::Top),
        "bottom" => Some(Edge::Bottom),
        "left" => Some(Edge::Left),
        "right" => Some(Edge::Right),
        _ => None,
    }
}

pub fn parse_mode(raw: Option<&str>) -> DockMode {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("float") => DockMode::Float,
        Some("auto-hide") | Some("autohide") => DockMode::AutoHide,
        _ => DockMode::Reserve,
    }
}

pub fn piece_dock(piece: &PagePiece) -> Option<Dock> {
    let edge = parse_edge(piece.anchor.as_deref())?;
    Some(Dock {
        edge,
        mode: parse_mode(piece.dock.as_deref()),
        peek: piece.peek.unwrap_or(4).min(64),
    })
}

/// Logical thickness: `data-h` for top / bottom bars, `data-w` for left / right.
pub fn thickness(piece: &PagePiece, edge: Edge) -> u32 {
    match edge {
        Edge::Top | Edge::Bottom => piece.height,
        Edge::Left | Edge::Right => piece.width,
    }
    .clamp(1, 2000)
}

/// Full-length strip `thick` px deep along `edge` of `area` (physical).
pub fn strip(edge: Edge, area: Rect, thick: i32) -> Rect {
    let thick = thick.clamp(1, area.width.max(area.height));
    match edge {
        Edge::Top => Rect { height: thick.min(area.height), ..area },
        Edge::Bottom => Rect {
            y: area.y + area.height - thick.min(area.height),
            height: thick.min(area.height),
            ..area
        },
        Edge::Left => Rect { width: thick.min(area.width), ..area },
        Edge::Right => Rect {
            x: area.x + area.width - thick.min(area.width),
            width: thick.min(area.width),
            ..area
        },
    }
}

/// Whether `r` is a usable bar strip for `edge` of `monitor`: inside the
/// monitor, `thick` deep, and on that edge's half. Explorer's AppBar answers
/// are not always (a bottom bar on a monitor whose taskbar is at the top came
/// back with its bottom at y = 0).
pub fn usable_strip(edge: Edge, r: Rect, monitor: Rect, thick: i32) -> bool {
    let inside = r.width > 0
        && r.height > 0
        && r.x >= monitor.x
        && r.y >= monitor.y
        && r.x + r.width <= monitor.x + monitor.width
        && r.y + r.height <= monitor.y + monitor.height;
    let mid_x = monitor.x + monitor.width / 2;
    let mid_y = monitor.y + monitor.height / 2;
    let (depth, on_edge) = match edge {
        Edge::Top => (r.height, r.y + r.height <= mid_y),
        Edge::Bottom => (r.height, r.y >= mid_y),
        Edge::Left => (r.width, r.x + r.width <= mid_x),
        Edge::Right => (r.width, r.x >= mid_x),
    };
    inside && on_edge && (depth - thick).abs() <= 1
}

/// Whether the cursor touches the edge zone of an auto-hide bar: within
/// `zone` px of `edge` of `area`, along the bar's length.
pub fn at_edge(edge: Edge, area: Rect, zone: i32, x: i32, y: i32) -> bool {
    let inside_x = x >= area.x && x < area.x + area.width;
    let inside_y = y >= area.y && y < area.y + area.height;
    match edge {
        Edge::Top => inside_x && y >= area.y && y < area.y + zone,
        Edge::Bottom => inside_x && y >= area.y + area.height - zone && y < area.y + area.height,
        Edge::Left => inside_y && x >= area.x && x < area.x + zone,
        Edge::Right => inside_y && x >= area.x + area.width - zone && x < area.x + area.width,
    }
}

fn contains(r: Rect, x: i32, y: i32) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Live state of one docked window.
#[derive(Debug, Clone)]
struct DockState {
    /// Piece id; several windows share it for a bar on every monitor.
    id: String,
    /// Window label: `widget-<id>`, or `widget-<id>:<n>` for a monitor copy.
    label: String,
    dock: Dock,
    /// Display number and primary flag the window was opened for.
    number: u32,
    primary: bool,
    /// Monitor rect (physical) and its work area.
    monitor: Rect,
    work: Rect,
    scale: f64,
    thick: i32,
    /// Auto-hide: currently slid out.
    expanded: bool,
    /// Auto-hide: when the cursor left the bar.
    left_at: Option<Instant>,
    #[cfg(windows)]
    appbar: bool,
    /// Windows granted no usable reservation on this monitor: float without
    /// asking again until the monitor changes (each refused try moved the
    /// work area and set off another round).
    refused: bool,
}

/// Docked windows by label.
fn states() -> &'static Mutex<HashMap<String, DockState>> {
    static MAP: OnceLock<Mutex<HashMap<String, DockState>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn app_slot() -> &'static OnceLock<AppHandle> {
    static APP: OnceLock<AppHandle> = OnceLock::new();
    &APP
}

/// `data-monitor="all"` on a docked bar: one window per monitor.
pub fn on_every_monitor(piece: &PagePiece) -> bool {
    piece_dock(piece).is_some()
        && crate::monitors::parse_sel(piece.monitor.as_deref()) == Some(crate::monitors::MonitorSel::All)
}

/// Window label of bar `id` on monitor `m`: the primary's copy keeps
/// `widget-<id>` (so a single-monitor setup looks like any other bar).
pub fn instance_label(id: &str, m: &MonitorInfo) -> String {
    if m.primary {
        crate::window_manager::widget_label(id)
    } else {
        crate::window_manager::monitor_copy_label(id, m.number)
    }
}

/// Windows a docked piece wants: `(label, monitor)`. Every monitor for
/// `data-monitor="all"`; otherwise its `data-monitor` (primary or a number),
/// else the primary. Docks do not follow the cursor.
pub fn instances(piece: &PagePiece, monitors: &[MonitorInfo]) -> Vec<(String, MonitorInfo)> {
    use crate::monitors::{select, MonitorSel};
    if on_every_monitor(piece) {
        return monitors.iter().map(|m| (instance_label(&piece.id, m), m.clone())).collect();
    }
    let sel = crate::monitors::parse_sel(piece.monitor.as_deref())
        .filter(|s| matches!(s, MonitorSel::Number(_)))
        .unwrap_or(MonitorSel::Primary);
    select(monitors, sel)
        .or_else(|| select(monitors, MonitorSel::Primary))
        .map(|m| vec![(crate::window_manager::widget_label(&piece.id), m.clone())])
        .unwrap_or_default()
}

fn set_rect(window: &WebviewWindow, r: Rect) {
    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x: r.x, y: r.y }));
    let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize {
        width: r.width.max(1) as u32,
        height: r.height.max(1) as u32,
    }));
}

/// Place a dock window for its current state (reserving space if needed).
fn place(window: &WebviewWindow, st: &mut DockState) {
    match st.dock.mode {
        DockMode::Float => set_rect(window, strip(st.dock.edge, st.work, st.thick)),
        DockMode::AutoHide => {
            let px = if st.expanded {
                st.thick
            } else {
                ((st.dock.peek as f64) * st.scale).round().max(1.0) as i32
            };
            set_rect(window, strip(st.dock.edge, st.work, px));
        }
        DockMode::Reserve => {
            #[cfg(windows)]
            {
                // Explorer judges an AppBar by the monitor its window is on:
                // put the window on its edge first.
                set_rect(window, strip(st.dock.edge, st.work, st.thick));
                if st.refused {
                    return;
                }
                if let Some(r) = appbar::set_pos(window, st.dock.edge, st.monitor, st.work, st.thick, !st.appbar) {
                    if !st.appbar {
                        tracing::info!("dock '{}': reserved {:?} on display {}", st.label, r, st.number);
                    }
                    st.appbar = true;
                    set_rect(window, r);
                    return;
                }
                tracing::warn!(
                    "dock '{}': Windows granted no usable reservation on display {}; it floats instead",
                    st.label, st.number
                );
                st.refused = true;
            }
            set_rect(window, strip(st.dock.edge, st.work, st.thick));
        }
    }
}

/// Open, place or close dock windows to match the page and the monitors.
/// Call on the main thread (AppBar registration and window subclassing need
/// the UI thread).
pub fn reconcile(app: &AppHandle) {
    let _ = app_slot().set(app.clone());
    // An update is about to install: reservations stay released.
    if crate::updater::updating() {
        return;
    }
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let monitors = crate::monitors::list(app);
    let hidden = user_hidden().lock().unwrap().clone();
    let wanted: Vec<(&PagePiece, String, MonitorInfo)> = cat
        .pieces
        .iter()
        .filter(|p| piece_dock(p).is_some())
        // Docks the user toggled off stay off until toggled on again.
        .filter(|p| !hidden.contains(&p.id.to_ascii_lowercase()))
        .flat_map(|p| instances(p, &monitors).into_iter().map(move |(label, m)| (p, label, m)))
        .collect();

    // Close windows the page no longer declares, and copies for monitors
    // that are gone.
    let stale: Vec<String> = states()
        .lock()
        .unwrap()
        .keys()
        .filter(|label| !wanted.iter().any(|(_, l, _)| l == *label))
        .cloned()
        .collect();
    for label in stale {
        close(app, &label);
    }

    for (piece, label, mon) in wanted {
        if let Err(err) = open(app, piece, &label, &mon) {
            tracing::warn!("dock '{label}' failed to open: {err}");
        }
    }
    start_autohide_watcher(app);
    #[cfg(windows)]
    start_reserve_watcher(app);
}

/// Whether a reserving bar's strip is kept out of its monitor's work area
/// (`bar` and `work` in physical pixels). A tolerance of one pixel.
fn strip_reserved(edge: Edge, bar: Rect, work: Rect) -> bool {
    match edge {
        Edge::Top => work.y >= bar.y + bar.height - 1,
        Edge::Bottom => work.y + work.height <= bar.y + 1,
        Edge::Left => work.x >= bar.x + bar.width - 1,
        Edge::Right => work.x + work.width <= bar.x + 1,
    }
}

/// How long to wait before registering a bar again after `tries` attempts
/// in a row that did not bring its space back.
fn heal_backoff(tries: u32) -> Duration {
    if tries == 0 {
        return Duration::ZERO;
    }
    Duration::from_secs((8u64 << (tries - 1).min(4)).min(120))
}

/// Keep reservations alive. Explorer can forget an AppBar without telling
/// it: a restarted Explorer drops bars that registered again while it was
/// still starting up. Every 3 s, check that each reserving bar's strip is
/// still kept out of the work area; after two misses in a row, register it
/// again. A try that did not take (Explorer still starting) is repeated
/// after 8 s, then 16, 32… up to 2 min, until the space is back.
#[cfg(windows)]
fn start_reserve_watcher(app: &AppHandle) {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut misses: HashMap<String, u32> = HashMap::new();
        // Per bar: when it was last registered again, and how often in a row.
        let mut healed: HashMap<String, (Instant, u32)> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(3));
            if crate::updater::updating() {
                continue;
            }
            let reserving: Vec<(String, Edge)> = states()
                .lock()
                .unwrap()
                .values()
                .filter(|s| s.dock.mode == DockMode::Reserve && s.appbar && !s.refused)
                .map(|s| (s.label.clone(), s.dock.edge))
                .collect();
            misses.retain(|label, _| reserving.iter().any(|(l, _)| l == label));
            for (label, edge) in reserving {
                let Some(window) = app.get_webview_window(&label) else {
                    continue;
                };
                let Some((bar, work)) = appbar::bar_and_work(&window) else {
                    continue;
                };
                if strip_reserved(edge, bar, work) {
                    misses.remove(&label);
                    healed.remove(&label);
                    continue;
                }
                let n = misses.entry(label.clone()).or_insert(0);
                *n += 1;
                let tries = healed.get(&label).map(|(_, k)| *k).unwrap_or(0);
                let wait = heal_backoff(tries);
                let recent = healed.get(&label).is_some_and(|(t, _)| t.elapsed() < wait);
                if *n < 2 || recent {
                    continue;
                }
                misses.remove(&label);
                healed.insert(label.clone(), (Instant::now(), tries + 1));
                tracing::info!("dock '{label}': its space is no longer reserved; reserving it again");
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    let Some(window) = handle.get_webview_window(&label) else {
                        return;
                    };
                    appbar::forget(&window);
                    if let Some(st) = states().lock().unwrap().get_mut(&label) {
                        place(&window, st);
                    }
                });
            }
        }
    });
}

/// Docks the user toggled off with their hotkey (until toggled on again).
fn user_hidden() -> &'static Mutex<std::collections::HashSet<String>> {
    static SET: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// Open (or re-place) window `label` of dock `piece` on monitor `mon`.
fn open(app: &AppHandle, piece: &PagePiece, label: &str, mon: &MonitorInfo) -> AppResult<()> {
    let dock = piece_dock(piece).expect("caller checked");
    let thick = ((thickness(piece, dock.edge) as f64) * mon.scale).round() as i32;
    let mut st = states().lock().unwrap().remove(label).unwrap_or(DockState {
        id: piece.id.clone(),
        label: label.to_string(),
        dock,
        number: mon.number,
        primary: mon.primary,
        monitor: mon.rect,
        work: mon.work,
        scale: mon.scale,
        thick,
        expanded: false,
        left_at: None,
        #[cfg(windows)]
        appbar: false,
        refused: false,
    });
    // Mode, edge or monitor changed: drop a reservation made for the old one,
    // and ask Windows again even if it refused before.
    if st.dock != dock || st.monitor != mon.rect {
        st.refused = false;
    }
    #[cfg(windows)]
    if st.appbar && (st.dock != dock || dock.mode != DockMode::Reserve || st.monitor != mon.rect) {
        if let Some(w) = app.get_webview_window(label) {
            appbar::remove(&w);
        }
        st.appbar = false;
    }
    let moved = (st.number, st.primary) != (mon.number, mon.primary);
    st.dock = dock;
    st.number = mon.number;
    st.primary = mon.primary;
    st.monitor = mon.rect;
    // While reserving, the monitor's work area already excludes our own
    // strip: keep the one from before, so a fallback strip does not creep.
    if !st.appbar {
        st.work = mon.work;
    }
    st.scale = mon.scale;
    st.thick = thick;

    if app.get_webview_window(label).is_none() {
        tracing::info!("dock '{label}': opening on display {} ({:?})", mon.number, dock.mode);
        crate::window_manager::mark_quiet(label);
        let first = strip(dock.edge, mon.work, thick);
        crate::window_manager::open_dock_window(
            app,
            &piece.id,
            crate::window_manager::DockInstance {
                label: label.to_string(),
                monitor: mon.number,
                primary: mon.primary,
            },
            crate::config::Position { x: first.x, y: first.y },
        )?;
    } else if moved {
        // Same window, other monitor (e.g. the primary changed): the page
        // reloads with its new `monitor` parameter.
        if let Some(window) = app.get_webview_window(label) {
            crate::window_manager::retarget_dock_window(&window, mon.number, mon.primary);
        }
    }
    if let Some(window) = app.get_webview_window(label) {
        #[cfg(windows)]
        appbar::watch(&window);
        // Placed while still hidden; the deferred raise shows it in place
        // (it does not move quiet windows), so it never appears elsewhere.
        place(&window, &mut st);
        let _ = window.set_always_on_top(true);
    }
    states().lock().unwrap().insert(label.to_string(), st);
    // Safety net: place once more after the window is up.
    let app2 = app.clone();
    let label2 = label.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        let handle = app2.clone();
        let _ = app2.run_on_main_thread(move || replace(&handle, &label2));
    });
    start_fullscreen_watcher(app);
    Ok(())
}

/// Re-apply a dock window's placement (keeps its AppBar reservation).
fn replace(app: &AppHandle, label: &str) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    let mut map = states().lock().unwrap();
    if let Some(st) = map.get_mut(label) {
        place(&window, st);
    }
}

/// Fullscreen apps (video, games) ignore the work area. While the active
/// window covers a monitor, that monitor's bars step behind it; they come
/// back on top when it leaves fullscreen or loses focus.
fn start_fullscreen_watcher(app: &AppHandle) {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut behind: Option<(isize, Rect)> = None;
        loop {
            std::thread::sleep(Duration::from_millis(300));
            if states().lock().unwrap().is_empty() {
                continue;
            }
            let now = fullscreen::foreground_fullscreen();
            if now.map(|(h, _)| h) == behind.map(|(h, _)| h) {
                continue;
            }
            behind = now;
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                let labels: Vec<(String, Rect)> = states()
                    .lock()
                    .unwrap()
                    .values()
                    .map(|s| (s.label.clone(), s.monitor))
                    .collect();
                for (label, monitor) in labels {
                    let Some(window) = handle.get_webview_window(&label) else {
                        continue;
                    };
                    match now {
                        Some((fs, fs_monitor)) if fs_monitor == monitor => fullscreen::step_behind(&window, fs),
                        _ => fullscreen::back_on_top(&window),
                    }
                }
            });
        }
    });
}

#[cfg(windows)]
mod fullscreen {
    use super::Rect;
    use tauri::WebviewWindow;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, SetWindowPos,
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    /// The foreground window and its monitor rect, when that window covers
    /// the whole monitor (and is not the desktop or one of ours).
    pub fn foreground_fullscreen() -> Option<(isize, Rect)> {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_invalid() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            if pid == std::process::id() {
                return None;
            }
            let mut class = [0u16; 64];
            let n = GetClassNameW(fg, &mut class).max(0) as usize;
            let class = String::from_utf16_lossy(&class[..n]);
            if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
                return None;
            }
            let mut r = RECT::default();
            GetWindowRect(fg, &mut r).ok()?;
            let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(mon, &mut info).as_bool() {
                return None;
            }
            let m = info.rcMonitor;
            let covers = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
            covers.then(|| {
                (
                    fg.0 as isize,
                    Rect { x: m.left, y: m.top, width: m.right - m.left, height: m.bottom - m.top },
                )
            })
        }
    }

    fn hwnd(window: &WebviewWindow) -> Option<HWND> {
        window.hwnd().ok().map(|h| HWND(h.0))
    }

    pub fn step_behind(window: &WebviewWindow, fullscreen: isize) {
        let Some(h) = hwnd(window) else {
            return;
        };
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        unsafe {
            // Leave the topmost band, then sit directly under the fullscreen window.
            let _ = SetWindowPos(h, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
            let _ = SetWindowPos(h, Some(HWND(fullscreen as *mut _)), 0, 0, 0, 0, flags);
        }
    }

    pub fn back_on_top(window: &WebviewWindow) {
        let Some(h) = hwnd(window) else {
            return;
        };
        unsafe {
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

#[cfg(not(windows))]
mod fullscreen {
    use super::Rect;
    use tauri::WebviewWindow;
    pub fn foreground_fullscreen() -> Option<(isize, Rect)> {
        None
    }
    pub fn step_behind(_window: &WebviewWindow, _fullscreen: isize) {}
    pub fn back_on_top(_window: &WebviewWindow) {}
}

fn close(app: &AppHandle, label: &str) {
    states().lock().unwrap().remove(label);
    if let Some(window) = app.get_webview_window(label) {
        #[cfg(windows)]
        appbar::remove(&window);
        let _ = window.close();
    }
}

pub fn is_dock(app: &AppHandle, id: &str) -> bool {
    let state = app.state::<AppState>();
    crate::desktop::page_catalog(&state)
        .spawnable(id)
        .is_some_and(|p| piece_dock(p).is_some())
}

/// Hotkey / `spawn()`: hide a showing dock (every monitor's window, releasing
/// their space) or bring a hidden one back. Returns whether it is now shown.
pub fn toggle(app: &AppHandle, id: &str) -> AppResult<bool> {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let Some(piece) = cat.spawnable(id) else {
        return Ok(false);
    };
    let key = piece.id.to_ascii_lowercase();
    let open_labels: Vec<String> = states()
        .lock()
        .unwrap()
        .values()
        .filter(|s| s.id.eq_ignore_ascii_case(&piece.id))
        .map(|s| s.label.clone())
        .collect();
    if !open_labels.is_empty() {
        user_hidden().lock().unwrap().insert(key);
        crate::push::to_piece(app, &piece.id, "hiding", &serde_json::Value::Null);
        for label in open_labels {
            close(app, &label);
        }
        return Ok(false);
    }
    user_hidden().lock().unwrap().remove(&key);
    for (label, mon) in instances(piece, &crate::monitors::list(app)) {
        open(app, piece, &label, &mon)?;
    }
    crate::push::to_piece(app, &piece.id, "shown", &serde_json::Value::Null);
    Ok(true)
}

/// The page changed: reload every bar window's page in place, keeping the
/// window, its position and its reservation.
pub fn reload_pages(app: &AppHandle) {
    let labels: Vec<String> = states().lock().unwrap().keys().cloned().collect();
    for label in labels {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.eval("location.reload()");
        }
    }
}

/// Release every AppBar reservation (on quit, and before a crash restart).
pub fn release_all(app: &AppHandle) {
    let labels: Vec<String> = states().lock().unwrap().keys().cloned().collect();
    for label in labels {
        if let Some(window) = app.get_webview_window(&label) {
            #[cfg(windows)]
            appbar::remove(&window);
        }
    }
}

/// The taskbar or another AppBar moved: re-place reserved docks.
#[cfg(windows)]
fn reposition_reserved(app: &AppHandle) {
    if crate::updater::updating() {
        return;
    }
    let labels: Vec<String> = states().lock().unwrap().keys().cloned().collect();
    for label in labels {
        let Some(window) = app.get_webview_window(&label) else {
            continue;
        };
        let mut map = states().lock().unwrap();
        if let Some(st) = map.get_mut(&label) {
            if st.dock.mode == DockMode::Reserve {
                place(&window, st);
            }
        }
    }
}

/// Register every reserving bar afresh: remove first (Explorer may or may
/// not still know it; a second ABM_NEW for a known bar fails), and give a
/// bar that was refused another chance. After Explorer restarted.
#[cfg(windows)]
fn reregister_reserved(app: &AppHandle) {
    if crate::updater::updating() {
        return;
    }
    let labels: Vec<String> = states().lock().unwrap().keys().cloned().collect();
    for label in labels {
        let Some(window) = app.get_webview_window(&label) else {
            continue;
        };
        let mut map = states().lock().unwrap();
        if let Some(st) = map.get_mut(&label) {
            if st.dock.mode == DockMode::Reserve {
                appbar::forget(&window);
                st.refused = false;
                place(&window, st);
            }
        }
    }
}

/// Slide auto-hide docks out while the cursor is at their edge or on them.
fn start_autohide_watcher(app: &AppHandle) {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new().name("versailles-autohide".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_millis(40));
        let any = states()
            .lock()
            .unwrap()
            .values()
            .any(|s| s.dock.mode == DockMode::AutoHide);
        if !any {
            std::thread::sleep(Duration::from_millis(400));
            continue;
        }
        let Ok((cx, cy)) = crate::media::get_mouse_position() else {
            continue;
        };
        let mut changes: Vec<(String, String, bool)> = Vec::new();
        {
            let mut map = states().lock().unwrap();
            for st in map.values_mut().filter(|s| s.dock.mode == DockMode::AutoHide) {
                let zone = ((st.dock.peek.max(2) as f64) * st.scale).round() as i32;
                let open_rect = strip(st.dock.edge, st.work, st.thick);
                let want = if st.expanded {
                    if contains(open_rect, cx, cy) {
                        st.left_at = None;
                        true
                    } else {
                        let since = *st.left_at.get_or_insert_with(Instant::now);
                        since.elapsed() < Duration::from_millis(500)
                    }
                } else {
                    at_edge(st.dock.edge, st.work, zone, cx, cy)
                };
                if want != st.expanded {
                    st.expanded = want;
                    st.left_at = None;
                    changes.push((st.label.clone(), st.id.clone(), want));
                }
            }
        }
        for (label, id, expanded) in changes {
            let handle = app.clone();
            // Each monitor's copy slides on its own: tell that window only.
            if !expanded {
                crate::push::to_window(&handle, &label, &id, "hiding", &serde_json::Value::Null);
            }
            let _ = app.run_on_main_thread(move || {
                let Some(window) = handle.get_webview_window(&label) else {
                    return;
                };
                let mut map = states().lock().unwrap();
                if let Some(st) = map.get_mut(&label) {
                    place(&window, st);
                }
                drop(map);
                let name = if expanded { "shown" } else { "hidden" };
                crate::push::to_window(&handle, &label, &id, name, &serde_json::Value::Null);
            });
        }
    });
}

#[cfg(windows)]
mod appbar {
    //! Windows AppBar registration for `reserve` docks.
    use super::{Edge, Rect};
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    use tauri::WebviewWindow;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::UI::Shell::{
        DefSubclassProc, SHAppBarMessage, SetWindowSubclass, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT,
        ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABN_FULLSCREENAPP,
        ABN_POSCHANGED, APPBARDATA,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        RegisterWindowMessageW, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, WM_USER,
    };

    /// Message Explorer sends to our bar windows.
    const CALLBACK: u32 = WM_USER + 0x0A5;
    const SUBCLASS_ID: usize = 0x5645_5253; // "VERS"

    /// When we last moved a bar ourselves. Explorer notifies the *other* bars
    /// on that edge, so two of ours would keep nudging each other; notices
    /// right after our own move are ignored.
    fn last_set() -> &'static Mutex<Option<std::time::Instant>> {
        static AT: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();
        AT.get_or_init(|| Mutex::new(None))
    }

    /// Explorer's taskbar when our bars registered. A restarted Explorer has
    /// a new one and has forgotten every AppBar.
    fn explorer_at_register() -> &'static Mutex<isize> {
        static AT: OnceLock<Mutex<isize>> = OnceLock::new();
        AT.get_or_init(|| Mutex::new(0))
    }

    fn explorer_now() -> isize {
        crate::taskbar::explorer_tray().map(|h| h.0 as isize).unwrap_or(0)
    }

    fn taskbar_created() -> u32 {
        static MSG: OnceLock<u32> = OnceLock::new();
        *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(windows::core::w!("TaskbarCreated")) })
    }

    fn registered() -> &'static Mutex<HashSet<isize>> {
        static SET: OnceLock<Mutex<HashSet<isize>>> = OnceLock::new();
        SET.get_or_init(|| Mutex::new(HashSet::new()))
    }

    fn hwnd(window: &WebviewWindow) -> Option<HWND> {
        window.hwnd().ok().map(|h| HWND(h.0))
    }

    fn data(h: HWND) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: h,
            uCallbackMessage: CALLBACK,
            ..Default::default()
        }
    }

    fn abe(edge: Edge) -> u32 {
        match edge {
            Edge::Top => ABE_TOP,
            Edge::Bottom => ABE_BOTTOM,
            Edge::Left => ABE_LEFT,
            Edge::Right => ABE_RIGHT,
        }
    }

    /// Register (when `new`) and position an AppBar `thick` px deep on
    /// `edge` of `monitor`. Returns the rect Explorer granted.
    pub fn set_pos(
        window: &WebviewWindow,
        edge: Edge,
        monitor: Rect,
        work: Rect,
        thick: i32,
        new: bool,
    ) -> Option<Rect> {
        let h = hwnd(window)?;
        unsafe {
            let mut abd = data(h);
            if new || !registered().lock().unwrap().contains(&(h.0 as isize)) {
                if SHAppBarMessage(ABM_NEW, &mut abd) == 0 {
                    return None;
                }
                registered().lock().unwrap().insert(h.0 as isize);
                *explorer_at_register().lock().unwrap() = explorer_now();
            }
            abd.uEdge = abe(edge);
            // Propose a strip already `thick` deep along the edge (Microsoft's
            // pattern): Explorer slides it past the taskbar and other bars. A
            // whole-monitor proposal collapsed to a 1 px sliver for a bottom
            // bar on a monitor whose taskbar is at the top.
            let r = super::strip(edge, monitor, thick);
            abd.rc = RECT {
                left: r.x,
                top: r.y,
                right: r.x + r.width,
                bottom: r.y + r.height,
            };
            let proposed = abd.rc;
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            let queried = abd.rc;
            // Explorer may have shifted or trimmed it: restore our depth from
            // the edge it kept.
            match edge {
                Edge::Top => abd.rc.bottom = abd.rc.top + thick,
                Edge::Bottom => abd.rc.top = abd.rc.bottom - thick,
                Edge::Left => abd.rc.right = abd.rc.left + thick,
                Edge::Right => abd.rc.left = abd.rc.right - thick,
            }
            let to_rect = |rc: RECT| Rect {
                x: rc.left,
                y: rc.top,
                width: rc.right - rc.left,
                height: rc.bottom - rc.top,
            };
            if !super::usable_strip(edge, to_rect(abd.rc), monitor, thick) {
                // Explorer's answer is unusable: take our strip of the work
                // area (it already leaves out the taskbar and other bars).
                let r = super::strip(edge, work, thick);
                abd.rc = RECT {
                    left: r.x,
                    top: r.y,
                    right: r.x + r.width,
                    bottom: r.y + r.height,
                };
            }
            let sent = abd.rc;
            SHAppBarMessage(ABM_SETPOS, &mut abd);
            tracing::debug!(
                "appbar {:?}: proposed {:?} queried {:?} sent {:?} granted {:?}",
                edge,
                (proposed.left, proposed.top, proposed.right, proposed.bottom),
                (queried.left, queried.top, queried.right, queried.bottom),
                (sent.left, sent.top, sent.right, sent.bottom),
                (abd.rc.left, abd.rc.top, abd.rc.right, abd.rc.bottom)
            );
            *last_set().lock().unwrap() = Some(std::time::Instant::now());
            let granted = to_rect(abd.rc);
            if !super::usable_strip(edge, granted, monitor, thick) {
                // Even the reservation came back unusable: give it up; the
                // caller places the bar as a floating strip instead.
                SHAppBarMessage(ABM_REMOVE, &mut abd);
                registered().lock().unwrap().remove(&(h.0 as isize));
                return None;
            }
            Some(granted)
        }
    }

    /// Drop the registration (Explorer may already have) so the next
    /// placement registers the bar afresh.
    pub fn forget(window: &WebviewWindow) {
        let Some(h) = hwnd(window) else {
            return;
        };
        registered().lock().unwrap().remove(&(h.0 as isize));
        let mut abd = data(h);
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
    }

    /// The visible bar's window rect and its monitor's work area, physical px.
    pub fn bar_and_work(window: &WebviewWindow) -> Option<(Rect, Rect)> {
        use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};
        let h = hwnd(window)?;
        unsafe {
            if !IsWindowVisible(h).as_bool() {
                return None;
            }
            let mut r = RECT::default();
            GetWindowRect(h, &mut r).ok()?;
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
                return None;
            }
            let w = info.rcWork;
            Some((
                Rect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top },
                Rect { x: w.left, y: w.top, width: w.right - w.left, height: w.bottom - w.top },
            ))
        }
    }

    pub fn remove(window: &WebviewWindow) {
        let Some(h) = hwnd(window) else {
            return;
        };
        if registered().lock().unwrap().remove(&(h.0 as isize)) {
            let mut abd = data(h);
            unsafe {
                SHAppBarMessage(ABM_REMOVE, &mut abd);
            }
        }
    }

    /// Receive Explorer's AppBar notifications on this window.
    pub fn watch(window: &WebviewWindow) {
        let Some(h) = hwnd(window) else {
            return;
        };
        // Registered before any broadcast can arrive.
        taskbar_created();
        unsafe {
            let _ = SetWindowSubclass(h, Some(proc_), SUBCLASS_ID, 0);
        }
    }

    /// `TaskbarCreated` reached a bar. Versailles' own tray sends it too
    /// (`tray`), so only a new Explorer taskbar means the reservations are
    /// gone: forget them and reserve again (once for all bars).
    fn on_taskbar_created() {
        let now = explorer_now();
        {
            let mut at = explorer_at_register().lock().unwrap();
            if now == 0 || now == *at {
                return;
            }
            *at = now;
        }
        tracing::info!("docks: Explorer restarted; reserving bar space again");
        if let Some(app) = super::app_slot().get().cloned() {
            std::thread::spawn(move || {
                // Let the new taskbar settle (it may still be placing itself).
                std::thread::sleep(std::time::Duration::from_millis(500));
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || super::reregister_reserved(&handle));
            });
        }
    }

    unsafe extern "system" fn proc_(
        h: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == CALLBACK {
            match wparam.0 as u32 {
                ABN_POSCHANGED => {
                    // Taskbar or another bar moved: re-place ours (deferred,
                    // never inside Explorer's notification).
                    let ours = last_set()
                        .lock()
                        .unwrap()
                        .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(400));
                    if ours {
                        return LRESULT(0);
                    }
                    if let Some(app) = super::app_slot().get().cloned() {
                        std::thread::spawn(move || {
                            let handle = app.clone();
                            let _ = app.run_on_main_thread(move || super::reposition_reserved(&handle));
                        });
                    }
                }
                ABN_FULLSCREENAPP => {
                    // A fullscreen app opened (lParam != 0) or closed: get out
                    // of its way, then come back on top.
                    let after = if lparam.0 != 0 { HWND_NOTOPMOST } else { HWND_TOPMOST };
                    unsafe {
                        let _ = SetWindowPos(h, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    }
                }
                _ => {}
            }
            return LRESULT(0);
        }
        if msg == taskbar_created() {
            on_taskbar_created();
        }
        unsafe { DefSubclassProc(h, msg, wparam, lparam) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { x: 0, y: 72, width: 1920, height: 1128 };

    #[test]
    fn heal_backs_off() {
        assert_eq!(heal_backoff(0), Duration::ZERO);
        assert_eq!(heal_backoff(1), Duration::from_secs(8));
        assert_eq!(heal_backoff(2), Duration::from_secs(16));
        assert_eq!(heal_backoff(4), Duration::from_secs(64));
        assert_eq!(heal_backoff(5), Duration::from_secs(120));
        assert_eq!(heal_backoff(30), Duration::from_secs(120));
    }

    #[test]
    fn tells_whether_a_strip_is_reserved() {
        let full = Rect { x: 0, y: 0, width: 1920, height: 1200 };
        let bottom = Rect { x: 0, y: 1134, width: 1920, height: 66 };
        let top = Rect { x: 0, y: 0, width: 1920, height: 48 };
        // Both bars reserved: work area between them.
        let work = Rect { x: 0, y: 48, width: 1920, height: 1086 };
        assert!(strip_reserved(Edge::Bottom, bottom, work));
        assert!(strip_reserved(Edge::Top, top, work));
        // Explorer forgot them: the work area is the whole monitor.
        assert!(!strip_reserved(Edge::Bottom, bottom, full));
        assert!(!strip_reserved(Edge::Top, top, full));
        // Side bars, and a one-pixel rounding is tolerated.
        let left = Rect { x: 0, y: 0, width: 40, height: 1200 };
        assert!(strip_reserved(Edge::Left, left, Rect { x: 39, y: 0, width: 1881, height: 1200 }));
        assert!(!strip_reserved(Edge::Left, left, full));
        let right = Rect { x: 1880, y: 0, width: 40, height: 1200 };
        assert!(strip_reserved(Edge::Right, right, Rect { x: 0, y: 0, width: 1880, height: 1200 }));
        assert!(!strip_reserved(Edge::Right, right, full));
    }

    #[test]
    fn parses_edges_and_modes() {
        assert_eq!(parse_edge(Some("Top")), Some(Edge::Top));
        assert_eq!(parse_edge(Some("right")), Some(Edge::Right));
        assert_eq!(parse_edge(Some("tc")), None);
        assert_eq!(parse_mode(None), DockMode::Reserve);
        assert_eq!(parse_mode(Some("float")), DockMode::Float);
        assert_eq!(parse_mode(Some("auto-hide")), DockMode::AutoHide);
        assert_eq!(parse_mode(Some("whatever")), DockMode::Reserve);
    }

    #[test]
    fn strips_along_each_edge() {
        assert_eq!(strip(Edge::Top, WORK, 48), Rect { x: 0, y: 72, width: 1920, height: 48 });
        assert_eq!(strip(Edge::Bottom, WORK, 48), Rect { x: 0, y: 72 + 1128 - 48, width: 1920, height: 48 });
        assert_eq!(strip(Edge::Left, WORK, 60), Rect { x: 0, y: 72, width: 60, height: 1128 });
        assert_eq!(strip(Edge::Right, WORK, 60), Rect { x: 1860, y: 72, width: 60, height: 1128 });
        // Never deeper than the area.
        assert_eq!(strip(Edge::Top, WORK, 5000).height, 1128);
    }

    fn mon(number: u32, primary: bool, x: i32) -> MonitorInfo {
        let rect = Rect { x, y: 0, width: 1920, height: 1080 };
        MonitorInfo {
            number,
            primary,
            name: format!(r"\\.\DISPLAY{number}"),
            scale: 1.0,
            rect,
            work: rect,
        }
    }

    fn piece(html: &str) -> PagePiece {
        crate::page::parse_page(html).pieces.remove(0)
    }

    #[test]
    fn one_window_per_monitor_for_all() {
        let monitors = [mon(1, false, -1920), mon(2, true, 0), mon(3, false, 1920)];
        let all = piece(r#"<template class="spawnable" data-id="taskbar" data-anchor="bottom" data-monitor="all"></template>"#);
        assert!(on_every_monitor(&all));
        let labels: Vec<(String, u32)> = instances(&all, &monitors).into_iter().map(|(l, m)| (l, m.number)).collect();
        assert_eq!(
            labels,
            vec![
                ("widget-taskbar:1".to_string(), 1),
                ("widget-taskbar".to_string(), 2),
                ("widget-taskbar:3".to_string(), 3),
            ]
        );
        // A monitor going away drops its copy.
        assert_eq!(instances(&all, &monitors[1..2]).len(), 1);
    }

    #[test]
    fn single_monitor_bars_keep_one_window() {
        let monitors = [mon(1, true, 0), mon(2, false, 1920)];
        let on2 = piece(r#"<template class="spawnable" data-id="bar" data-anchor="top" data-monitor="2"></template>"#);
        let plain = piece(r#"<template class="spawnable" data-id="bar" data-anchor="top"></template>"#);
        let gone = piece(r#"<template class="spawnable" data-id="bar" data-anchor="top" data-monitor="7"></template>"#);
        let cursor = piece(r#"<template class="spawnable" data-id="bar" data-anchor="top" data-monitor="cursor"></template>"#);
        assert!(!on_every_monitor(&on2));
        let one = |p: &PagePiece| -> Vec<(String, u32)> {
            instances(p, &monitors).into_iter().map(|(l, m)| (l, m.number)).collect()
        };
        assert_eq!(one(&on2), vec![("widget-bar".to_string(), 2)]);
        assert_eq!(one(&plain), vec![("widget-bar".to_string(), 1)]);
        assert_eq!(one(&gone), vec![("widget-bar".to_string(), 1)]);
        assert_eq!(one(&cursor), vec![("widget-bar".to_string(), 1)]);
        // Not a dock: never "every monitor".
        let overlay = piece(r#"<template class="spawnable" data-id="x" data-anchor="c" data-monitor="all"></template>"#);
        assert!(!on_every_monitor(&overlay));
    }

    #[test]
    fn usable_strip_rejects_explorers_broken_answers() {
        let mon2 = Rect { x: 1920, y: 0, width: 3840, height: 2160 };
        // What Explorer granted on display 2: a 1 px sliver at the top.
        assert!(!usable_strip(Edge::Bottom, Rect { x: 1920, y: 72, width: 3840, height: 1 }, mon2, 66));
        // Bottom pushed to y = 0 (negative height).
        assert!(!usable_strip(Edge::Bottom, Rect { x: 1920, y: 2094, width: 3840, height: -2094 }, mon2, 66));
        // The right answer.
        assert!(usable_strip(Edge::Bottom, Rect { x: 1920, y: 2094, width: 3840, height: 66 }, mon2, 66));
        // A top bar below a 72 px taskbar is fine; one on the wrong half is not.
        assert!(usable_strip(Edge::Top, Rect { x: 1920, y: 72, width: 3840, height: 48 }, mon2, 48));
        assert!(!usable_strip(Edge::Top, Rect { x: 1920, y: 2000, width: 3840, height: 48 }, mon2, 48));
        // Off the monitor.
        assert!(!usable_strip(Edge::Left, Rect { x: 0, y: 0, width: 60, height: 2160 }, mon2, 60));
    }

    #[test]
    fn edge_zone_detection() {
        assert!(at_edge(Edge::Top, WORK, 6, 500, 72));
        assert!(at_edge(Edge::Top, WORK, 6, 500, 77));
        assert!(!at_edge(Edge::Top, WORK, 6, 500, 78));
        assert!(!at_edge(Edge::Top, WORK, 6, 2000, 72)); // other monitor
        assert!(at_edge(Edge::Right, WORK, 6, 1919, 600));
        assert!(!at_edge(Edge::Right, WORK, 6, 1900, 600));
        assert!(at_edge(Edge::Bottom, WORK, 6, 10, 72 + 1128 - 1));
    }
}
