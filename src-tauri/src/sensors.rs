//! Read-only system hooks (`battery`, `network`, `focus`, `audio`) and the
//! watcher that pushes their changes, plus new toasts for `notify` (see
//! `toasts`) and page-wide `idle` / `active` and `theme-changed` events. A
//! sensor is only read while some piece declares its hook, and only a
//! changed value is pushed.

use crate::error::{AppError, AppResult};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// Hooks `sense_now` answers, with how often the watcher reads them.
pub const SENSORS: &[(&str, Duration)] = &[
    ("focus", Duration::from_millis(500)),
    ("audio", Duration::from_millis(1000)),
    ("network", Duration::from_millis(3000)),
    ("battery", Duration::from_millis(15000)),
    // Quick settings (see `quick`).
    ("radios", Duration::from_millis(2000)),
    ("power-mode", Duration::from_millis(2000)),
    ("dnd", Duration::from_millis(2000)),
    ("audio-control", Duration::from_millis(3000)),
    // DDC/CI brightness reads are slow: not more often.
    ("display", Duration::from_millis(5000)),
    ("bluetooth", Duration::from_millis(3000)),
    ("wifi", Duration::from_millis(5000)),
    // Connections and accessibility (#63–#66): Windows sends none of these
    // as events an unpackaged app can get, so they are polled.
    ("vpn", Duration::from_millis(2000)),
    ("hotspot", Duration::from_millis(3000)),
    ("sharing", Duration::from_millis(3000)),
    ("accessibility", Duration::from_millis(2000)),
];

/// No input for this long sends `idle`; the next input sends `active`.
pub const IDLE_AFTER_MS: u64 = 60_000;

/// How often the watcher looks for new toasts (`notify`).
pub const NOTIFY_EVERY: Duration = Duration::from_millis(2000);

pub fn is_sensor(hook: &str) -> bool {
    SENSORS.iter().any(|(h, _)| *h == hook)
}

/// Hooks `sense_now` answers: the sensors plus `notify` (whose pushes are
/// new toasts, not a changed value).
pub fn is_readable(hook: &str) -> bool {
    is_sensor(hook) || hook == "notify"
}

/// Current value of one readable hook.
pub fn read(hook: &str) -> AppResult<Value> {
    match hook {
        "battery" => imp::battery(),
        "network" => imp::network(),
        "focus" => imp::focus(),
        "audio" => imp::audio(),
        "notify" => crate::toasts::read(),
        "radios" | "audio-control" | "display" | "power-mode" | "wifi" | "bluetooth" | "dnd" => crate::quick::read(hook),
        "vpn" | "hotspot" | "sharing" | "accessibility" => crate::quick::read(hook),
        _ => Err(AppError::msg(format!("'{hook}' is not a sensor hook"))),
    }
}

/// `SYSTEM_POWER_STATUS` fields → page JSON.
pub fn battery_json(ac_line: u8, flag: u8, percent: u8, saver: bool, seconds_left: u32) -> Value {
    let present = flag != 128 && flag != 255;
    json!({
        "present": present,
        "percent": if present && percent <= 100 { Some(percent) } else { None },
        "plugged": match ac_line { 0 => Some(false), 1 => Some(true), _ => None },
        "charging": present && flag & 8 != 0,
        "secondsLeft": if seconds_left == u32::MAX { None } else { Some(seconds_left) },
        "saver": saver,
    })
}

/// Connectivity level name used in the `network` payload.
pub fn level_name(level: i32) -> &'static str {
    match level {
        1 => "local",
        2 => "constrained",
        3 => "internet",
        _ => "none",
    }
}

