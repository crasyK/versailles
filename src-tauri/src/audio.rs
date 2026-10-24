//! Microphone and per-app volume (#61, #62): the capture half of `audio` and
//! `audio-control`, and the volume mixer.
//!
//! - `input()`: the default capture endpoint (eConsole) for the `audio`
//!   sensor: `{ device, volume, muted }`, `null` without a microphone.
//! - `inputs()`: active capture endpoints for `audio-control`, like its
//!   `outputs`. The default input is set through the same undocumented
//!   `IPolicyConfig` call as the default output (`quick::set_default_endpoint`).
//! - `sessions()`: the audio sessions on the default output, as Windows'
//!   volume mixer lists them (`IAudioSessionManager2`, documented). An `id` is
//!   the session instance identifier: stable while the session lives.
//!
//! Volume and mute changes go through `quick_set` (control class), like the
//! rest of `audio-control`.

use crate::error::{AppError, AppResult};
use serde_json::Value;

/// What the mixer calls the system sounds session.
pub const SYSTEM_SOUNDS: &str = "System sounds";

/// A volume scalar (0.0–1.0) as a page sees it: 0–100.
pub fn percent(scalar: f32) -> u32 {
    if !scalar.is_finite() {
        return 0;
    }
    (scalar as f64 * 100.0).round().clamp(0.0, 100.0) as u32
}

/// A page's 0–100 level as a volume scalar (0.0–1.0).
pub fn scalar(level: f64) -> f32 {
    if !level.is_finite() {
        return 0.0;
    }
    (level.clamp(0.0, 100.0) / 100.0) as f32
}

/// Whether the mixer lists a session: never an expired one, and the system
/// sounds session only when it has a display name (as Windows' mixer).
pub fn listed_session(expired: bool, system: bool, display_name: Option<&str>) -> bool {
    !expired && (!system || display_name.is_some_and(|n| !n.trim().is_empty()))
}

/// A session's label: "System sounds", else the name the app gave its
/// session, the executable's file description, or its file name.
/// `display_name` comes resolved (an `@dll,-id` string that could not be
/// resolved is ignored).
pub fn session_app(system: bool, display_name: Option<&str>, description: Option<&str>, exe: Option<&str>) -> Option<String> {
    if system {
        return Some(SYSTEM_SOUNDS.to_string());
    }
    let named = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty() && !s.starts_with('@')).map(String::from);
    named(display_name).or_else(|| named(description)).or_else(|| {
        exe.and_then(|p| p.rsplit(['\\', '/']).next())
            .map(|f| f.strip_suffix(".exe").or_else(|| f.strip_suffix(".EXE")).unwrap_or(f))
            .filter(|s| !s.is_empty())
            .map(String::from)
    })
}

/// Mixer order: apps by name, the system sounds last; ties by id so the
/// order is stable between reads.
pub fn sort_sessions(sessions: &mut [Value]) {
    let key = |v: &Value| {
        (
            v["system"].as_bool().unwrap_or(false),
            v["app"].as_str().unwrap_or("").to_lowercase(),
            v["id"].as_str().unwrap_or("").to_string(),
        )
    };
    sessions.sort_by_key(key);
}

/// Whether `list` (an array of `{ id, … }`) holds `id`.
pub fn lists_id(list: &Value, id: &str) -> bool {
    list.as_array().is_some_and(|l| l.iter().any(|o| o["id"] == id))
}

/// The default microphone for the `audio` sensor; `null` when there is none.
pub fn input() -> Value {
    imp::input().unwrap_or(Value::Null)
}

/// Active capture endpoints `[{ id, name, default }]`; empty when they cannot be read.
pub fn inputs() -> Value {
    imp::inputs().unwrap_or_else(|e| {
        tracing::debug!("audio-control: inputs: {e}");
        Value::Array(Vec::new())
    })
}

/// The default output's audio sessions; empty when they cannot be read.
pub fn sessions() -> Value {
    imp::sessions().unwrap_or_else(|e| {
        tracing::debug!("audio-control: sessions: {e}");
        Value::Array(Vec::new())
    })
}

pub fn set_input_volume(level: f64) -> AppResult<()> {
    imp::set_input_volume(scalar(level))
}

pub fn set_input_mute(on: bool) -> AppResult<()> {
    imp::set_input_mute(on)
}

