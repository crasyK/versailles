//! Quick settings (#32): the `radios`, `audio-control`, `display` and
//! `power-mode` hooks. Changes for `wifi` / `bluetooth` (`wireless`), `vpn`,
//! `hotspot`, `sharing` and `accessibility` (their own modules) go through
//! here too.
//!
//! Reads are sensors (`sensors::SENSORS`): `now()` answers, changes are
//! pushed. Changes go through one command, `quick_set`, in the control class:
//! refused from the wallpaper and from folder widgets, only a spawn window
//! (bar, slide-out, overlay) may change a setting, and the SDK asks for a
//! click or key press first. Every change is logged.
//!
//! Wi-Fi / Bluetooth radios, volume, mute, projection mode and brightness use
//! documented APIs. The default output (`IPolicyConfig`), the power mode
//! overlay (`PowerSetActiveOverlayScheme`), airplane mode (`IRadioManager`)
//! and night light (a CloudStore registry blob, `nightlight`) are
//! undocumented: they are feature checked, read as unsupported and fail with
//! a message instead of crashing.

use crate::error::{AppError, AppResult};
use serde::Deserialize;
use serde_json::Value;

/// A change a page asks for. `kind` picks the variant (`{ kind: "volume", level: 40 }`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QuickAction {
    /// `radio`: `"wifi"` or `"bluetooth"`.
    #[serde(rename_all = "camelCase")]
    Radio { radio: String, on: bool },
    /// Airplane mode: all radios off (`on: true`) or back on.
    Airplane { on: bool },
    /// Default output volume, 0–100.
    Volume { level: f64 },
    Mute { on: bool },
    /// Make this output (an `id` from `audio-control`'s `outputs`) the default.
    Output { id: String },
    /// `id` from `display`'s `brightness` list; `level` 0–100.
    Brightness { id: String, level: f64 },
    /// Win+P: `"pc"`, `"duplicate"`, `"extend"` or `"second"`.
    Project { mode: String },
    NightLight { on: bool },
    /// `"efficiency"`, `"balanced"` or `"performance"`.
    PowerMode { mode: String },
    /// Look for Wi-Fi networks now (the list updates a few seconds later).
    WifiScan,
    /// Join a network in range; `password` for a new secured one.
    WifiConnect { ssid: String, password: Option<String> },
    WifiDisconnect,
    /// `id` from `bluetooth`'s `devices`; audio devices only.
    BluetoothConnect { id: String },
    BluetoothDisconnect { id: String },
    /// Do Not Disturb: `"off"`, `"priority"` (priority only) or `"alarms"` (alarms only).
    Dnd { mode: String },
    /// Remove one notification (an `id` from `notify`).
    NotifyDismiss { id: u32 },
    /// Remove all notifications.
    NotifyClear,
    /// Open the app that raised a notification (its `aumid`).
    NotifyOpen { aumid: String },
    /// Turn an app's own Windows pop-ups on or off.
    NotifyBanners { aumid: String, on: bool },
    // ── connections and accessibility (#63–#66, see `vpn`, `hotspot`,
    // `sharing`, `accessibility`) ──
    /// Connect a Windows VPN profile (a `name` from `vpn`'s `profiles`).
    VpnConnect { name: String },
    VpnDisconnect { name: String },
    /// Mobile hotspot on or off.
    Hotspot { on: bool },
    /// Open Windows' Cast panel (Win+K).
    CastPanel,
    /// Nearby sharing: `"off"`, `"my-devices"` or `"everyone"`.
    Nearby { mode: String },
    /// `name`: `"magnifier"`, `"contrast"`, `"captions"`, `"colorFilter"`, `"stickyKeys"`.
    Accessibility { name: String, on: bool },
}

impl QuickAction {
    /// For the log: the action without a Wi-Fi password.
    pub fn describe(&self) -> String {
        match self {
            QuickAction::WifiConnect { ssid, password } => {
                format!("WifiConnect {{ ssid: {ssid:?}, password: {} }}", if password.is_some() { "<set>" } else { "none" })
            }
            other => format!("{other:?}"),
        }
    }

    /// The hook a piece must declare for this change.
    pub fn hook(&self) -> &'static str {
        match self {
            QuickAction::Radio { .. } | QuickAction::Airplane { .. } => "radios",
            QuickAction::Volume { .. } | QuickAction::Mute { .. } | QuickAction::Output { .. } => "audio-control",
            QuickAction::Brightness { .. } | QuickAction::Project { .. } | QuickAction::NightLight { .. } => "display",
            QuickAction::PowerMode { .. } => "power-mode",
            QuickAction::WifiScan | QuickAction::WifiConnect { .. } | QuickAction::WifiDisconnect => "wifi",
            QuickAction::BluetoothConnect { .. } | QuickAction::BluetoothDisconnect { .. } => "bluetooth",
            QuickAction::Dnd { .. } => "dnd",
            QuickAction::NotifyDismiss { .. }
            | QuickAction::NotifyClear
            | QuickAction::NotifyOpen { .. }
            | QuickAction::NotifyBanners { .. } => "notify-control",
            QuickAction::VpnConnect { .. } | QuickAction::VpnDisconnect { .. } => "vpn",
            QuickAction::Hotspot { .. } => "hotspot",
            QuickAction::CastPanel | QuickAction::Nearby { .. } => "sharing",
            QuickAction::Accessibility { .. } => "accessibility",
        }
    }
}

