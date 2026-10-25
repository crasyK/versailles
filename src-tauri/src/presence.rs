//! What anyone can see right now, for the watchers that poll Windows (#49).
//!
//! A sensor (and the `windows` list) is read only while a piece that declares
//! it has a visible window: the wallpaper for an `article.widget`, the piece's
//! own window for a spawnable (a docked bar, an open slide-out). A hidden
//! slide-out such as quick settings costs nothing until it is shown; showing
//! it wakes the watchers (`wake`), which read at once and push what changed
//! meanwhile. Nothing is read while the session is locked or the display is
//! off (`away`): a hidden top-level window on its own thread hears
//! `WM_WTSSESSION_CHANGE` and the console display power setting.

use crate::page::{PageCatalog, PageKind};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};

static LOCKED: AtomicBool = AtomicBool::new(false);
static DISPLAY_OFF: AtomicBool = AtomicBool::new(false);

/// Nobody can see the screen: the session is locked or the display is off.
pub fn away() -> bool {
    LOCKED.load(Ordering::SeqCst) || DISPLAY_OFF.load(Ordering::SeqCst)
}

fn bell() -> &'static (Mutex<u64>, Condvar) {
    static BELL: OnceLock<(Mutex<u64>, Condvar)> = OnceLock::new();
    BELL.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

/// Wake every watcher sleeping in `nap` (a piece was shown, the user is back).
pub fn wake() {
    let (count, cv) = bell();
    *count.lock().unwrap() += 1;
    cv.notify_all();
}

/// Sleep for `d`, or less when `wake` is called meanwhile.
pub fn nap(d: Duration) {
    let (count, cv) = bell();
    let guard = count.lock().unwrap();
    let seen = *guard;
    let _ = cv.wait_timeout_while(guard, d, |n| *n == seen);
}

/// Whether a piece is seen, given whether the wallpaper is and the labels of
/// its own visible windows (`widget-<id>`, `widget-<id>:<n>`).
pub fn piece_seen(kind: PageKind, id: &str, wallpaper: bool, visible: &[String]) -> bool {
    match kind {
        PageKind::Widget => wallpaper,
        PageKind::Spawnable => visible
            .iter()
            .any(|l| crate::page::label_piece_id(l).is_some_and(|p| p.eq_ignore_ascii_case(id))),
    }
}

/// Hooks declared by pieces that have a visible window; none while `away`.
pub fn seen_hooks(app: &AppHandle, cat: &PageCatalog) -> HashSet<String> {
    let mut out = HashSet::new();
    if away() || cat.pieces.iter().all(|p| p.hooks.is_empty()) {
        return out;
    }
    let labels: Vec<String> = app.webview_windows().into_keys().collect();
    imp::forget_closed(&labels);
    let visible: Vec<String> = labels.into_iter().filter(|l| imp::shown(app, l)).collect();
    let wallpaper = visible.iter().any(|l| crate::page::is_desktop_label(l));
    for piece in &cat.pieces {
        if !piece.hooks.is_empty() && piece_seen(piece.kind, &piece.id, wallpaper, &visible) {
            out.extend(piece.hooks.iter().cloned());
        }
    }
    out
}

/// Start listening for lock / unlock and display on / off (once per run).
pub fn start() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("versailles-presence".into())
            .spawn(imp::listen_thread)
            .map_err(|err| tracing::warn!("presence: listener failed: {err}"));
    });
}

