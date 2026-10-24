//! `posture` hook (read-only): whether a convertible is folded into its
//! tablet posture, whether it is docked, and what touch input the machine
//! has. Pages use it to reshape before the first tap (bigger buttons, a
//! touch-first command bar).
//!
//! - `tablet`: `GetSystemMetrics(SM_CONVERTIBLESLATEMODE)`, 0 = tablet
//!   (slate) posture. The metric only means something on a convertible:
//!   elsewhere Windows still answers a number, which says nothing about the
//!   machine (and a 0 would read as "tablet"). Windows keeps the posture in
//!   `HKLM\SYSTEM\CurrentControlSet\Control\PriorityControl\ConvertibleSlateMode`
//!   once the machine's slate / laptop indicator (the hinge or keyboard
//!   sensor a convertible's firmware reports) has given one, so that value's
//!   presence is what marks a convertible here; without it `tablet` is
//!   `null`. (Chromium gates the same metric on the power platform role
//!   instead, which also counts plain laptops; the registry value is the
//!   narrower test. Neither is documented as the test.)
//! - `docked`: `SM_SYSTEMDOCKED` is non-zero.
//! - `touch`: `SM_DIGITIZER` (`NID_*` bits; built-in or external) and
//!   `SM_MAXIMUMTOUCHES`.
//!
//! Windows broadcasts `WM_SETTINGCHANGE` with `"ConvertibleSlateMode"` or
//! `"SystemDockMode"` when either changes. Broadcasts never reach
//! message-only windows, and no host window handles that message, so a
//! hidden top-level window on its own thread listens (started the first
//! time a piece declares `posture`) and tells the sensor watcher to read at
//! once, and once more a second later in case the metric lags the broadcast.
//! The watcher's 5 s poll covers a missed broadcast.

use crate::error::AppResult;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

/// `NID_*` bits of `SM_DIGITIZER`.
const NID_INTEGRATED_TOUCH: i32 = 0x01;
const NID_EXTERNAL_TOUCH: i32 = 0x02;
const NID_INTEGRATED_PEN: i32 = 0x04;
const NID_EXTERNAL_PEN: i32 = 0x08;

/// Raw readings → page JSON. `slate_mode` is `SM_CONVERTIBLESLATEMODE`, or
/// `None` when the machine does not report a posture.
pub fn posture_json(slate_mode: Option<i32>, docked: i32, digitizer: i32, max_touches: i32) -> Value {
    json!({
        "tablet": slate_mode.map(|m| m == 0),
        "docked": docked != 0,
        "touch": {
            "screen": digitizer & (NID_INTEGRATED_TOUCH | NID_EXTERNAL_TOUCH) != 0,
            "pen": digitizer & (NID_INTEGRATED_PEN | NID_EXTERNAL_PEN) != 0,
            "maxTouches": max_touches.max(0),
        },
    })
}

/// Whether a `WM_SETTINGCHANGE` area (its lParam string) is a posture change.
pub fn is_posture_setting(area: &str) -> bool {
    area.eq_ignore_ascii_case("ConvertibleSlateMode") || area.eq_ignore_ascii_case("SystemDockMode")
}

/// Set by the listener when Windows announces a posture change.
static CHANGED: AtomicBool = AtomicBool::new(false);

/// Whether a posture change was announced since the last call.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::SeqCst)
}

/// Current posture.
pub fn read() -> AppResult<Value> {
    imp::read()
}

