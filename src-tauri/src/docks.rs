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

use crate::error::AppResult;
use crate::layout::Rect;
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
    id: String,
    dock: Dock,
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
}

fn states() -> &'static Mutex<HashMap<String, DockState>> {
    static MAP: OnceLock<Mutex<HashMap<String, DockState>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn app_slot() -> &'static OnceLock<AppHandle> {
    static APP: OnceLock<AppHandle> = OnceLock::new();
    &APP
}

fn label(id: &str) -> String {
    crate::window_manager::widget_label(id)
}

/// Monitor a dock sits on: its `data-monitor` (primary or a number), else
/// the primary. Docks do not follow the cursor.
fn dock_monitor(app: &AppHandle, piece: &PagePiece) -> Option<crate::monitors::MonitorInfo> {
    let monitors = crate::monitors::list(app);
    let sel = crate::monitors::parse_sel(piece.monitor.as_deref())
        .filter(|s| matches!(s, crate::monitors::MonitorSel::Number(_)))
        .unwrap_or(crate::monitors::MonitorSel::Primary);
    crate::monitors::select(&monitors, sel)
        .or_else(|| crate::monitors::select(&monitors, crate::monitors::MonitorSel::Primary))
        .cloned()
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
                if let Some(r) = appbar::set_pos(window, st.dock.edge, st.monitor, st.thick, !st.appbar) {
                    st.appbar = true;
                    set_rect(window, r);
                    return;
                }
            }
            set_rect(window, strip(st.dock.edge, st.work, st.thick));
        }
    }
}

/// Open, place or close dock windows to match the page. Call on the main
/// thread (AppBar registration and window subclassing need the UI thread).
pub fn reconcile(app: &AppHandle) {
    let _ = app_slot().set(app.clone());
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let wanted: Vec<&PagePiece> = cat.pieces.iter().filter(|p| piece_dock(p).is_some()).collect();

    // Close docks the page no longer declares (or that stopped being docks).
    let stale: Vec<String> = states()
        .lock()
        .unwrap()
        .keys()
        .filter(|id| !wanted.iter().any(|p| p.id.eq_ignore_ascii_case(id)))
        .cloned()
        .collect();
    for id in stale {
        close(app, &id);
    }

    for piece in wanted {
        let hidden_by_user = states()
            .lock()
            .unwrap()
            .get(&piece.id)
            .is_none()
            && user_hidden().lock().unwrap().contains(&piece.id.to_ascii_lowercase());
        if hidden_by_user {
            continue;
        }
        if let Err(err) = open(app, piece) {
            tracing::warn!("dock '{}' failed to open: {err}", piece.id);
        }
    }
    start_autohide_watcher(app);
}

/// Docks the user toggled off with their hotkey (until toggled on again).
fn user_hidden() -> &'static Mutex<std::collections::HashSet<String>> {
    static SET: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

fn open(app: &AppHandle, piece: &PagePiece) -> AppResult<()> {
    let dock = piece_dock(piece).expect("caller checked");
    let Some(mon) = dock_monitor(app, piece) else {
        return Ok(());
    };
    let thick = ((thickness(piece, dock.edge) as f64) * mon.scale).round() as i32;
    let mut st = states().lock().unwrap().remove(&piece.id).unwrap_or(DockState {
        id: piece.id.clone(),
        dock,
        monitor: mon.rect,
        work: mon.work,
        scale: mon.scale,
        thick,
        expanded: false,
        left_at: None,
        #[cfg(windows)]
        appbar: false,
    });
    // Mode or edge changed: drop a reservation made for the old one.
    #[cfg(windows)]
    if st.appbar && (st.dock != dock || dock.mode != DockMode::Reserve) {
        if let Some(w) = app.get_webview_window(&label(&piece.id)) {
            appbar::remove(&w);
        }
        st.appbar = false;
    }
    st.dock = dock;
    st.monitor = mon.rect;
    st.work = mon.work;
    st.scale = mon.scale;
    st.thick = thick;

    let win_label = label(&piece.id);
    if app.get_webview_window(&win_label).is_none() {
        crate::window_manager::mark_quiet(&win_label);
        let first = strip(dock.edge, mon.work, thick);
        crate::window_manager::open_widget_window(
            app,
            &app.state::<AppState>().window_manager,
            &piece.id,
            Some(crate::config::Position { x: first.x, y: first.y }),
            Some(true),
        )?;
    }
    if let Some(window) = app.get_webview_window(&win_label) {
        #[cfg(windows)]
        appbar::watch(&window);
        place(&window, &mut st);
        let _ = window.set_always_on_top(true);
        let _ = window.show();
    }
    states().lock().unwrap().insert(piece.id.clone(), st);
    Ok(())
}

