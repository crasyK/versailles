use crate::error::{AppError, AppResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const PREWARM_LABEL: &str = "prewarm";

fn prewarm_window_slot() -> &'static Mutex<Option<WebviewWindow>> {
    static SLOT: OnceLock<Mutex<Option<WebviewWindow>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Spawn a hidden 1×1 WebView2 on a background task so the runtime is warm
/// before the first real window is built.
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

    let _ = window.set_decorations(false);
    let _ = window.set_shadow(false);

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
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        let _ = app.run_on_main_thread(move || {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        });
    });
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