/// Idle transition for `idle_ms` since the last input: `Some(true)` = went
/// idle, `Some(false)` = became active, `None` = no change.
pub fn idle_transition(was_idle: bool, idle_ms: u64) -> Option<bool> {
    match (was_idle, idle_ms >= IDLE_AFTER_MS) {
        (false, true) => Some(true),
        (true, false) => Some(false),
        _ => None,
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{w, PWSTR};
    use windows::Win32::Foundation::CloseHandle;

    pub fn battery() -> AppResult<Value> {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut s = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut s) }.map_err(|e| AppError::msg(e.to_string()))?;
        Ok(battery_json(
            s.ACLineStatus,
            s.BatteryFlag,
            s.BatteryLifePercent,
            s.SystemStatusFlag & 1 != 0,
            s.BatteryLifeTime,
        ))
    }

    pub fn network() -> AppResult<Value> {
        use windows::Networking::Connectivity::NetworkInformation;
        let Ok(profile) = NetworkInformation::GetInternetConnectionProfile() else {
            return Ok(json!({ "online": false, "level": "none", "name": null, "wifi": false, "signalBars": null }));
        };
        let level = profile
            .GetNetworkConnectivityLevel()
            .map(|l| l.0)
            .unwrap_or(0);
        Ok(json!({
            "online": level == 3,
            "level": level_name(level),
            "name": profile.ProfileName().ok().map(|s| s.to_string()),
            "wifi": profile.IsWlanConnectionProfile().unwrap_or(false),
            "signalBars": profile.GetSignalBars().ok().and_then(|r| r.Value().ok()),
        }))
    }

    pub fn focus() -> AppResult<Value> {
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        };
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return Ok(Value::Null);
            }
            let len = GetWindowTextLengthW(hwnd).max(0) as usize;
            let mut buf = vec![0u16; len + 1];
            let copied = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
            let title = String::from_utf16_lossy(&buf[..copied]);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let exe = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .ok()
                .and_then(|handle| {
                    let mut path = [0u16; 1024];
                    let mut size = path.len() as u32;
                    let ok = QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        PWSTR(path.as_mut_ptr()),
                        &mut size,
                    )
                    .is_ok();
                    let _ = CloseHandle(handle);
                    ok.then(|| String::from_utf16_lossy(&path[..size as usize]))
                });
            let app = exe.as_deref().and_then(|p| {
                std::path::Path::new(p)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            });
            Ok(json!({
                "title": title,
                "app": app,
                "exe": exe,
                "pid": pid,
                "self": pid == std::process::id(),
            }))
        }
    }

    pub fn audio() -> AppResult<Value> {
        use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
        };
        let err = |e: windows::core::Error| AppError::msg(e.to_string());
        unsafe {
            // Already initialised on this thread is fine.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let devices: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err)?;
            let Ok(device) = devices.GetDefaultAudioEndpoint(eRender, eConsole) else {
                return Ok(json!({ "device": null, "volume": null, "muted": null }));
            };
            let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).map_err(err)?;
            let level = volume.GetMasterVolumeLevelScalar().map_err(err)?;
            let muted = volume.GetMute().map_err(err)?.as_bool();
            let name = device
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                .ok()
                .map(|v| v.to_string())
                .filter(|s| !s.is_empty());
            Ok(json!({
                "device": name,
                "volume": (level * 100.0).round() as u32,
                "muted": muted,
            }))
        }
    }

    /// Milliseconds since the last keyboard / mouse input.
    pub fn idle_ms() -> Option<u64> {
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        unsafe {
            if !GetLastInputInfo(&mut info).as_bool() {
                return None;
            }
            Some(GetTickCount().wrapping_sub(info.dwTime) as u64)
        }
    }

    /// Whether Windows apps are set to dark mode.
    pub fn dark_mode() -> Option<bool> {
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        status.is_ok().then_some(value == 0)
    }

    pub fn init_thread() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    fn unavailable() -> AppResult<Value> {
        Err(AppError::msg("system hooks are only available on Windows"))
    }
    pub fn battery() -> AppResult<Value> {
        unavailable()
    }
    pub fn network() -> AppResult<Value> {
        unavailable()
    }
    pub fn focus() -> AppResult<Value> {
        unavailable()
    }
    pub fn audio() -> AppResult<Value> {
        unavailable()
    }
    pub fn idle_ms() -> Option<u64> {
        None
    }
    pub fn dark_mode() -> Option<bool> {
        None
    }
    pub fn init_thread() {}
}

/// Hooks some piece on the page declares.
fn declared_hooks(app: &AppHandle) -> HashSet<String> {
    let state = app.state::<crate::state::AppState>();
    let cat = crate::desktop::page_catalog(&state);
    cat.pieces.iter().flat_map(|p| p.hooks.iter().cloned()).collect()
}