/// `GUID_CONSOLE_DISPLAY_STATE` data: 0 off, 1 on, 2 dimmed.
pub fn display_off(state: u8) -> bool {
    state == 0
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::collections::HashMap;
    use windows::core::{w, GUID};
    use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
    use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, IsIconic, IsWindow, IsWindowVisible,
        RegisterClassW, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, MSG, PBT_POWERSETTINGCHANGE, WINDOW_STYLE,
        WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP, WTS_SESSION_LOCK,
        WTS_SESSION_UNLOCK,
    };

    const GUID_CONSOLE_DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);

    /// Window label → its handle, so a visibility check is two cheap calls
    /// instead of a round trip to the main thread.
    fn handles() -> &'static Mutex<HashMap<String, isize>> {
        static MAP: OnceLock<Mutex<HashMap<String, isize>>> = OnceLock::new();
        MAP.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn forget_closed(labels: &[String]) {
        handles().lock().unwrap().retain(|l, _| labels.contains(l));
    }

    pub fn shown(app: &AppHandle, label: &str) -> bool {
        let cached = handles().lock().unwrap().get(label).copied();
        let h = match cached.map(|h| HWND(h as *mut _)) {
            Some(h) if unsafe { IsWindow(Some(h)) }.as_bool() => h,
            _ => {
                let Some(h) = app
                    .get_webview_window(label)
                    .and_then(|w| w.hwnd().ok())
                    .map(|h| HWND(h.0))
                else {
                    return false;
                };
                handles().lock().unwrap().insert(label.to_string(), h.0 as isize);
                h
            }
        };
        unsafe { IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() }
    }

    unsafe extern "system" fn proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_WTSSESSION_CHANGE => match w.0 as u32 {
                WTS_SESSION_LOCK => LOCKED.store(true, Ordering::SeqCst),
                WTS_SESSION_UNLOCK => {
                    LOCKED.store(false, Ordering::SeqCst);
                    wake();
                }
                _ => {}
            },
            WM_POWERBROADCAST if w.0 as u32 == PBT_POWERSETTINGCHANGE && l.0 != 0 => {
                let setting = &*(l.0 as *const POWERBROADCAST_SETTING);
                if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && setting.DataLength >= 1 {
                    let off = display_off(setting.Data[0]);
                    if DISPLAY_OFF.swap(off, Ordering::SeqCst) && !off {
                        wake();
                    }
                }
                return LRESULT(1);
            }
            _ => {}
        }
        DefWindowProcW(h, msg, w, l)
    }

    pub fn listen_thread() {
        unsafe {
            let Some(instance) = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0)) else {
                return;
            };
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance,
                lpszClassName: w!("VersaillesPresenceWatch"),
                ..Default::default()
            });
            // Never shown.
            let made = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("VersaillesPresenceWatch"),
                w!(""),
                WINDOW_STYLE(WS_POPUP.0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            );
            let window = match made {
                Ok(h) => h,
                Err(err) => {
                    tracing::warn!("presence: listener window failed, sensors keep polling while away: {err}");
                    return;
                }
            };
            if let Err(err) = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) {
                tracing::warn!("presence: no lock notifications: {err}");
            }
            // Windows answers at once with the current display state.
            if let Err(err) = RegisterPowerSettingNotification(
                HANDLE(window.0),
                &GUID_CONSOLE_DISPLAY_STATE,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            ) {
                tracing::warn!("presence: no display state notifications: {err}");
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn forget_closed(_labels: &[String]) {}
    pub fn shown(app: &AppHandle, label: &str) -> bool {
        app.get_webview_window(label)
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false)
    }
    pub fn listen_thread() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widgets_follow_the_wallpaper_spawnables_their_own_windows() {
        let visible = vec!["desktop".to_string(), "widget-taskbar:2".to_string(), "widget-topbar".to_string()];
        assert!(piece_seen(PageKind::Widget, "clock", true, &visible));
        assert!(!piece_seen(PageKind::Widget, "clock", false, &visible));
        // A bar's monitor copy counts; so does a different case.
        assert!(piece_seen(PageKind::Spawnable, "taskbar", true, &visible));
        assert!(piece_seen(PageKind::Spawnable, "TopBar", false, &visible));
        // A hidden slide-out (no visible window) is not seen.
        assert!(!piece_seen(PageKind::Spawnable, "quick", true, &visible));
        assert!(!piece_seen(PageKind::Spawnable, "top", true, &visible));
    }

    #[test]
    fn display_states() {
        assert!(display_off(0));
        assert!(!display_off(1));
        // Dimmed is still visible.
        assert!(!display_off(2));
    }

    #[test]
    fn wake_ends_a_nap() {
        let start = std::time::Instant::now();
        let t = std::thread::spawn(|| nap(Duration::from_secs(10)));
        // Until the nap has started and heard it.
        while !t.is_finished() && start.elapsed() < Duration::from_secs(5) {
            wake();
            std::thread::sleep(Duration::from_millis(20));
        }
        t.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