fn close(app: &AppHandle, id: &str) {
    states().lock().unwrap().remove(id);
    if let Some(window) = app.get_webview_window(&label(id)) {
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

/// Hotkey / `spawn()`: hide a showing dock (releasing its space) or bring a
/// hidden one back. Returns whether it is now shown.
pub fn toggle(app: &AppHandle, id: &str) -> AppResult<bool> {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let Some(piece) = cat.spawnable(id) else {
        return Ok(false);
    };
    let key = piece.id.to_ascii_lowercase();
    if states().lock().unwrap().contains_key(&piece.id) {
        user_hidden().lock().unwrap().insert(key);
        crate::push::to_piece(app, &piece.id, "hiding", &serde_json::Value::Null);
        close(app, &piece.id);
        return Ok(false);
    }
    user_hidden().lock().unwrap().remove(&key);
    open(app, piece)?;
    crate::push::to_piece(app, &piece.id, "shown", &serde_json::Value::Null);
    Ok(true)
}

/// Release every AppBar reservation (on quit, and before a crash restart).
pub fn release_all(app: &AppHandle) {
    let ids: Vec<String> = states().lock().unwrap().keys().cloned().collect();
    for id in ids {
        if let Some(window) = app.get_webview_window(&label(&id)) {
            #[cfg(windows)]
            appbar::remove(&window);
        }
    }
}

/// The taskbar or another AppBar moved: re-place reserved docks.
#[cfg(windows)]
fn reposition_reserved(app: &AppHandle) {
    let ids: Vec<String> = states().lock().unwrap().keys().cloned().collect();
    for id in ids {
        let Some(window) = app.get_webview_window(&label(&id)) else {
            continue;
        };
        let mut map = states().lock().unwrap();
        if let Some(st) = map.get_mut(&id) {
            if st.dock.mode == DockMode::Reserve {
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
    std::thread::spawn(move || loop {
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
        let mut changes: Vec<(String, bool)> = Vec::new();
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
                    changes.push((st.id.clone(), want));
                }
            }
        }
        for (id, expanded) in changes {
            let handle = app.clone();
            if !expanded {
                crate::push::to_piece(&handle, &id, "hiding", &serde_json::Value::Null);
            }
            let _ = app.run_on_main_thread(move || {
                let Some(window) = handle.get_webview_window(&label(&id)) else {
                    return;
                };
                let mut map = states().lock().unwrap();
                if let Some(st) = map.get_mut(&id) {
                    place(&window, st);
                }
                drop(map);
                let name = if expanded { "shown" } else { "hidden" };
                crate::push::to_piece(&handle, &id, name, &serde_json::Value::Null);
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
        SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        WM_USER,
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
    pub fn set_pos(window: &WebviewWindow, edge: Edge, monitor: Rect, thick: i32, new: bool) -> Option<Rect> {
        let h = hwnd(window)?;
        unsafe {
            let mut abd = data(h);
            if new || !registered().lock().unwrap().contains(&(h.0 as isize)) {
                if SHAppBarMessage(ABM_NEW, &mut abd) == 0 {
                    return None;
                }
                registered().lock().unwrap().insert(h.0 as isize);
            }
            abd.uEdge = abe(edge);
            abd.rc = RECT {
                left: monitor.x,
                top: monitor.y,
                right: monitor.x + monitor.width,
                bottom: monitor.y + monitor.height,
            };
            // Ask for the whole monitor side; Explorer moves the inner edge past
            // the taskbar and other bars. Then set our depth from that edge.
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            match edge {
                Edge::Top => abd.rc.bottom = abd.rc.top + thick,
                Edge::Bottom => abd.rc.top = abd.rc.bottom - thick,
                Edge::Left => abd.rc.right = abd.rc.left + thick,
                Edge::Right => abd.rc.left = abd.rc.right - thick,
            }
            SHAppBarMessage(ABM_SETPOS, &mut abd);
            *last_set().lock().unwrap() = Some(std::time::Instant::now());
            let r = abd.rc;
            Some(Rect {
                x: r.left,
                y: r.top,
                width: (r.right - r.left).max(1),
                height: (r.bottom - r.top).max(1),
            })
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
        unsafe {
            let _ = SetWindowSubclass(h, Some(proc_), SUBCLASS_ID, 0);
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
        unsafe { DefSubclassProc(h, msg, wparam, lparam) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { x: 0, y: 72, width: 1920, height: 1128 };

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