/// Apply a change and answer: the hook's new state, or `{ ok: true,
/// dialog: true }` / `{ ok: true, settings: true }` when Windows' own dialog
/// or Settings page opened instead (a VPN that needs credentials, turning a
/// contrast theme off). Blocking: call off the main thread.
pub fn answer(action: &QuickAction) -> AppResult<Value> {
    let opened = match action {
        QuickAction::VpnConnect { name } => crate::vpn::connect(name)?,
        QuickAction::Accessibility { name, on } => crate::accessibility::set(name, *on)?,
        other => {
            apply(other)?;
            None
        }
    };
    match opened {
        Some(value) => Ok(value),
        None => read(action.hook()),
    }
}

/// Open a URI or program the way Explorer does (`ShellExecute`), on a thread
/// of its own: quick settings run on multithreaded COM threads, and the shell
/// wants a single-threaded apartment.
pub fn shell_open(target: &str) -> AppResult<()> {
    let target = target.to_string();
    std::thread::spawn(move || crate::cli::open_target(&target))
        .join()
        .map_err(|_| AppError::msg("shell: open failed"))?
        .map_err(AppError::msg)
}

/// Current value of a quick-settings sensor.
/// Pin COM's multithreaded apartment for the life of the process (see
/// `imp::keep_mta_alive`). Called at start; quick settings call it too.
pub fn keep_com_alive() {
    #[cfg(windows)]
    imp::keep_mta_alive();
}

pub fn read(hook: &str) -> AppResult<Value> {
    match hook {
        "radios" => imp::radios(),
        "audio-control" => imp::outputs(),
        "display" => imp::display(),
        "power-mode" => imp::power_mode(),
        "wifi" | "bluetooth" => crate::wireless::read(hook),
        "dnd" => imp::dnd(),
        // Answered after a change: the notifications as `notify` reads them.
        "notify-control" => crate::toasts::read(),
        "vpn" => crate::vpn::read(),
        "hotspot" => crate::hotspot::read(),
        "sharing" => crate::sharing::read(),
        "accessibility" => crate::accessibility::read(),
        _ => Err(AppError::msg(format!("'{hook}' is not a quick-settings hook"))),
    }
}

/// Apply a change. Blocking (COM, WinRT, DDC/CI): call off the main thread.
pub fn apply(action: &QuickAction) -> AppResult<()> {
    match action {
        QuickAction::Radio { radio, on } => imp::set_radio(radio, *on),
        QuickAction::Airplane { on } => imp::set_airplane(*on),
        QuickAction::Volume { level } => imp::set_volume(level.clamp(0.0, 100.0)),
        QuickAction::Mute { on } => imp::set_mute(*on),
        QuickAction::Output { id } => imp::set_output(id),
        QuickAction::Brightness { id, level } => imp::set_brightness(id, level.clamp(0.0, 100.0).round() as u32),
        QuickAction::Project { mode } => imp::project(mode),
        QuickAction::NightLight { on } => crate::nightlight::set(*on),
        QuickAction::PowerMode { mode } => imp::set_power_mode(mode),
        QuickAction::WifiScan => crate::wireless::wifi_scan(),
        QuickAction::WifiConnect { ssid, password } => crate::wireless::wifi_connect(ssid, password.as_deref()),
        QuickAction::WifiDisconnect => crate::wireless::wifi_disconnect(),
        QuickAction::BluetoothConnect { id } => crate::wireless::bluetooth_connect(id, true),
        QuickAction::BluetoothDisconnect { id } => crate::wireless::bluetooth_connect(id, false),
        QuickAction::Dnd { mode } => imp::set_dnd(mode),
        QuickAction::NotifyDismiss { id } => crate::toasts::dismiss(*id),
        QuickAction::NotifyClear => crate::toasts::clear(),
        QuickAction::NotifyOpen { aumid } => crate::toasts::open_app(aumid),
        QuickAction::NotifyBanners { aumid, on } => crate::toasts::banners(aumid, *on),
        QuickAction::VpnConnect { name } => crate::vpn::connect(name).map(drop),
        QuickAction::VpnDisconnect { name } => crate::vpn::disconnect(name),
        QuickAction::Hotspot { on } => crate::hotspot::set(*on),
        QuickAction::CastPanel => crate::sharing::open_cast(),
        QuickAction::Nearby { mode } => crate::sharing::set_nearby(mode),
        QuickAction::Accessibility { name, on } => crate::accessibility::set(name, *on).map(drop),
    }
}

/// Do Not Disturb profiles as Windows names them, and as pages do.
const DND_PROFILES: &[(&str, &str)] = &[
    ("off", "Microsoft.QuietHoursProfile.Unrestricted"),
    ("priority", "Microsoft.QuietHoursProfile.PriorityOnly"),
    ("alarms", "Microsoft.QuietHoursProfile.AlarmsOnly"),
];

/// Power mode overlay GUIDs (Settings > Power > Power mode).
const POWER_MODES: &[(&str, u128)] = &[
    ("efficiency", 0x961cc777_2547_4f9d_8174_7d86181b8a7a),
    ("balanced", 0x00000000_0000_0000_0000_000000000000),
    ("performance", 0xded574b5_45a0_4f42_8737_46345c09c238),
];