/// Background watcher: pushes sensor changes to pieces with the hook, and
/// `idle` / `active` / `theme-changed` to every page window.
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        imp::init_thread();
        let mut last: HashMap<&str, Value> = HashMap::new();
        let mut due: HashMap<&str, Instant> = HashMap::new();
        let mut idle = false;
        let mut dark = imp::dark_mode();
        let mut next_idle = Instant::now();
        let mut next_theme = Instant::now();
        let mut toasts = crate::toasts::Watch::default();
        let mut next_toasts = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = Instant::now();
            let declared = declared_hooks(&app);
            for (hook, every) in SENSORS {
                if !declared.contains(*hook) {
                    // Next declaration starts fresh and pushes its first value.
                    last.remove(hook);
                    continue;
                }
                if due.get(hook).is_some_and(|t| now < *t) {
                    continue;
                }
                due.insert(hook, now + *every);
                if let Ok(value) = read(hook) {
                    if last.get(hook) != Some(&value) {
                        crate::push::to_hook(&app, hook, &value);
                        last.insert(hook, value);
                    }
                }
            }
            if !declared.contains("notify") {
                toasts.reset();
            } else if now >= next_toasts {
                next_toasts = now + NOTIFY_EVERY;
                for toast in toasts.poll() {
                    if let Ok(data) = serde_json::to_value(&toast) {
                        crate::push::to_hook(&app, "notify", &data);
                    }
                }
            }
            if now >= next_idle {
                next_idle = now + Duration::from_secs(1);
                if let Some(ms) = imp::idle_ms() {
                    match idle_transition(idle, ms) {
                        Some(true) => {
                            idle = true;
                            crate::push::to_all(&app, "idle", &json!({ "idleMs": ms }));
                        }
                        Some(false) => {
                            idle = false;
                            crate::push::to_all(&app, "active", &Value::Null);
                        }
                        None => {}
                    }
                }
            }
            if now >= next_theme {
                next_theme = now + Duration::from_secs(2);
                let current = imp::dark_mode();
                if current.is_some() && current != dark {
                    dark = current;
                    crate::push::to_all(&app, "theme-changed", &json!({ "dark": current }));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_payload() {
        let v = battery_json(1, 8, 64, false, u32::MAX);
        assert_eq!(v["present"], true);
        assert_eq!(v["percent"], 64);
        assert_eq!(v["plugged"], true);
        assert_eq!(v["charging"], true);
        assert_eq!(v["secondsLeft"], Value::Null);
        // Desktop PC: no system battery.
        let v = battery_json(1, 128, 255, false, u32::MAX);
        assert_eq!(v["present"], false);
        assert_eq!(v["percent"], Value::Null);
        assert_eq!(v["charging"], false);
        let v = battery_json(0, 1, 90, true, 7200);
        assert_eq!(v["plugged"], false);
        assert_eq!(v["saver"], true);
        assert_eq!(v["secondsLeft"], 7200);
    }

    #[test]
    fn network_levels() {
        assert_eq!(level_name(0), "none");
        assert_eq!(level_name(1), "local");
        assert_eq!(level_name(2), "constrained");
        assert_eq!(level_name(3), "internet");
    }

    #[test]
    fn idle_transitions_once_each_way() {
        assert_eq!(idle_transition(false, 1_000), None);
        assert_eq!(idle_transition(false, IDLE_AFTER_MS), Some(true));
        assert_eq!(idle_transition(true, IDLE_AFTER_MS + 5_000), None);
        assert_eq!(idle_transition(true, 10), Some(false));
    }

    #[test]
    fn sensor_names() {
        assert!(is_sensor("battery"));
        assert!(is_sensor("focus"));
        assert!(!is_sensor("media"));
        assert!(!is_sensor("pty"));
        // `notify` is read through `sense_now` but is not a polled value.
        assert!(!is_sensor("notify"));
        assert!(is_readable("notify"));
        assert!(is_readable("audio"));
        assert!(!is_readable("bus"));
        assert!(is_sensor("radios") && is_sensor("display") && is_sensor("power-mode"));
    }
}