/// Make an active input (an `id` from `inputs`) the default for all roles.
pub fn set_default_input(id: &str) -> AppResult<()> {
    // Only ids of active inputs: never hand the undocumented call junk.
    if !lists_id(&imp::inputs()?, id) {
        return Err(AppError::msg(format!("audio-control: no active input '{id}'")));
    }
    crate::quick::set_default_endpoint(id)
}

pub fn set_session_volume(id: &str, level: f64) -> AppResult<()> {
    imp::set_session(id, Some(scalar(level)), None)
}

pub fn set_session_mute(id: &str, on: bool) -> AppResult<()> {
    imp::set_session(id, None, Some(on))
}

#[cfg(windows)]
mod imp {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use windows::core::{Interface, PCWSTR, PWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl2,
        IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

    fn err(e: windows::core::Error) -> AppError {
        AppError::msg(e.to_string())
    }

    fn enumerator() -> AppResult<IMMDeviceEnumerator> {
        crate::quick::keep_com_alive();
        unsafe {
            // Already initialised on this thread is fine.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err)
        }
    }

    /// A COM-allocated string, freed.
    unsafe fn take(p: PWSTR) -> Option<String> {
        if p.is_null() {
            return None;
        }
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }

    fn device_id(device: &IMMDevice) -> Option<String> {
        unsafe { device.GetId().ok().and_then(|id| take(id)) }
    }

    fn device_name(device: &IMMDevice) -> Option<String> {
        use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
        use windows::Win32::System::Com::STGM_READ;
        unsafe { device.OpenPropertyStore(STGM_READ).and_then(|s| s.GetValue(&PKEY_Device_FriendlyName)) }
            .ok()
            .map(|v| v.to_string())
            .filter(|s| !s.is_empty())
    }

    // ── microphone ─────────────────────────────────────────────────────

    fn default_input() -> AppResult<Option<IMMDevice>> {
        let devices = enumerator()?;
        Ok(unsafe { devices.GetDefaultAudioEndpoint(eCapture, eConsole) }.ok())
    }

    fn default_input_volume() -> AppResult<IAudioEndpointVolume> {
        let device = default_input()?.ok_or_else(|| AppError::msg("audio-control: no microphone"))?;
        unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(err)
    }

    pub fn input() -> AppResult<Value> {
        let Some(device) = default_input()? else {
            return Ok(Value::Null);
        };
        let volume: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(err)?;
        let (level, muted) = unsafe { (volume.GetMasterVolumeLevelScalar().map_err(err)?, volume.GetMute().map_err(err)?) };
        Ok(json!({ "device": device_name(&device), "volume": percent(level), "muted": muted.as_bool() }))
    }

    pub fn inputs() -> AppResult<Value> {
        let devices = enumerator()?;
        unsafe {
            let default = devices.GetDefaultAudioEndpoint(eCapture, eConsole).ok().and_then(|d| device_id(&d));
            let list = devices.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE).map_err(err)?;
            let n = list.GetCount().map_err(err)?;
            let mut out = Vec::new();
            for i in 0..n {
                let Ok(device) = list.Item(i) else { continue };
                let Some(id) = device_id(&device) else { continue };
                out.push(json!({
                    "id": id,
                    "name": device_name(&device),
                    "default": default.as_deref() == Some(id.as_str()),
                }));
            }
            Ok(Value::Array(out))
        }
    }

    pub fn set_input_volume(level: f32) -> AppResult<()> {
        let volume = default_input_volume()?;
        unsafe { volume.SetMasterVolumeLevelScalar(level, std::ptr::null()) }.map_err(err)
    }

    pub fn set_input_mute(on: bool) -> AppResult<()> {
        let volume = default_input_volume()?;
        unsafe { volume.SetMute(on, std::ptr::null()) }.map_err(err)
    }

    // ── sessions ───────────────────────────────────────────────────────