/// Start the `WM_SETTINGCHANGE` listener (once per run).
pub fn listen() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("posture".into())
            .spawn(imp::listen_thread)
            .map_err(|err| tracing::warn!("posture: listener failed: {err}"));
    });
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics, KillTimer,
        RegisterClassW, SetTimer, TranslateMessage, MSG, SM_CONVERTIBLESLATEMODE, SM_DIGITIZER,
        SM_MAXIMUMTOUCHES, SM_SYSTEMDOCKED, WINDOW_STYLE, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };

    /// Timer for the second read after a broadcast.
    const RECHECK: usize = 1;

    /// Whether Windows keeps a convertible posture (see the module doc).
    fn convertible() -> bool {
        unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!("SYSTEM\\CurrentControlSet\\Control\\PriorityControl"),
                w!("ConvertibleSlateMode"),
                RRF_RT_REG_DWORD,
                None,
                None,
                None,
            )
        }
        .is_ok()
    }

    pub fn read() -> AppResult<Value> {
        unsafe {
            let slate = convertible().then(|| GetSystemMetrics(SM_CONVERTIBLESLATEMODE));
            Ok(posture_json(
                slate,
                GetSystemMetrics(SM_SYSTEMDOCKED),
                GetSystemMetrics(SM_DIGITIZER),
                GetSystemMetrics(SM_MAXIMUMTOUCHES),
            ))
        }
    }

    unsafe extern "system" fn proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_SETTINGCHANGE if l.0 != 0 => {
                let area = PCWSTR(l.0 as *const u16).to_string().unwrap_or_default();
                if is_posture_setting(&area) {
                    CHANGED.store(true, Ordering::SeqCst);
                    SetTimer(Some(h), RECHECK, 1000, None);
                }
            }
            WM_TIMER if w.0 == RECHECK => {
                let _ = KillTimer(Some(h), RECHECK);
                CHANGED.store(true, Ordering::SeqCst);
                return LRESULT(0);
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
                lpszClassName: w!("VersaillesPostureWatch"),
                ..Default::default()
            });
            // Top-level (broadcasts skip message-only windows), never shown.
            let made = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("VersaillesPostureWatch"),
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
            if let Err(err) = made {
                tracing::warn!("posture: listener window failed, the 5 s poll still works: {err}");
                return;
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
    pub fn read() -> AppResult<Value> {
        Err(crate::error::AppError::msg("system hooks are only available on Windows"))
    }
    pub fn listen_thread() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laptop_with_touch_and_pen() {
        // This machine: laptop posture, digitizer 0xC5, 10 touch points.
        let v = posture_json(Some(1), 0, 0xC5, 10);
        assert_eq!(v["tablet"], false);
        assert_eq!(v["docked"], false);
        assert_eq!(v["touch"]["screen"], true);
        assert_eq!(v["touch"]["pen"], true);
        assert_eq!(v["touch"]["maxTouches"], 10);
    }

    #[test]
    fn folded_is_tablet() {
        let v = posture_json(Some(0), 0, 0xC5, 10);
        assert_eq!(v["tablet"], true);
    }

    #[test]
    fn no_posture_is_null() {
        // Desktop PC: no convertible posture, docked, no digitizer.
        let v = posture_json(None, 1, 0, 0);
        assert_eq!(v["tablet"], Value::Null);
        assert_eq!(v["docked"], true);
        assert_eq!(v["touch"]["screen"], false);
        assert_eq!(v["touch"]["pen"], false);
        assert_eq!(v["touch"]["maxTouches"], 0);
    }

    #[test]
    fn external_digitizers_count() {
        let v = posture_json(None, 0, NID_EXTERNAL_TOUCH, 2);
        assert_eq!(v["touch"]["screen"], true);
        assert_eq!(v["touch"]["pen"], false);
        let v = posture_json(None, 0, NID_EXTERNAL_PEN | 0x80, 0);
        assert_eq!(v["touch"]["screen"], false);
        assert_eq!(v["touch"]["pen"], true);
        // Ready / multi-input bits alone are not a digitizer.
        let v = posture_json(None, 0, 0xC0, 0);
        assert_eq!(v["touch"]["screen"], false);
        assert_eq!(v["touch"]["pen"], false);
    }

    #[test]
    fn negative_touches_clamp() {
        assert_eq!(posture_json(None, 0, 0, -1)["touch"]["maxTouches"], 0);
    }

    #[test]
    fn setting_areas() {
        assert!(is_posture_setting("ConvertibleSlateMode"));
        assert!(is_posture_setting("SystemDockMode"));
        assert!(is_posture_setting("convertibleslatemode"));
        assert!(!is_posture_setting("ImmersiveColorSet"));
        assert!(!is_posture_setting(""));
    }

    /// `cargo test live_posture -- --ignored --nocapture`: print the current
    /// reading (fold the screen and run again).
    #[test]
    #[ignore]
    fn live_posture() {
        let v = read().unwrap();
        println!("posture: {v}");
        assert!(v["touch"]["maxTouches"].is_u64());
    }
}