fn power_mode_name(guid: u128) -> Option<&'static str> {
    POWER_MODES.iter().find(|(_, g)| *g == guid).map(|(n, _)| *n)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use serde_json::json;
    use windows::core::{GUID, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoIncrementMTAUsage, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    fn err(e: windows::core::Error) -> AppError {
        AppError::msg(e.to_string())
    }

    fn com() {
        keep_mta_alive();
        // Already initialised on this thread is fine.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    /// Keep the process's multithreaded apartment alive for good. Quick
    /// settings run on short-lived blocking threads: when the last of them
    /// left the MTA, COM tore it down and the WinRT factories windows-rs
    /// caches (`Radio`) pointed at nothing, so the next call crashed.
    pub fn keep_mta_alive() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            if let Err(e) = unsafe { CoIncrementMTAUsage() } {
                tracing::warn!("quick settings: could not pin the COM apartment: {e}");
            }
        });
    }

    // ── radios ─────────────────────────────────────────────────────────

    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

    fn all_radios() -> AppResult<Vec<Radio>> {
        let list = Radio::GetRadiosAsync().and_then(|op| op.get()).map_err(err)?;
        let n = list.Size().map_err(err)?;
        (0..n).map(|i| list.GetAt(i).map_err(err)).collect()
    }

    fn kind_name(kind: RadioKind) -> Option<&'static str> {
        match kind {
            RadioKind::WiFi => Some("wifi"),
            RadioKind::Bluetooth => Some("bluetooth"),
            RadioKind::MobileBroadband => Some("mobile"),
            _ => None,
        }
    }

    pub fn radios() -> AppResult<Value> {
        com();
        let mut out = serde_json::Map::new();
        for name in ["wifi", "bluetooth"] {
            out.insert(name.into(), Value::Null);
        }
        for radio in all_radios()? {
            let (Ok(kind), Ok(state)) = (radio.Kind(), radio.State()) else {
                continue;
            };
            if let Some(name) = kind_name(kind) {
                out.insert(name.into(), json!(state == RadioState::On));
            }
        }
        out.insert("airplane".into(), json!(airplane()));
        Ok(Value::Object(out))
    }

    pub fn set_radio(name: &str, on: bool) -> AppResult<()> {
        com();
        let want = name.trim().to_ascii_lowercase();
        let access = Radio::RequestAccessAsync().and_then(|op| op.get()).map_err(err)?;
        if access != RadioAccessStatus::Allowed {
            return Err(AppError::msg(format!("radios: Windows refused access ({access:?})")));
        }
        let radio = all_radios()?
            .into_iter()
            .find(|r| r.Kind().ok().and_then(kind_name) == Some(want.as_str()))
            .ok_or_else(|| AppError::msg(format!("radios: no '{want}' radio on this PC")))?;
        let state = if on { RadioState::On } else { RadioState::Off };
        let status = radio.SetStateAsync(state).and_then(|op| op.get()).map_err(err)?;
        if status != RadioAccessStatus::Allowed {
            return Err(AppError::msg(format!("radios: Windows refused the change ({status:?})")));
        }
        Ok(())
    }

    /// Windows' own airplane mode switch (undocumented; what the Quick
    /// Settings tile uses, served by RmSvc). Signatures checked against
    /// yori's `lib/airplane.c` and Lenovo Legion Toolkit's `AirplaneMode.cs`.
    mod radio_manager {
        #![allow(non_snake_case)]
        use windows::core::{BOOL, HRESULT};
        #[windows_core::interface("db3afbfb-08e6-46c6-aa70-bf9a34c30ab7")]
        pub unsafe trait IRadioManager: windows::core::IUnknown {
            fn IsRMSupported(&self, supported: *mut u32) -> HRESULT;
            fn GetUIRadioInstances(&self, list: *mut *mut core::ffi::c_void) -> HRESULT;
            fn GetSystemRadioState(&self, radios_on: *mut BOOL, changeable: *mut BOOL, reason: *mut u32) -> HRESULT;
            fn SetSystemRadioState(&self, radios_on: BOOL) -> HRESULT;
        }

        /// `IsRMSupported`'s answer. The generated methods are private to this module.
        pub unsafe fn supported(m: &IRadioManager) -> windows::core::Result<u32> {
            let mut out = 0u32;
            m.IsRMSupported(&mut out).ok()?;
            Ok(out)
        }

        /// `(radios on, changeable, reason)`.
        pub unsafe fn state(m: &IRadioManager) -> windows::core::Result<(bool, bool, u32)> {
            let (mut on, mut changeable, mut reason) = (BOOL(1), BOOL(1), 0u32);
            m.GetSystemRadioState(&mut on, &mut changeable, &mut reason).ok()?;
            Ok((on.as_bool(), changeable.as_bool(), reason))
        }

        pub unsafe fn set(m: &IRadioManager, radios_on: bool) -> windows::core::Result<()> {
            m.SetSystemRadioState(radios_on.into()).ok()
        }
    }

    const RADIO_MANAGEMENT: GUID = GUID::from_u128(0x581333f6_28db_41be_bc7a_ff201f12f3f6);

    fn radio_manager() -> Option<radio_manager::IRadioManager> {
        use windows::Win32::System::Com::CLSCTX_LOCAL_SERVER;
        com();
        let m: radio_manager::IRadioManager = unsafe { CoCreateInstance(&RADIO_MANAGEMENT, None, CLSCTX_LOCAL_SERVER) }.ok()?;
        (unsafe { radio_manager::supported(&m) }.ok()? != 0).then_some(m)
    }

    /// Whether airplane mode is on; `None` when Windows does not offer it or
    /// it cannot be changed.
    fn airplane() -> Option<bool> {
        let m = radio_manager()?;
        let (radios_on, changeable, _) = unsafe { radio_manager::state(&m) }.ok()?;
        changeable.then_some(!radios_on)
    }

    pub fn set_airplane(on: bool) -> AppResult<()> {
        let m = radio_manager().ok_or_else(|| AppError::msg("radios: airplane mode is not supported on this PC"))?;
        let (radios_on, changeable, _) =
            unsafe { radio_manager::state(&m) }.map_err(|e| AppError::msg(format!("radios: airplane mode ({e})")))?;
        if !changeable {
            return Err(AppError::msg("radios: airplane mode cannot be changed on this PC"));
        }
        if radios_on != on {
            return Ok(()); // already as asked
        }
        unsafe { radio_manager::set(&m, !on) }.map_err(|e| AppError::msg(format!("radios: Windows refused airplane mode ({e})")))
    }

    /// For `live_airplane`: the raw answers, read-only.
    #[cfg(test)]
    pub fn airplane_raw() -> String {
        use windows::Win32::System::Com::CLSCTX_LOCAL_SERVER;
        com();
        let m: windows::core::Result<radio_manager::IRadioManager> =
            unsafe { CoCreateInstance(&RADIO_MANAGEMENT, None, CLSCTX_LOCAL_SERVER) };
        match m {
            Err(e) => format!("CoCreateInstance failed: {e}"),
            Ok(m) => format!(
                "IsRMSupported: {:?}, GetSystemRadioState (radios on, changeable, reason): {:?}",
                unsafe { radio_manager::supported(&m) },
                unsafe { radio_manager::state(&m) }
            ),
        }
    }

    // ── audio-control ──────────────────────────────────────────────────

    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCommunications, eConsole, eMultimedia, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE,
    };

    fn enumerator() -> AppResult<IMMDeviceEnumerator> {
        com();
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)
    }

    fn device_id(device: &IMMDevice) -> Option<String> {
        unsafe {
            let id = device.GetId().ok()?;
            let s = id.to_string().ok();
            windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as *const _));
            s
        }
    }

    fn device_name(device: &IMMDevice) -> Option<String> {
        use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
        use windows::Win32::System::Com::STGM_READ;
        unsafe { device.OpenPropertyStore(STGM_READ).and_then(|s| s.GetValue(&PKEY_Device_FriendlyName)) }
            .ok()
            .map(|v| v.to_string())
            .filter(|s| !s.is_empty())
    }

    /// Active outputs and which one is the default.
    pub fn outputs() -> AppResult<Value> {
        let devices = enumerator()?;
        unsafe {
            let default = devices.GetDefaultAudioEndpoint(eRender, eConsole).ok().and_then(|d| device_id(&d));
            let list = devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).map_err(err)?;
            let n = list.GetCount().map_err(err)?;
            let mut outputs = Vec::new();
            for i in 0..n {
                let Ok(device) = list.Item(i) else { continue };
                let Some(id) = device_id(&device) else { continue };
                outputs.push(json!({
                    "id": id,
                    "name": device_name(&device),
                    "default": default.as_deref() == Some(id.as_str()),
                }));
            }
            Ok(json!({ "outputs": outputs }))
        }
    }

    fn default_volume() -> AppResult<IAudioEndpointVolume> {
        let devices = enumerator()?;
        unsafe {
            let device = devices
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|_| AppError::msg("audio-control: no default output"))?;
            device.Activate(CLSCTX_ALL, None).map_err(err)
        }
    }

    pub fn set_volume(level: f64) -> AppResult<()> {
        let volume = default_volume()?;
        unsafe { volume.SetMasterVolumeLevelScalar((level / 100.0) as f32, std::ptr::null()) }.map_err(err)
    }

    pub fn set_mute(on: bool) -> AppResult<()> {
        let volume = default_volume()?;
        unsafe { volume.SetMute(on, std::ptr::null()) }.map_err(err)
    }

    /// Undocumented, but what Windows' own sound settings use (Windows 7+).
    mod policy {
        #![allow(non_snake_case)]
        use windows::core::{HRESULT, PCWSTR};
        #[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
        pub unsafe trait IPolicyConfig: windows::core::IUnknown {
            fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut core::ffi::c_void) -> HRESULT;
            fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut core::ffi::c_void) -> HRESULT;
            fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
            fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut core::ffi::c_void, mix: *mut core::ffi::c_void) -> HRESULT;
            fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, a: *mut i64, b: *mut i64) -> HRESULT;
            fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
            fn GetShareMode(&self, id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
            fn SetShareMode(&self, id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
            fn GetPropertyValue(&self, id: PCWSTR, key: *const core::ffi::c_void, value: *mut core::ffi::c_void) -> HRESULT;
            fn SetPropertyValue(&self, id: PCWSTR, key: *const core::ffi::c_void, value: *const core::ffi::c_void) -> HRESULT;
            fn SetDefaultEndpoint(&self, id: PCWSTR, role: i32) -> HRESULT;
            fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
        }

        /// The generated methods are private to this module.
        pub unsafe fn set_default(p: &IPolicyConfig, id: PCWSTR, role: i32) -> HRESULT {
            p.SetDefaultEndpoint(id, role)
        }
    }
    use policy::IPolicyConfig;

    const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    pub fn set_output(id: &str) -> AppResult<()> {
        // Only ids of active outputs: never hand the undocumented call junk.
        let known = outputs()?;
        let listed = known["outputs"].as_array().is_some_and(|l| l.iter().any(|o| o["id"] == id));
        if !listed {
            return Err(AppError::msg(format!("audio-control: no active output '{id}'")));
        }
        com();
        let policy: IPolicyConfig = unsafe { CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }
            .map_err(|e| AppError::msg(format!("audio-control: default output not supported here ({e})")))?;
        let wide: Vec<u16> = id.encode_utf16().chain([0]).collect();
        for role in [eConsole, eMultimedia, eCommunications] {
            unsafe { policy::set_default(&policy, PCWSTR(wide.as_ptr()), role.0) }.ok().map_err(err)?;
        }
        Ok(())
    }

    // ── display: projection and brightness ─────────────────────────────

    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetDisplayConfigBufferSizes, GetMonitorBrightness,
        GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, QueryDisplayConfig,
        SetDisplayConfig, SetMonitorBrightness, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
        DISPLAYCONFIG_TOPOLOGY_CLONE, DISPLAYCONFIG_TOPOLOGY_EXTEND, DISPLAYCONFIG_TOPOLOGY_EXTERNAL,
        DISPLAYCONFIG_TOPOLOGY_ID, DISPLAYCONFIG_TOPOLOGY_INTERNAL, PHYSICAL_MONITOR, QDC_DATABASE_CURRENT,
        SDC_APPLY, SDC_TOPOLOGY_CLONE, SDC_TOPOLOGY_EXTEND, SDC_TOPOLOGY_EXTERNAL, SDC_TOPOLOGY_INTERNAL,
    };
    use windows::core::BOOL;
    use windows::Win32::Foundation::{ERROR_SUCCESS, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};

    fn projection() -> Option<&'static str> {
        unsafe {
            let (mut paths, mut modes) = (0u32, 0u32);
            if GetDisplayConfigBufferSizes(QDC_DATABASE_CURRENT, &mut paths, &mut modes) != ERROR_SUCCESS {
                return None;
            }
            let mut path = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
            let mut mode = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
            let mut topology = DISPLAYCONFIG_TOPOLOGY_ID::default();
            let r = QueryDisplayConfig(
                QDC_DATABASE_CURRENT,
                &mut paths,
                path.as_mut_ptr(),
                &mut modes,
                mode.as_mut_ptr(),
                Some(&mut topology),
            );
            if r != ERROR_SUCCESS {
                return None;
            }
            Some(match topology {
                DISPLAYCONFIG_TOPOLOGY_INTERNAL => "pc",
                DISPLAYCONFIG_TOPOLOGY_CLONE => "duplicate",
                DISPLAYCONFIG_TOPOLOGY_EXTEND => "extend",
                DISPLAYCONFIG_TOPOLOGY_EXTERNAL => "second",
                _ => return None,
            })
        }
    }

    pub fn project(mode: &str) -> AppResult<()> {
        let topology = match mode.trim().to_ascii_lowercase().as_str() {
            "pc" | "internal" => SDC_TOPOLOGY_INTERNAL,
            "duplicate" | "clone" => SDC_TOPOLOGY_CLONE,
            "extend" => SDC_TOPOLOGY_EXTEND,
            "second" | "external" => SDC_TOPOLOGY_EXTERNAL,
            other => return Err(AppError::msg(format!("display: unknown projection mode '{other}'"))),
        };
        let r = unsafe { SetDisplayConfig(None, None, SDC_APPLY | topology) };
        if r != 0 {
            return Err(AppError::msg(format!("display: Windows refused projection '{mode}' (error {r})")));
        }
        Ok(())
    }

    /// Every monitor handle with its GDI device name (`\\.\DISPLAY2`).
    fn monitors() -> Vec<(HMONITOR, String)> {
        unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
            let out = &mut *(data.0 as *mut Vec<(HMONITOR, String)>);
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            let name = if GetMonitorInfoW(m, &mut info as *mut _ as *mut _).as_bool() {
                let end = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
                String::from_utf16_lossy(&info.szDevice[..end])
            } else {
                String::new()
            };
            out.push((m, name));
            true.into()
        }
        let mut out: Vec<(HMONITOR, String)> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut out as *mut _ as isize));
        }
        out
    }

    /// Run `f` on every physical monitor behind a monitor handle (DDC/CI).
    fn with_physical<T>(m: HMONITOR, mut f: impl FnMut(usize, &PHYSICAL_MONITOR) -> Option<T>) -> Vec<T> {
        let mut out = Vec::new();
        unsafe {
            let mut n = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(m, &mut n).is_err() || n == 0 {
                return out;
            }
            let mut list = vec![PHYSICAL_MONITOR::default(); n as usize];
            if GetPhysicalMonitorsFromHMONITOR(m, &mut list).is_err() {
                return out;
            }
            for (i, p) in list.iter().enumerate() {
                if let Some(v) = f(i, p) {
                    out.push(v);
                }
            }
            let _ = DestroyPhysicalMonitors(&list);
        }
        out
    }

    /// External monitors over DDC/CI: `ddc:\\.\DISPLAY2:0`.
    fn ddc_brightness() -> Vec<Value> {
        let mut out = Vec::new();
        for (m, device) in monitors() {
            out.extend(with_physical(m, |i, p| {
                let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
                if unsafe { GetMonitorBrightness(p.hPhysicalMonitor, &mut min, &mut cur, &mut max) } == 0 || max <= min {
                    return None;
                }
                let level = ((cur - min) as f64 * 100.0 / (max - min) as f64).round();
                Some(json!({ "id": format!("ddc:{device}:{i}"), "device": device, "kind": "external", "level": level }))
            }));
        }
        out
    }

    fn set_ddc(device: &str, index: usize, level: u32) -> AppResult<()> {
        let Some((m, _)) = monitors().into_iter().find(|(_, d)| d == device) else {
            return Err(AppError::msg(format!("display: no monitor '{device}'")));
        };
        let done = with_physical(m, |i, p| {
            if i != index {
                return None;
            }
            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            unsafe {
                if GetMonitorBrightness(p.hPhysicalMonitor, &mut min, &mut cur, &mut max) == 0 || max <= min {
                    return Some(false);
                }
                let raw = min + ((max - min) as f64 * level as f64 / 100.0).round() as u32;
                Some(SetMonitorBrightness(p.hPhysicalMonitor, raw) != 0)
            }
        });
        match done.first() {
            Some(true) => Ok(()),
            _ => Err(AppError::msg(format!("display: '{device}' does not take brightness over DDC/CI"))),
        }
    }

    // Built-in panels: WMI root\WMI WmiMonitorBrightness(Methods).
    use windows::core::BSTR;
    use windows::Win32::System::Com::{CoSetProxyBlanket, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE};
    use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
    use windows::Win32::System::Variant::{VARIANT, VT_I4, VT_UI1};
    use windows::Win32::System::Wmi::{
        IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
        WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
    };

    fn wmi() -> AppResult<IWbemServices> {
        com();
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_ALL).map_err(err)?;
            let services = locator
                .ConnectServer(&BSTR::from("ROOT\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)
                .map_err(err)?;
            CoSetProxyBlanket(
                &services,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )
            .map_err(err)?;
            Ok(services)
        }
    }

    fn query(services: &IWbemServices, wql: &str) -> AppResult<Vec<IWbemClassObject>> {
        unsafe {
            let rows = services
                .ExecQuery(
                    &BSTR::from("WQL"),
                    &BSTR::from(wql),
                    WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                    None,
                )
                .map_err(err)?;
            let mut out = Vec::new();
            loop {
                let mut row = [None];
                let mut got = 0u32;
                let _ = rows.Next(WBEM_INFINITE, &mut row, &mut got);
                match row[0].take() {
                    Some(obj) if got == 1 => out.push(obj),
                    _ => break,
                }
            }
            Ok(out)
        }
    }

    fn prop(obj: &IWbemClassObject, name: &str) -> Option<VARIANT> {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let mut value = VARIANT::default();
        unsafe { obj.Get(PCWSTR(wide.as_ptr()), 0, &mut value, None, None) }.ok()?;
        Some(value)
    }

    fn internal_brightness() -> Vec<Value> {
        let Ok(services) = wmi() else {
            return Vec::new();
        };
        let Ok(rows) = query(&services, "SELECT InstanceName, CurrentBrightness FROM WmiMonitorBrightness WHERE Active=TRUE") else {
            return Vec::new();
        };
        rows.iter()
            .filter_map(|row| {
                let name = prop(row, "InstanceName").and_then(|v| BSTR::try_from(&v).ok())?.to_string();
                let level = prop(row, "CurrentBrightness").and_then(|v| u32::try_from(&v).ok())?;
                Some(json!({ "id": format!("wmi:{name}"), "device": null, "kind": "internal", "level": level }))
            })
            .collect()
    }

    fn small_variant(vt: windows::Win32::System::Variant::VARENUM, value: i32) -> VARIANT {
        let mut v = VARIANT::default();
        unsafe {
            let inner = &mut *v.Anonymous.Anonymous;
            inner.vt = vt;
            if vt == VT_UI1 {
                inner.Anonymous.bVal = value as u8;
            } else {
                inner.Anonymous.lVal = value;
            }
        }
        v
    }

    fn set_wmi(instance: &str, level: u32) -> AppResult<()> {
        let services = wmi()?;
        let safe = instance.replace('\\', "\\\\").replace('\'', "");
        let rows = query(
            &services,
            &format!("SELECT * FROM WmiMonitorBrightnessMethods WHERE InstanceName='{safe}'"),
        )?;
        let row = rows.first().ok_or_else(|| AppError::msg("display: built-in panel not found"))?;
        let path = prop(row, "__PATH")
            .and_then(|v| BSTR::try_from(&v).ok())
            .ok_or_else(|| AppError::msg("display: no WMI path for the panel"))?;
        unsafe {
            let mut class: Option<IWbemClassObject> = None;
            services
                .GetObject(&BSTR::from("WmiMonitorBrightnessMethods"), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None)
                .map_err(err)?;
            let class = class.ok_or_else(|| AppError::msg("display: WmiMonitorBrightnessMethods missing"))?;
            let mut sig: Option<IWbemClassObject> = None;
            class
                .GetMethod(windows::core::w!("WmiSetBrightness"), 0, &mut sig, std::ptr::null_mut())
                .map_err(err)?;
            let params = sig
                .ok_or_else(|| AppError::msg("display: WmiSetBrightness missing"))?
                .SpawnInstance(0)
                .map_err(err)?;
            params.Put(windows::core::w!("Timeout"), 0, &small_variant(VT_I4, 0), 0).map_err(err)?;
            params
                .Put(windows::core::w!("Brightness"), 0, &small_variant(VT_UI1, level.min(100) as i32), 0)
                .map_err(err)?;
            services
                .ExecMethod(&path, &BSTR::from("WmiSetBrightness"), WBEM_GENERIC_FLAG_TYPE(0), None, &params, None, None)
                .map_err(err)?;
        }
        Ok(())
    }

    pub fn display() -> AppResult<Value> {
        let mut brightness = internal_brightness();
        brightness.extend(ddc_brightness());
        Ok(json!({ "projection": projection(), "brightness": brightness, "nightLight": crate::nightlight::read() }))
    }

    pub fn set_brightness(id: &str, level: u32) -> AppResult<()> {
        if let Some(instance) = id.strip_prefix("wmi:") {
            return set_wmi(instance, level);
        }
        if let Some(rest) = id.strip_prefix("ddc:") {
            if let Some((device, index)) = rest.rsplit_once(':') {
                if let Ok(index) = index.parse() {
                    return set_ddc(device, index, level);
                }
            }
        }
        Err(AppError::msg(format!("display: unknown brightness id '{id}'")))
    }

    // ── dnd ────────────────────────────────────────────────────────────

    /// The shell's Do Not Disturb settings (undocumented; what yasb uses).
    mod quiet {
        #![allow(non_snake_case)]
        use windows::core::{HRESULT, PCWSTR, PWSTR};
        #[windows_core::interface("6bff4732-81ec-4ffb-ae67-b6c1bc29631f")]
        pub unsafe trait IQuietHoursSettings: windows::core::IUnknown {
            fn get_UserSelectedProfile(&self, profile: *mut PWSTR) -> HRESULT;
            fn put_UserSelectedProfile(&self, profile: PCWSTR) -> HRESULT;
        }

        /// The generated methods are private to this module.
        pub unsafe fn get(s: &IQuietHoursSettings) -> windows::core::Result<String> {
            let mut out = PWSTR::null();
            s.get_UserSelectedProfile(&mut out).ok()?;
            let text = out.to_string().unwrap_or_default();
            windows::Win32::System::Com::CoTaskMemFree(Some(out.0 as *const _));
            Ok(text)
        }

        pub unsafe fn put(s: &IQuietHoursSettings, profile: &str) -> windows::core::Result<()> {
            let wide: Vec<u16> = profile.encode_utf16().chain([0]).collect();
            s.put_UserSelectedProfile(PCWSTR(wide.as_ptr())).ok()
        }
    }

    const QUIET_HOURS_SETTINGS: GUID = GUID::from_u128(0xf53321fa_34f8_4b7f_b9a3_361877cb94cf);

    fn quiet_hours() -> Option<quiet::IQuietHoursSettings> {
        use windows::Win32::System::Com::CLSCTX_LOCAL_SERVER;
        com();
        unsafe { CoCreateInstance(&QUIET_HOURS_SETTINGS, None, CLSCTX_LOCAL_SERVER) }.ok()
    }

    pub fn dnd() -> AppResult<Value> {
        let profile = quiet_hours().and_then(|s| unsafe { quiet::get(&s) }.ok());
        let mode = profile
            .as_deref()
            .and_then(|p| DND_PROFILES.iter().find(|(_, w)| *w == p).map(|(n, _)| *n));
        Ok(json!({ "supported": profile.is_some(), "mode": mode }))
    }

    pub fn set_dnd(mode: &str) -> AppResult<()> {
        let want = mode.trim().to_ascii_lowercase();
        let profile = DND_PROFILES
            .iter()
            .find(|(n, _)| *n == want)
            .map(|(_, p)| *p)
            .ok_or_else(|| AppError::msg(format!("dnd: unknown mode '{want}' (off, priority, alarms)")))?;
        let settings = quiet_hours().ok_or_else(|| AppError::msg("dnd: not supported on this Windows"))?;
        unsafe { quiet::put(&settings, profile) }.map_err(|e| AppError::msg(format!("dnd: Windows refused ({e})")))
    }

    // ── power-mode ─────────────────────────────────────────────────────

    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    type GetOverlay = unsafe extern "system" fn(*mut GUID) -> u32;
    type SetOverlay = unsafe extern "system" fn(GUID) -> u32;

    /// `powrprof.dll` exports behind Settings' power mode slider.
    fn overlay_fns() -> Option<(GetOverlay, SetOverlay)> {
        unsafe {
            let lib = LoadLibraryW(windows::core::w!("powrprof.dll")).ok()?;
            let get = GetProcAddress(lib, windows::core::s!("PowerGetEffectiveOverlayScheme"))?;
            let set = GetProcAddress(lib, windows::core::s!("PowerSetActiveOverlayScheme"))?;
            Some((std::mem::transmute::<_, GetOverlay>(get), std::mem::transmute::<_, SetOverlay>(set)))
        }
    }

    pub fn power_mode() -> AppResult<Value> {
        let mut status = SYSTEM_POWER_STATUS::default();
        let saver = unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.SystemStatusFlag == 1;
        let mode = overlay_fns().and_then(|(get, _)| {
            let mut guid = GUID::zeroed();
            (unsafe { get(&mut guid) } == 0).then(|| power_mode_name(guid.to_u128()))
        });
        Ok(json!({
            "supported": mode.is_some(),
            "mode": mode.flatten(),
            "energySaver": saver,
        }))
    }

    pub fn set_power_mode(mode: &str) -> AppResult<()> {
        let want = mode.trim().to_ascii_lowercase();
        let guid = POWER_MODES
            .iter()
            .find(|(n, _)| *n == want)
            .map(|(_, g)| GUID::from_u128(*g))
            .ok_or_else(|| AppError::msg(format!("power-mode: unknown mode '{want}'")))?;
        let (_, set) = overlay_fns().ok_or_else(|| AppError::msg("power-mode: not supported on this Windows"))?;
        let r = unsafe { set(guid) };
        if r != 0 {
            return Err(AppError::msg(format!(
                "power-mode: Windows refused '{want}' (error {r}; the power plan must be Balanced)"
            )));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    fn unavailable<T>() -> AppResult<T> {
        Err(AppError::msg("quick settings are only available on Windows"))
    }
    pub fn radios() -> AppResult<Value> {
        unavailable()
    }
    pub fn set_radio(_: &str, _: bool) -> AppResult<()> {
        unavailable()
    }
    pub fn set_airplane(_: bool) -> AppResult<()> {
        unavailable()
    }
    pub fn outputs() -> AppResult<Value> {
        unavailable()
    }
    pub fn set_volume(_: f64) -> AppResult<()> {
        unavailable()
    }
    pub fn set_mute(_: bool) -> AppResult<()> {
        unavailable()
    }
    pub fn set_output(_: &str) -> AppResult<()> {
        unavailable()
    }
    pub fn display() -> AppResult<Value> {
        unavailable()
    }
    pub fn set_brightness(_: &str, _: u32) -> AppResult<()> {
        unavailable()
    }
    pub fn project(_: &str) -> AppResult<()> {
        unavailable()
    }
    pub fn power_mode() -> AppResult<Value> {
        unavailable()
    }
    pub fn set_power_mode(_: &str) -> AppResult<()> {
        unavailable()
    }
    pub fn dnd() -> AppResult<Value> {
        unavailable()
    }
    pub fn set_dnd(_: &str) -> AppResult<()> {
        unavailable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_parse_and_name_their_hook() {
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "radio", "radio": "wifi", "on": false }"#).unwrap();
        assert_eq!(a.hook(), "radios");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "volume", "level": 40 }"#).unwrap();
        assert_eq!(a.hook(), "audio-control");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "project", "mode": "extend" }"#).unwrap();
        assert_eq!(a.hook(), "display");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "nightLight", "on": true }"#).unwrap();
        assert_eq!(a.hook(), "display");
        assert_eq!(a.describe(), "NightLight { on: true }");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "airplane", "on": false }"#).unwrap();
        assert_eq!(a.hook(), "radios");
        assert_eq!(a.describe(), "Airplane { on: false }");
        assert!(serde_json::from_str::<QuickAction>(r#"{ "kind": "airplane" }"#).is_err());
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "powerMode", "mode": "balanced" }"#).unwrap();
        assert_eq!(a.hook(), "power-mode");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "wifiConnect", "ssid": "Home" }"#).unwrap();
        assert_eq!(a.hook(), "wifi");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "bluetoothDisconnect", "id": "{x}" }"#).unwrap();
        assert_eq!(a.hook(), "bluetooth");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "dnd", "mode": "priority" }"#).unwrap();
        assert_eq!(a.hook(), "dnd");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "notifyDismiss", "id": 7 }"#).unwrap();
        assert_eq!(a.hook(), "notify-control");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "notifyClear" }"#).unwrap();
        assert_eq!(a.hook(), "notify-control");
        assert!(serde_json::from_str::<QuickAction>(r#"{ "kind": "reboot" }"#).is_err());
    }

    #[test]
    fn connection_and_accessibility_actions() {
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "vpnConnect", "name": "Work" }"#).unwrap();
        assert_eq!(a.hook(), "vpn");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "vpnDisconnect", "name": "Work" }"#).unwrap();
        assert_eq!(a.hook(), "vpn");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "hotspot", "on": true }"#).unwrap();
        assert_eq!(a.hook(), "hotspot");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "castPanel" }"#).unwrap();
        assert_eq!(a.hook(), "sharing");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "nearby", "mode": "everyone" }"#).unwrap();
        assert_eq!(a.hook(), "sharing");
        let a: QuickAction = serde_json::from_str(r#"{ "kind": "accessibility", "name": "stickyKeys", "on": true }"#).unwrap();
        assert_eq!(a.hook(), "accessibility");
        assert!(serde_json::from_str::<QuickAction>(r#"{ "kind": "hotspot" }"#).is_err());
        // Every hook a change names is readable, so `answer` can read it back.
        for hook in ["vpn", "hotspot", "sharing", "accessibility"] {
            assert!(crate::sensors::is_sensor(hook), "{hook}");
            assert!(crate::page::KNOWN_HOOKS.contains(&hook), "{hook}");
        }
    }

    /// `cargo test live_reads -- --ignored --nocapture`: what this PC answers.
    #[test]
    #[ignore]
    fn live_reads() {
        for hook in ["radios", "audio-control", "display", "power-mode", "dnd"] {
            let t = std::time::Instant::now();
            let value = read(hook).map(|v| v.to_string());
            println!("{hook} ({} ms): {value:?}", t.elapsed().as_millis());
        }
    }

    /// `cargo test live_airplane -- --ignored --nocapture`: read-only, what
    /// `IRadioManager` answers here. Never switches airplane mode.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_airplane() {
        println!("{}", imp::airplane_raw());
        println!("radios: {:?}", read("radios").map(|v| v.to_string()));
    }

    /// `cargo test live_dnd -- --ignored --nocapture`: switch to priority
    /// only and back to what it was.
    #[test]
    #[ignore]
    fn live_dnd() {
        let before = read("dnd").unwrap();
        println!("dnd before: {before}");
        println!("set priority: {:?}", apply(&QuickAction::Dnd { mode: "priority".into() }));
        std::thread::sleep(std::time::Duration::from_millis(500));
        println!("dnd now: {}", read("dnd").unwrap());
        let back = before["mode"].as_str().unwrap_or("off").to_string();
        println!("restore {back}: {:?}", apply(&QuickAction::Dnd { mode: back.clone() }));
        println!("dnd after: {}", read("dnd").unwrap());
    }

    #[test]
    fn power_mode_names() {
        assert_eq!(power_mode_name(0), Some("balanced"));
        assert_eq!(power_mode_name(0xded574b5_45a0_4f42_8737_46345c09c238), Some("performance"));
        assert_eq!(power_mode_name(1), None);
    }
}