    /// `System.FileDescription`.
    const PKEY_FILE_DESCRIPTION: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0x0cef7d53_fa64_11d1_a203_0000f81fedee),
        pid: 3,
    };

    /// Per executable: its file description and icon (data URL). Kept for
    /// executables that still have a session.
    type Looks = (Option<String>, Option<String>);
    fn looks_cache() -> &'static Mutex<HashMap<String, Looks>> {
        static CACHE: OnceLock<Mutex<HashMap<String, Looks>>> = OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn file_description(path: &str) -> Option<String> {
        use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};
        let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
        unsafe {
            let item: IShellItem2 = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
            take(item.GetString(&PKEY_FILE_DESCRIPTION).ok()?)
        }
    }

    fn looks(path: &str) -> Looks {
        if let Some(hit) = looks_cache().lock().unwrap().get(path) {
            return hit.clone();
        }
        let found = (file_description(path), crate::app_windows::exe_icon_url(path));
        looks_cache().lock().unwrap().insert(path.to_string(), found.clone());
        found
    }

    /// `@%SystemRoot%\System32\AudioSrv.Dll,-202` → its text.
    fn resolve(name: String) -> Option<String> {
        if !name.starts_with('@') {
            return Some(name);
        }
        use windows::Win32::UI::Shell::SHLoadIndirectString;
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let mut buf = [0u16; 512];
        unsafe { SHLoadIndirectString(PCWSTR(wide.as_ptr()), &mut buf, None) }.ok()?;
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    /// The default output's sessions that are not expired, with their controls.
    fn live_sessions() -> AppResult<Vec<(IAudioSessionControl2, bool)>> {
        let devices = enumerator()?;
        unsafe {
            let device = devices
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|_| AppError::msg("audio-control: no default output"))?;
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None).map_err(err)?;
            let list = manager.GetSessionEnumerator().map_err(err)?;
            let n = list.GetCount().map_err(err)?;
            let mut out = Vec::new();
            for i in 0..n {
                let Ok(control) = list.GetSession(i) else { continue };
                let Ok(state) = control.GetState() else { continue };
                if state == AudioSessionStateExpired {
                    continue;
                }
                let Ok(control) = control.cast::<IAudioSessionControl2>() else { continue };
                out.push((control, state == AudioSessionStateActive));
            }
            Ok(out)
        }
    }

    fn instance_id(control: &IAudioSessionControl2) -> Option<String> {
        unsafe { control.GetSessionInstanceIdentifier().ok().and_then(|p| take(p)) }
    }

    pub fn sessions() -> AppResult<Value> {
        let mut out = Vec::new();
        let mut exes = Vec::new();
        for (control, active) in live_sessions()? {
            unsafe {
                let Some(id) = instance_id(&control) else { continue };
                // S_OK: the system sounds session; S_FALSE: an app's.
                let system = control.IsSystemSoundsSession().0 == 0;
                let display = control.GetDisplayName().ok().and_then(|p| take(p)).filter(|s| !s.is_empty());
                if !listed_session(false, system, display.as_deref()) {
                    continue;
                }
                let display = display.and_then(resolve);
                let pid = control.GetProcessId().unwrap_or(0);
                let exe = if system || pid == 0 { None } else { crate::app_windows::process_path(pid) };
                let (description, icon) = match exe.as_deref() {
                    Some(path) => looks(path),
                    None => (None, None),
                };
                if let Some(path) = &exe {
                    exes.push(path.clone());
                }
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else { continue };
                let level = volume.GetMasterVolume().unwrap_or(0.0);
                let muted = volume.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                out.push(json!({
                    "id": id,
                    "app": session_app(system, display.as_deref(), description.as_deref(), exe.as_deref()),
                    "exe": exe,
                    "pid": pid,
                    "icon": icon,
                    "volume": percent(level),
                    "muted": muted,
                    "active": active,
                    "system": system,
                }));
            }
        }
        looks_cache().lock().unwrap().retain(|path, _| exes.contains(path));
        sort_sessions(&mut out);
        Ok(Value::Array(out))
    }

    pub fn set_session(id: &str, level: Option<f32>, mute: Option<bool>) -> AppResult<()> {
        let control = live_sessions()?
            .into_iter()
            .map(|(c, _)| c)
            .find(|c| instance_id(c).as_deref() == Some(id))
            .ok_or_else(|| AppError::msg(format!("audio-control: no audio session '{id}'")))?;
        let volume: ISimpleAudioVolume = control.cast().map_err(err)?;
        unsafe {
            if let Some(level) = level {
                volume.SetMasterVolume(level, std::ptr::null()).map_err(err)?;
            }
            if let Some(on) = mute {
                volume.SetMute(on, std::ptr::null()).map_err(err)?;
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    fn unavailable<T>() -> AppResult<T> {
        Err(AppError::msg("audio-control is only available on Windows"))
    }
    pub fn input() -> AppResult<Value> {
        unavailable()
    }
    pub fn inputs() -> AppResult<Value> {
        unavailable()
    }
    pub fn sessions() -> AppResult<Value> {
        unavailable()
    }
    pub fn set_input_volume(_: f32) -> AppResult<()> {
        unavailable()
    }
    pub fn set_input_mute(_: bool) -> AppResult<()> {
        unavailable()
    }
    pub fn set_session(_: &str, _: Option<f32>, _: Option<bool>) -> AppResult<()> {
        unavailable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn volume_levels_round_and_clamp() {
        assert_eq!(percent(0.0), 0);
        assert_eq!(percent(0.444), 44);
        assert_eq!(percent(0.446), 45);
        assert_eq!(percent(1.0), 100);
        assert_eq!(percent(1.2), 100);
        assert_eq!(percent(-0.1), 0);
        assert_eq!(percent(f32::NAN), 0);
        assert_eq!(scalar(40.0), 0.4);
        assert_eq!(scalar(150.0), 1.0);
        assert_eq!(scalar(-3.0), 0.0);
        assert_eq!(scalar(f64::NAN), 0.0);
        assert_eq!(percent(scalar(73.0)), 73);
    }

    #[test]
    fn mixer_lists_live_sessions_and_named_system_sounds() {
        assert!(listed_session(false, false, None));
        assert!(!listed_session(true, false, Some("Spotify")));
        assert!(!listed_session(false, true, None));
        assert!(!listed_session(false, true, Some("  ")));
        assert!(listed_session(false, true, Some("@%SystemRoot%\\System32\\AudioSrv.Dll,-202")));
    }

    #[test]
    fn session_labels() {
        assert_eq!(session_app(true, Some("Systemsounds"), None, None).as_deref(), Some(SYSTEM_SOUNDS));
        let exe = Some("C:\\Program Files\\Mozilla Firefox\\firefox.exe");
        assert_eq!(session_app(false, Some("Game"), Some("Firefox"), exe).as_deref(), Some("Game"));
        assert_eq!(session_app(false, Some(""), Some("Firefox"), exe).as_deref(), Some("Firefox"));
        assert_eq!(session_app(false, Some("@x.dll,-1"), Some(" "), exe).as_deref(), Some("firefox"));
        assert_eq!(session_app(false, None, None, Some("C:\\Apps\\TOOL.EXE")).as_deref(), Some("TOOL"));
        assert_eq!(session_app(false, None, None, None), None);
    }

    #[test]
    fn sessions_sort_by_app_with_system_sounds_last() {
        let mut list = vec![
            json!({ "id": "c", "app": SYSTEM_SOUNDS, "system": true }),
            json!({ "id": "b", "app": "spotify", "system": false }),
            json!({ "id": "a2", "app": "Chrome", "system": false }),
            json!({ "id": "a1", "app": "chrome", "system": false }),
        ];
        sort_sessions(&mut list);
        let ids: Vec<&str> = list.iter().map(|v| v["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["a1", "a2", "b", "c"]);
    }

    #[test]
    fn ids_are_checked_against_the_list() {
        let list = json!([{ "id": "{0.0.1.00000000}.{a}" }, { "id": "{0.0.1.00000000}.{b}" }]);
        assert!(lists_id(&list, "{0.0.1.00000000}.{b}"));
        assert!(!lists_id(&list, "{0.0.1.00000000}.{c}"));
        assert!(!lists_id(&Value::Null, "x"));
    }

    /// `cargo test live_audio -- --ignored --nocapture`: read-only, what
    /// this PC answers for the microphone, inputs and sessions.
    #[test]
    #[ignore]
    fn live_audio() {
        for (name, read) in [("input", input as fn() -> Value), ("inputs", inputs), ("sessions", sessions)] {
            let t = std::time::Instant::now();
            let mut value = read();
            // Icons are long: show whether there is one.
            if let Some(list) = value.as_array_mut() {
                for s in list {
                    if let Some(icon) = s.get_mut("icon") {
                        *icon = json!(icon.as_str().map(|u| format!("<{} bytes>", u.len())));
                    }
                }
            }
            println!("{name} ({} ms): {value:#}", t.elapsed().as_millis());
        }
        let t = std::time::Instant::now();
        let _ = sessions();
        println!("sessions again, cached ({} ms)", t.elapsed().as_millis());
    }
}
