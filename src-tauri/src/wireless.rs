//! Wi-Fi networks and Bluetooth devices (#34): the `wifi` and `bluetooth`
//! hooks. Reads are sensors (see `sensors::SENSORS`); changes go through
//! `quick_set` (control class, see `quick`).
//!
//! - Wi-Fi uses the WLAN API. Since Windows 11 24H2 the network list needs the
//!   user's location permission; without it the read says
//!   `access: "location-denied"` instead of returning an empty list.
//! - Bluetooth lists paired devices (classic and LE) with their connection
//!   state. Windows has no public "connect" call; audio devices (headphones,
//!   speakers) are connected and disconnected through their audio driver's
//!   `KSPROPSETID_BtAudio` one-shot properties, the way ToothTray does it.
//! - Pairing a new device (#69): a scan (device watcher for unpaired
//!   association endpoints), then custom pairing, with the PIN to compare
//!   or type handed to the page step by step.

use crate::error::{AppError, AppResult};
use serde_json::Value;

pub fn read(hook: &str) -> AppResult<Value> {
    match hook {
        "wifi" => imp::wifi(),
        "bluetooth" => imp::bluetooth(),
        _ => Err(AppError::msg(format!("'{hook}' is not a wireless hook"))),
    }
}

pub fn wifi_scan() -> AppResult<()> {
    imp::wifi_scan()
}

pub fn wifi_connect(ssid: &str, password: Option<&str>) -> AppResult<()> {
    imp::wifi_connect(ssid, password)
}

pub fn wifi_disconnect() -> AppResult<()> {
    imp::wifi_disconnect()
}

pub fn bluetooth_connect(id: &str, on: bool) -> AppResult<()> {
    imp::bluetooth_connect(id, on)
}

/// How long a Bluetooth scan listens by default, and its bounds (seconds).
pub const SCAN_SECONDS: (f64, f64, f64) = (10.0, 3.0, 30.0);

/// Look for Bluetooth devices that can be paired, for `seconds` (see
/// [`SCAN_SECONDS`]): `{ devices: [{ id, name, kind, signal }] }`.
pub fn bluetooth_scan(seconds: Option<f64>) -> AppResult<Value> {
    let (default, min, max) = SCAN_SECONDS;
    let seconds = seconds.filter(|s| s.is_finite()).unwrap_or(default).clamp(min, max);
    imp::bluetooth_scan(std::time::Duration::from_secs_f64(seconds))
}

/// Pair a device from the last scan; `pin` for a device that asks for one
/// (old keyboards, modules with a fixed PIN). Answers a [`PairStep`].
pub fn bluetooth_pair(id: &str, pin: Option<&str>) -> AppResult<Value> {
    imp::bluetooth_pair(id, pin.map(str::trim).filter(|p| !p.is_empty()))
}

/// Answer the pairing of `id` that waits for the user: `accept` (the PIN
/// matches, or was typed on the device), with `pin` when it asked for one.
pub fn bluetooth_pair_reply(id: &str, accept: bool, pin: Option<&str>) -> AppResult<Value> {
    imp::bluetooth_pair_reply(id, accept, pin.map(str::trim).filter(|p| !p.is_empty()))
}

/// Where a pairing stands, as a page sees it: `{ status, id, pin? }`.
#[derive(Debug, Clone, PartialEq)]
pub enum PairStep {
    /// Done: the device is paired (or already was).
    Paired,
    /// The user declined, or Windows cancelled.
    Cancelled,
    /// Both screens show `pin`: the user confirms it matches (`pairReply`).
    Confirm(String),
    /// Type `pin` on the device (a keyboard), then `pairReply(id, true)`.
    Display(String),
    /// The device wants a PIN: `pairReply(id, true, pin)`.
    NeedPin,
    /// Still going (the device has not answered yet): `pairReply(id, true)` waits on.
    Waiting,
}

pub fn step_json(id: &str, step: &PairStep) -> Value {
    let (status, pin) = match step {
        PairStep::Paired => ("paired", None),
        PairStep::Cancelled => ("cancelled", None),
        PairStep::Confirm(p) => ("confirm", Some(p)),
        PairStep::Display(p) => ("display", Some(p)),
        PairStep::NeedPin => ("pin", None),
        PairStep::Waiting => ("waiting", None),
    };
    let mut v = serde_json::json!({ "status": status, "id": id });
    if let Some(pin) = pin {
        v["pin"] = serde_json::json!(pin);
    }
    v
}

/// `DevicePairingResultStatus` in words, for the error a failed pairing gives.
pub fn pair_failure(status: i32) -> &'static str {
    match status {
        1 => "the device is not ready to pair (is it in pairing mode?)",
        2 => "not paired",
        4 => "the device refused the connection",
        5 => "the device has too many connections",
        6 => "hardware failure",
        7 => "the device did not answer in time",
        8 => "authentication not allowed",
        9 => "authentication failed (wrong PIN?)",
        10 => "the device offers nothing Windows can use",
        11 => "the device cannot pair securely enough",
        12 => "Windows denied access",
        13 => "the device sent bad pairing data",
        15 => "another pairing is in progress",
        18 => "the device is paired with something else",
        _ => "pairing failed",
    }
}

/// One device a scan found.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub id: String,
    pub name: String,
    /// Bluetooth LE (else classic).
    pub le: bool,
    /// Signal strength in dBm, when Windows reports one.
    pub signal: Option<i32>,
}

/// The scan answer: named devices, strongest first.
pub fn scan_json(found: &[Found]) -> Value {
    let mut list: Vec<&Found> = found.iter().filter(|f| !f.name.trim().is_empty()).collect();
    list.sort_by(|a, b| {
        b.signal
            .unwrap_or(i32::MIN)
            .cmp(&a.signal.unwrap_or(i32::MIN))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    serde_json::json!({
        "devices": list.iter().map(|f| serde_json::json!({
            "id": f.id,
            "name": f.name.trim(),
            "kind": if f.le { "le" } else { "classic" },
            "signal": f.signal,
        })).collect::<Vec<_>>(),
    })
}

/// `<`, `>`, `&`, quotes: a profile is XML and the SSID / key come from a page.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// A WLAN profile for a new network. `auth` / `cipher`: profile names
/// (`WPA2PSK` / `AES`, `WPA3SAE` / `AES`, `open` / `none`).
fn profile_xml(ssid: &str, auth: &str, cipher: &str, key: Option<&str>) -> String {
    let name = xml_escape(ssid);
    let shared = key
        .map(|k| {
            format!(
                "<sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>",
                xml_escape(k)
            )
        })
        .unwrap_or_default();
    format!(
        r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
  <name>{name}</name>
  <SSIDConfig><SSID><name>{name}</name></SSID></SSIDConfig>
  <connectionType>ESS</connectionType>
  <connectionMode>auto</connectionMode>
  <MSM><security>
    <authEncryption><authentication>{auth}</authentication><encryption>{cipher}</encryption><useOneX>false</useOneX></authEncryption>
    {shared}
  </security></MSM>
</WLANProfile>"#
    )
}

#[cfg(windows)]
mod imp {
    use super::*;
    use serde_json::json;
    use windows::core::{GUID, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HANDLE};
    use windows::Win32::NetworkManagement::WiFi::{
        WlanCloseHandle, WlanConnect, WlanDeleteProfile, WlanDisconnect, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList,
        WlanOpenHandle, WlanScan, WlanSetProfile, dot11_BSS_type_infrastructure, wlan_connection_mode_profile,
        DOT11_AUTH_ALGO_80211_OPEN, DOT11_AUTH_ALGO_RSNA_PSK, DOT11_AUTH_ALGO_WPA3_SAE, DOT11_AUTH_ALGO_WPA_PSK,
        WLAN_AVAILABLE_NETWORK, WLAN_AVAILABLE_NETWORK_CONNECTED, WLAN_AVAILABLE_NETWORK_HAS_PROFILE,
        WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO_LIST,
    };

    fn err(e: windows::core::Error) -> AppError {
        AppError::msg(e.to_string())
    }

    fn com() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    // ── Wi-Fi ──────────────────────────────────────────────────────────

    /// An open WLAN client handle, closed on drop.
    struct Wlan(HANDLE);

    impl Drop for Wlan {
        fn drop(&mut self) {
            unsafe {
                WlanCloseHandle(self.0, None);
            }
        }
    }

    fn wlan() -> AppResult<Wlan> {
        let (mut version, mut handle) = (0u32, HANDLE::default());
        let r = unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) };
        if r != 0 {
            return Err(AppError::msg(format!("wifi: WLAN service unavailable (error {r})")));
        }
        Ok(Wlan(handle))
    }

    /// The first Wi-Fi adapter.
    fn interface(w: &Wlan) -> AppResult<Option<GUID>> {
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            let r = WlanEnumInterfaces(w.0, None, &mut list);
            if r != 0 || list.is_null() {
                return Err(AppError::msg(format!("wifi: no adapters (error {r})")));
            }
            let n = (*list).dwNumberOfItems as usize;
            let first = (n > 0).then(|| (*list).InterfaceInfo[0].InterfaceGuid);
            WlanFreeMemory(list as *const _);
            Ok(first)
        }
    }

    struct Network {
        ssid: String,
        profile: String,
        signal: u32,
        secure: bool,
        known: bool,
        connected: bool,
        auth: i32,
    }

    fn ssid_of(n: &WLAN_AVAILABLE_NETWORK) -> String {
        let len = (n.dot11Ssid.uSSIDLength as usize).min(n.dot11Ssid.ucSSID.len());
        String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]).into_owned()
    }

    fn wide_str(w: &[u16]) -> String {
        let end = w.iter().position(|c| *c == 0).unwrap_or(w.len());
        String::from_utf16_lossy(&w[..end])
    }

    /// Networks in range, one per SSID (connected, else the strongest).
    /// `Err(None)`: location permission missing.
    fn networks(w: &Wlan, iface: &GUID) -> Result<Vec<Network>, Option<AppError>> {
        unsafe {
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            let r = WlanGetAvailableNetworkList(w.0, iface, 0, None, &mut list);
            if r == ERROR_ACCESS_DENIED.0 {
                return Err(None);
            }
            if r != 0 || list.is_null() {
                return Err(Some(AppError::msg(format!("wifi: network list failed (error {r})"))));
            }
            let n = (*list).dwNumberOfItems as usize;
            let items = std::slice::from_raw_parts((*list).Network.as_ptr(), n);
            let mut out: Vec<Network> = Vec::new();
            for item in items {
                let ssid = ssid_of(item);
                if ssid.is_empty() {
                    continue; // hidden network
                }
                let net = Network {
                    ssid,
                    profile: wide_str(&item.strProfileName),
                    signal: item.wlanSignalQuality,
                    secure: item.bSecurityEnabled.as_bool(),
                    known: item.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0,
                    connected: item.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                    auth: item.dot11DefaultAuthAlgorithm.0,
                };
                match out.iter_mut().find(|o| o.ssid == net.ssid) {
                    Some(o) if (net.connected, net.known, net.signal) > (o.connected, o.known, o.signal) => *o = net,
                    Some(_) => {}
                    None => out.push(net),
                }
            }
            WlanFreeMemory(list as *const _);
            out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
            Ok(out)
        }
    }

    pub fn wifi() -> AppResult<Value> {
        let w = wlan()?;
        let Some(iface) = interface(&w)? else {
            return Ok(json!({ "access": "no-adapter", "connected": null, "networks": [] }));
        };
        match networks(&w, &iface) {
            Ok(list) => Ok(json!({
                "access": "ok",
                "connected": list.iter().find(|n| n.connected).map(|n| n.ssid.clone()),
                "networks": list.iter().map(|n| json!({
                    "ssid": n.ssid,
                    "signal": n.signal,
                    "secure": n.secure,
                    "known": n.known,
                    "connected": n.connected,
                })).collect::<Vec<_>>(),
            })),
            Err(None) => Ok(json!({ "access": "location-denied", "connected": null, "networks": [] })),
            Err(Some(e)) => Err(e),
        }
    }

    pub fn wifi_scan() -> AppResult<()> {
        let w = wlan()?;
        let iface = interface(&w)?.ok_or_else(|| AppError::msg("wifi: no Wi-Fi adapter"))?;
        let r = unsafe { WlanScan(w.0, &iface, None, None, None) };
        if r != 0 {
            return Err(AppError::msg(format!("wifi: scan refused (error {r})")));
        }
        Ok(())
    }

    pub fn wifi_connect(ssid: &str, password: Option<&str>) -> AppResult<()> {
        let w = wlan()?;
        let iface = interface(&w)?.ok_or_else(|| AppError::msg("wifi: no Wi-Fi adapter"))?;
        let list = networks(&w, &iface).map_err(|e| {
            e.unwrap_or_else(|| AppError::msg("wifi: Windows needs location access to list networks"))
        })?;
        let net = list
            .iter()
            .find(|n| n.ssid == ssid)
            .ok_or_else(|| AppError::msg(format!("wifi: '{ssid}' is not in range")))?;
        let password = password.filter(|p| !p.is_empty());
        let created = !(net.known && password.is_none());
        let profile = if !created {
            net.profile.clone()
        } else {
            let (auth, cipher) = match net.auth {
                a if a == DOT11_AUTH_ALGO_80211_OPEN.0 && !net.secure => ("open", "none"),
                a if a == DOT11_AUTH_ALGO_WPA3_SAE.0 => ("WPA3SAE", "AES"),
                a if a == DOT11_AUTH_ALGO_RSNA_PSK.0 => ("WPA2PSK", "AES"),
                a if a == DOT11_AUTH_ALGO_WPA_PSK.0 => ("WPAPSK", "TKIP"),
                _ if !net.secure => ("open", "none"),
                _ => return Err(AppError::msg(format!("wifi: '{ssid}' needs a sign-in Versailles cannot do (enterprise)"))),
            };
            if auth != "open" && password.is_none() {
                return Err(AppError::msg(format!("wifi: '{ssid}' needs a password")));
            }
            let xml = HSTRING::from(profile_xml(ssid, auth, cipher, password));
            let mut reason = 0u32;
            let r = unsafe { WlanSetProfile(w.0, &iface, 0, &xml, PCWSTR::null(), true, None, &mut reason) };
            if r != 0 {
                return Err(AppError::msg(format!("wifi: Windows refused the profile (error {r}, reason {reason})")));
            }
            ssid.to_string()
        };
        let wide: Vec<u16> = profile.encode_utf16().chain([0]).collect();
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(wide.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        let r = unsafe { WlanConnect(w.0, &iface, &params, None) };
        let joined = r == 0 && wait_for(&w, &iface, CONNECT_WAIT, |list| list.iter().any(|n| n.ssid == ssid && n.connected));
        if joined {
            return Ok(());
        }
        // A profile we just saved with a wrong password would make the
        // network look known and fail again: drop it.
        if created {
            unsafe {
                WlanDeleteProfile(w.0, &iface, &HSTRING::from(profile.as_str()), None);
            }
        }
        if r != 0 {
            return Err(AppError::msg(format!("wifi: connect to '{ssid}' failed (error {r})")));
        }
        Err(AppError::msg(format!(
            "wifi: '{ssid}' did not connect within {} s{}",
            CONNECT_WAIT.as_secs(),
            if created { " (wrong password?)" } else { "" }
        )))
    }

    /// How long a connect waits for Windows to report the network joined.
    const CONNECT_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

    /// Poll the network list until `done` or `timeout`.
    fn wait_for(w: &Wlan, iface: &GUID, timeout: std::time::Duration, done: impl Fn(&[Network]) -> bool) -> bool {
        let end = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < end {
            if networks(w, iface).is_ok_and(|list| done(&list)) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        false
    }

    pub fn wifi_disconnect() -> AppResult<()> {
        let w = wlan()?;
        let iface = interface(&w)?.ok_or_else(|| AppError::msg("wifi: no Wi-Fi adapter"))?;
        let r = unsafe { WlanDisconnect(w.0, &iface, None) };
        if r != 0 {
            return Err(AppError::msg(format!("wifi: disconnect failed (error {r})")));
        }
        // Answer with the new state, not the one from before.
        wait_for(&w, &iface, std::time::Duration::from_secs(5), |list| !list.iter().any(|n| n.connected));
        Ok(())
    }

    // ── Bluetooth ──────────────────────────────────────────────────────

    use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DeviceWatcher};
    use windows::Foundation::IPropertyValue;
    use windows::Win32::Media::Audio::{
        eRender, IConnector, IDeviceTopology, IMMDeviceEnumerator, IPart, MMDeviceEnumerator, DEVICE_STATEMASK_ALL,
        DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::Media::KernelStreaming::{
        IKsControl, KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0, KSPROPERTY_ONESHOT_DISCONNECT,
        KSPROPERTY_ONESHOT_RECONNECT, KSPROPERTY_TYPE_GET, KSPROPSETID_BtAudio,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};

    /// Paired Bluetooth devices (classic and LE), as association endpoints.
    const PAIRED_AQS: &str = "(System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" OR \
        System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\") AND \
        System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True";

    fn guid_text(g: &GUID) -> String {
        format!("{g:?}").to_ascii_lowercase()
    }

    /// The Bluetooth audio driver's controls behind each render endpoint,
    /// grouped by device container (`{guid}`), and whether one is active.
    fn audio_controls() -> AppResult<Vec<(String, bool, Vec<IKsControl>)>> {
        use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_ContainerId;
        use windows::Win32::System::Com::StructuredStorage::PropVariantToGUID;
        com();
        let devices: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)?;
        let mut out: Vec<(String, bool, Vec<IKsControl>)> = Vec::new();
        unsafe {
            let list = devices.EnumAudioEndpoints(eRender, windows::Win32::Media::Audio::DEVICE_STATE(DEVICE_STATEMASK_ALL)).map_err(err)?;
            for i in 0..list.GetCount().map_err(err)? {
                let Ok(device) = list.Item(i) else { continue };
                let active = device.GetState().map(|s| s == DEVICE_STATE_ACTIVE).unwrap_or(false);
                let Some(container) = device
                    .OpenPropertyStore(STGM_READ)
                    .and_then(|s| s.GetValue(&PKEY_Device_ContainerId))
                    .ok()
                    .and_then(|v| PropVariantToGUID(&v).ok())
                else {
                    continue;
                };
                let Ok(topology) = device.Activate::<IDeviceTopology>(CLSCTX_ALL, None) else { continue };
                let mut controls = Vec::new();
                for c in 0..topology.GetConnectorCount().unwrap_or(0) {
                    let Some(other) = topology.GetConnector(c).and_then(|c: IConnector| c.GetConnectedTo()).ok() else {
                        continue;
                    };
                    let Ok(part) = windows::core::Interface::cast::<IPart>(&other) else { continue };
                    let Ok(other_topology) = part.GetTopologyObject() else { continue };
                    let Ok(id) = other_topology.GetDeviceId() else { continue };
                    let id_text = id.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(id.0 as *const _));
                    // bthenum / bthhfenum: the Bluetooth audio driver.
                    if !id_text.to_ascii_lowercase().starts_with("{2}.\\\\?\\bth") {
                        continue;
                    }
                    let Ok(driver) = devices.GetDevice(&HSTRING::from(id_text.as_str())) else { continue };
                    if let Ok(ks) = driver.Activate::<IKsControl>(CLSCTX_ALL, None) {
                        controls.push(ks);
                    }
                }
                if controls.is_empty() {
                    continue;
                }
                let key = guid_text(&container);
                match out.iter_mut().find(|(k, _, _)| *k == key) {
                    Some(entry) => {
                        entry.1 |= active;
                        entry.2.extend(controls);
                    }
                    None => out.push((key, active, controls)),
                }
            }
        }
        Ok(out)
    }

    /// A paired device as the watcher last saw it.
    struct Paired {
        name: String,
        connected: bool,
        container: Option<GUID>,
    }

    /// Paired devices by association-endpoint id, kept by [`start_watcher`].
    fn paired() -> &'static std::sync::Mutex<std::collections::HashMap<String, Paired>> {
        static P: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Paired>>> =
            std::sync::OnceLock::new();
        P.get_or_init(Default::default)
    }

    fn prop(map: &windows_collections::IMapView<HSTRING, windows::core::IInspectable>, key: &str) -> Option<IPropertyValue> {
        map.Lookup(&HSTRING::from(key))
            .ok()
            .and_then(|v| windows::core::Interface::cast::<IPropertyValue>(&v).ok())
    }

    /// A one-shot query for paired devices waits out Windows' 30 s Bluetooth
    /// discovery. A device watcher reports the known ones at once and then
    /// keeps their connection state current, as Settings does. It starts with
    /// the first read and runs for the rest of the process.
    fn start_watcher() {
        use windows::Foundation::TypedEventHandler;
        use std::sync::atomic::{AtomicBool, Ordering};
        static STARTED: AtomicBool = AtomicBool::new(false);
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let spawned = std::thread::Builder::new().name("versailles-bluetooth".into()).spawn(|| {
            com();
            let props: windows_collections::IIterable<HSTRING> = vec![
                HSTRING::from("System.Devices.Aep.IsConnected"),
                HSTRING::from("System.Devices.Aep.ContainerId"),
            ]
            .into();
            let watcher = match DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
                &HSTRING::from(PAIRED_AQS),
                &props,
                DeviceInformationKind::AssociationEndpoint,
            ) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("bluetooth: device watcher failed: {e}");
                    return;
                }
            };
            let added = TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(|_, info| {
                if let Ok(info) = info.ok() {
                    let props = info.Properties().ok();
                    let get = |k: &str| props.as_ref().and_then(|m| prop(m, k));
                    paired().lock().unwrap().insert(
                        info.Id()?.to_string(),
                        Paired {
                            name: info.Name().map(|n| n.to_string()).unwrap_or_default(),
                            connected: get("System.Devices.Aep.IsConnected").and_then(|v| v.GetBoolean().ok()).unwrap_or(false),
                            container: get("System.Devices.Aep.ContainerId").and_then(|v| v.GetGuid().ok()),
                        },
                    );
                }
                Ok(())
            });
            let updated = TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(|_, update| {
                if let Ok(update) = update.ok() {
                    let id = update.Id()?.to_string();
                    if let Some(connected) = update
                        .Properties()
                        .ok()
                        .and_then(|m| prop(&m, "System.Devices.Aep.IsConnected"))
                        .and_then(|v| v.GetBoolean().ok())
                    {
                        if let Some(d) = paired().lock().unwrap().get_mut(&id) {
                            d.connected = connected;
                        }
                    }
                }
                Ok(())
            });
            let removed = TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(|_, update| {
                if let Ok(update) = update.ok() {
                    paired().lock().unwrap().remove(&update.Id()?.to_string());
                }
                Ok(())
            });
            let ok = watcher.Added(&added).is_ok()
                && watcher.Updated(&updated).is_ok()
                && watcher.Removed(&removed).is_ok()
                && watcher.Start().is_ok();
            if !ok {
                tracing::warn!("bluetooth: device watcher did not start");
                return;
            }
            // Keep the watcher (and its handlers) alive.
            loop {
                std::thread::park();
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("bluetooth: watcher thread failed: {e}");
        }
    }

    pub fn bluetooth() -> AppResult<Value> {
        let first = paired().lock().unwrap().is_empty();
        start_watcher();
        if first {
            // The known devices arrive within moments of the start.
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if !paired().lock().unwrap().is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    break;
                }
            }
        }
        let audio = audio_controls().unwrap_or_default();
        let mut devices: Vec<Value> = Vec::new();
        for (aep, d) in paired().lock().unwrap().iter() {
            let id = d.container.as_ref().map(guid_text).unwrap_or_else(|| aep.clone());
            let audio_entry = audio.iter().find(|(k, _, _)| *k == id);
            let connected = d.connected || audio_entry.is_some_and(|(_, active, _)| *active);
            // A device listed twice (classic and LE) shows once, connected if either is.
            if let Some(existing) = devices.iter_mut().find(|e| e["id"] == id.as_str()) {
                if connected {
                    existing["connected"] = json!(true);
                }
                if existing["name"].as_str().unwrap_or("").is_empty() {
                    existing["name"] = json!(d.name);
                }
                continue;
            }
            devices.push(json!({
                "id": id,
                "name": d.name,
                "connected": connected,
                "audio": audio_entry.is_some(),
            }));
        }
        devices.retain(|d| !d["name"].as_str().unwrap_or("").is_empty());
        devices.sort_by(|a, b| {
            b["connected"].as_bool().cmp(&a["connected"].as_bool()).then(
                a["name"].as_str().unwrap_or("").to_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_lowercase()),
            )
        });
        Ok(json!({ "devices": devices }))
    }

    pub fn bluetooth_connect(id: &str, on: bool) -> AppResult<()> {
        let audio = audio_controls()?;
        let Some((_, _, controls)) = audio.iter().find(|(k, _, _)| k == id) else {
            return Err(AppError::msg(
                "bluetooth: only audio devices (headphones, speakers) can be connected from here",
            ));
        };
        let property = KSIDENTIFIER {
            Anonymous: KSIDENTIFIER_0 {
                Anonymous: KSIDENTIFIER_0_0 {
                    Set: KSPROPSETID_BtAudio,
                    Id: if on { KSPROPERTY_ONESHOT_RECONNECT.0 as u32 } else { KSPROPERTY_ONESHOT_DISCONNECT.0 as u32 },
                    Flags: KSPROPERTY_TYPE_GET,
                },
            },
        };
        let mut done = false;
        for ks in controls {
            let mut returned = 0u32;
            let r = unsafe {
                ks.KsProperty(&property, std::mem::size_of::<KSIDENTIFIER>() as u32, std::ptr::null_mut(), 0, &mut returned)
            };
            done |= r.is_ok();
        }
        if !done {
            return Err(AppError::msg("bluetooth: the device did not take the request"));
        }
        Ok(())
    }

    // ── Pairing (#69) ──────────────────────────────────────────────────
    //
    // Custom pairing (`DeviceInformationCustomPairing`), which works in an
    // unpackaged, unelevated app; plain `DeviceInformationPairing.PairAsync`
    // does not outside UWP (it needs the app's own pairing UI). As
    // PolarGoose's BluetoothDevicePairing does, but a PIN to compare or
    // type is handed to the page instead of accepted blindly.

    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use windows::Devices::Enumeration::{
        DeviceInformationCustomPairing, DevicePairingKinds, DevicePairingRequestedEventArgs, DevicePairingResultStatus,
    };

    /// Unpaired Bluetooth devices (classic and LE), as association endpoints.
    const UNPAIRED_AQS: &str = "(System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" OR \
        System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\") AND \
        System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#False";
    const LE_PROTOCOL: &str = "bb7bb05e-5972-42b5-94fc-76eaa7084d49";

    /// How long one call waits for the pairing to finish or ask something.
    const STEP_WAIT: Duration = Duration::from_secs(30);
    /// How long Windows waits for the user's answer before giving up.
    const ANSWER_WAIT: Duration = Duration::from_secs(90);

    /// The devices the last scan found, by id: only these can be paired.
    fn scanned() -> &'static Mutex<std::collections::HashMap<String, DeviceInformation>> {
        static S: OnceLock<Mutex<std::collections::HashMap<String, DeviceInformation>>> = OnceLock::new();
        S.get_or_init(Default::default)
    }

    pub fn bluetooth_scan(duration: Duration) -> AppResult<Value> {
        use windows::Foundation::TypedEventHandler;
        com();
        let props: windows_collections::IIterable<HSTRING> = vec![
            HSTRING::from("System.Devices.Aep.SignalStrength"),
            HSTRING::from("System.Devices.Aep.ProtocolId"),
        ]
        .into();
        let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
            &HSTRING::from(UNPAIRED_AQS),
            &props,
            DeviceInformationKind::AssociationEndpoint,
        )
        .map_err(err)?;
        type Seen = Arc<Mutex<std::collections::HashMap<String, (DeviceInformation, Found)>>>;
        let seen: Seen = Arc::default();
        let signal_of = |props: Option<windows_collections::IMapView<HSTRING, windows::core::IInspectable>>| {
            props.as_ref().and_then(|m| prop(m, "System.Devices.Aep.SignalStrength")).and_then(|v| v.GetInt32().ok())
        };
        let on_added = seen.clone();
        let added = TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(move |_, info| {
            if let Ok(info) = info.ok() {
                let props = info.Properties().ok();
                let le = props
                    .as_ref()
                    .and_then(|m| prop(m, "System.Devices.Aep.ProtocolId"))
                    .and_then(|v| v.GetGuid().ok())
                    .is_some_and(|g| guid_text(&g).contains(LE_PROTOCOL));
                let found = Found {
                    id: info.Id()?.to_string(),
                    name: info.Name().map(|n| n.to_string()).unwrap_or_default(),
                    le,
                    signal: signal_of(props),
                };
                on_added.lock().unwrap().insert(found.id.clone(), (info.clone(), found));
            }
            Ok(())
        });
        let on_updated = seen.clone();
        let updated = TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(move |_, update| {
            if let Ok(update) = update.ok() {
                let id = update.Id()?.to_string();
                if let Some((info, found)) = on_updated.lock().unwrap().get_mut(&id) {
                    let _ = info.Update(update);
                    if let Some(s) = signal_of(update.Properties().ok()) {
                        found.signal = Some(s);
                    }
                    if let Ok(name) = info.Name() {
                        if !name.is_empty() {
                            found.name = name.to_string();
                        }
                    }
                }
            }
            Ok(())
        });
        let on_removed = seen.clone();
        let removed = TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(move |_, update| {
            if let Ok(update) = update.ok() {
                on_removed.lock().unwrap().remove(&update.Id()?.to_string());
            }
            Ok(())
        });
        watcher.Added(&added).map_err(err)?;
        watcher.Updated(&updated).map_err(err)?;
        watcher.Removed(&removed).map_err(err)?;
        watcher.Start().map_err(err)?;
        std::thread::sleep(duration);
        let _ = watcher.Stop();
        let seen = std::mem::take(&mut *seen.lock().unwrap());
        let mut cache = scanned().lock().unwrap();
        cache.clear();
        let mut found = Vec::new();
        for (id, (info, f)) in seen {
            cache.insert(id, info);
            found.push(f);
        }
        tracing::info!("bluetooth: scan found {} devices to pair", found.len());
        Ok(scan_json(&found))
    }

    /// What the pairing thread tells the waiting call.
    enum Event {
        Ask(PairStep),
        Done(Result<(), String>),
    }

    /// The user's answer to an `Ask`: accept (with a PIN), or decline.
    type Reply = Option<Option<String>>;

    struct Pending {
        id: String,
        events: Receiver<Event>,
        reply: Sender<Reply>,
        started: Instant,
    }

    fn pending() -> &'static Mutex<Option<Pending>> {
        static P: OnceLock<Mutex<Option<Pending>>> = OnceLock::new();
        P.get_or_init(Default::default)
    }

    /// Wait for the pairing's next step; a finished pairing is forgotten.
    fn next_step(id: &str) -> AppResult<Value> {
        let mut slot = pending().lock().unwrap();
        let Some(p) = slot.as_ref().filter(|p| p.id == id) else {
            return Err(AppError::msg("bluetooth: no pairing in progress for this device"));
        };
        let step = match p.events.recv_timeout(STEP_WAIT) {
            Ok(Event::Ask(step)) => return Ok(step_json(id, &step)),
            Err(RecvTimeoutError::Timeout) => return Ok(step_json(id, &PairStep::Waiting)),
            Ok(Event::Done(result)) => result,
            Err(RecvTimeoutError::Disconnected) => Err("pairing stopped".into()),
        };
        *slot = None;
        match step {
            Ok(()) => {
                tracing::info!("bluetooth: paired {id}");
                Ok(step_json(id, &PairStep::Paired))
            }
            Err(e) if e == "cancelled" => Ok(step_json(id, &PairStep::Cancelled)),
            Err(e) => Err(AppError::msg(format!("bluetooth: {e}"))),
        }
    }

    /// The pairing ceremony's question, answered by Windows' handler thread.
    fn on_request(
        args: &DevicePairingRequestedEventArgs,
        pin: Option<&str>,
        events: &Sender<Event>,
        replies: &Mutex<Receiver<Reply>>,
    ) -> windows::core::Result<()> {
        let kind = args.PairingKind()?;
        let ask = |step: PairStep| -> Reply {
            let replies = replies.lock().unwrap();
            // Replies to steps that asked nothing (display, waiting) are not answers.
            while replies.try_recv().is_ok() {}
            let _ = events.send(Event::Ask(step));
            replies.recv_timeout(ANSWER_WAIT).unwrap_or(None)
        };
        if kind == DevicePairingKinds::ConfirmOnly {
            // The user asked for this device by clicking it.
            args.Accept()
        } else if kind == DevicePairingKinds::ProvidePin {
            let pin = match pin {
                Some(p) => Some(p.to_string()),
                None => ask(PairStep::NeedPin).flatten(),
            };
            match pin {
                Some(p) => args.AcceptWithPin(&HSTRING::from(p)),
                None => Ok(()),
            }
        } else if kind == DevicePairingKinds::ConfirmPinMatch {
            match ask(PairStep::Confirm(args.Pin()?.to_string())) {
                Some(_) => args.Accept(),
                None => Ok(()),
            }
        } else if kind == DevicePairingKinds::DisplayPin {
            // The PIN goes to the page; the device's keyboard answers.
            let _ = events.send(Event::Ask(PairStep::Display(args.Pin()?.to_string())));
            args.Accept()
        } else {
            Ok(())
        }
    }

    pub fn bluetooth_pair(id: &str, pin: Option<&str>) -> AppResult<Value> {
        use windows::Foundation::TypedEventHandler;
        {
            let mut slot = pending().lock().unwrap();
            if slot.as_ref().is_some_and(|p| p.started.elapsed() < ANSWER_WAIT * 2) {
                return Err(AppError::msg("bluetooth: another pairing is in progress"));
            }
            *slot = None;
        }
        let info = scanned()
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::msg("bluetooth: not a device from the last scan (scan first)"))?;
        com();
        let pairing = info.Pairing().map_err(err)?;
        if pairing.IsPaired().unwrap_or(false) {
            return Ok(step_json(id, &PairStep::Paired));
        }
        if !pairing.CanPair().unwrap_or(false) {
            return Err(AppError::msg("bluetooth: this device cannot be paired now (is it in pairing mode?)"));
        }
        let custom: DeviceInformationCustomPairing = pairing.Custom().map_err(err)?;
        // A device with a fixed PIN can ask for DisplayPin instead of
        // ProvidePin when both are offered (PolarGoose): with a PIN, offer
        // no DisplayPin.
        let mut kinds = DevicePairingKinds::ConfirmOnly | DevicePairingKinds::ProvidePin | DevicePairingKinds::ConfirmPinMatch;
        if pin.is_none() {
            kinds = kinds | DevicePairingKinds::DisplayPin;
        }
        let (event_tx, event_rx) = channel::<Event>();
        let (reply_tx, reply_rx) = channel::<Reply>();
        let replies = Arc::new(Mutex::new(reply_rx));
        let handler_events = event_tx.clone();
        let handler_pin = pin.map(str::to_string);
        let handler = TypedEventHandler::<DeviceInformationCustomPairing, DevicePairingRequestedEventArgs>::new(
            move |_, args| {
                if let Ok(args) = args.ok() {
                    let deferral = args.GetDeferral()?;
                    let r = on_request(args, handler_pin.as_deref(), &handler_events, &replies);
                    let _ = deferral.Complete();
                    r?;
                }
                Ok(())
            },
        );
        let token = custom.PairingRequested(&handler).map_err(err)?;
        tracing::info!("bluetooth: pairing {id}");
        let name = id.to_string();
        std::thread::Builder::new()
            .name("versailles-bluetooth-pair".into())
            .spawn(move || {
                com();
                let result = custom
                    .PairAsync(kinds)
                    .and_then(|op| op.get())
                    .and_then(|r| r.Status())
                    .map_err(|e| e.to_string())
                    .and_then(|status| match status {
                        DevicePairingResultStatus::Paired | DevicePairingResultStatus::AlreadyPaired => Ok(()),
                        DevicePairingResultStatus::RejectedByHandler | DevicePairingResultStatus::PairingCanceled => {
                            Err("cancelled".to_string())
                        }
                        other => Err(pair_failure(other.0).to_string()),
                    });
                let _ = custom.RemovePairingRequested(token);
                if let Err(e) = &result {
                    tracing::info!("bluetooth: pairing {name} ended: {e}");
                }
                let _ = event_tx.send(Event::Done(result));
            })
            .map_err(|e| AppError::msg(format!("bluetooth: {e}")))?;
        *pending().lock().unwrap() = Some(Pending { id: id.to_string(), events: event_rx, reply: reply_tx, started: Instant::now() });
        next_step(id)
    }

    pub fn bluetooth_pair_reply(id: &str, accept: bool, pin: Option<&str>) -> AppResult<Value> {
        {
            let slot = pending().lock().unwrap();
            let Some(p) = slot.as_ref().filter(|p| p.id == id) else {
                return Err(AppError::msg("bluetooth: no pairing in progress for this device"));
            };
            // Display and waiting steps ask nothing: the reply only waits on.
            let _ = p.reply.send(if accept { Some(pin.map(str::to_string)) } else { None });
        }
        next_step(id)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    fn unavailable<T>() -> AppResult<T> {
        Err(AppError::msg("wifi / bluetooth are only available on Windows"))
    }
    pub fn wifi() -> AppResult<Value> {
        unavailable()
    }
    pub fn wifi_scan() -> AppResult<()> {
        unavailable()
    }
    pub fn wifi_connect(_: &str, _: Option<&str>) -> AppResult<()> {
        unavailable()
    }
    pub fn wifi_disconnect() -> AppResult<()> {
        unavailable()
    }
    pub fn bluetooth() -> AppResult<Value> {
        unavailable()
    }
    pub fn bluetooth_connect(_: &str, _: bool) -> AppResult<()> {
        unavailable()
    }
    pub fn bluetooth_scan(_: std::time::Duration) -> AppResult<Value> {
        unavailable()
    }
    pub fn bluetooth_pair(_: &str, _: Option<&str>) -> AppResult<Value> {
        unavailable()
    }
    pub fn bluetooth_pair_reply(_: &str, _: bool, _: Option<&str>) -> AppResult<Value> {
        unavailable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_steps_and_scan_answers() {
        assert_eq!(step_json("a", &PairStep::Paired), serde_json::json!({ "status": "paired", "id": "a" }));
        assert_eq!(
            step_json("a", &PairStep::Confirm("123456".into())),
            serde_json::json!({ "status": "confirm", "id": "a", "pin": "123456" })
        );
        assert_eq!(step_json("a", &PairStep::NeedPin)["status"], "pin");
        assert_eq!(step_json("a", &PairStep::Display("0042".into()))["pin"], "0042");
        assert_eq!(step_json("a", &PairStep::Waiting)["status"], "waiting");
        assert!(pair_failure(9).contains("PIN"));
        assert_eq!(pair_failure(999), "pairing failed");

        let found = [
            Found { id: "1".into(), name: "Mouse".into(), le: true, signal: Some(-70) },
            Found { id: "2".into(), name: " ".into(), le: true, signal: Some(-40) },
            Found { id: "3".into(), name: "Headset".into(), le: false, signal: Some(-50) },
            Found { id: "4".into(), name: "Speaker".into(), le: false, signal: None },
        ];
        let v = scan_json(&found);
        let ids: Vec<&str> = v["devices"].as_array().unwrap().iter().map(|d| d["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["3", "1", "4"]);
        assert_eq!(v["devices"][0]["kind"], "classic");
        assert_eq!(v["devices"][1]["kind"], "le");
        assert!(v["devices"][2]["signal"].is_null());
    }

    /// Looks for unpaired devices nearby (no pairing, nothing changed):
    /// `cargo test --lib live_bluetooth_scan -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_bluetooth_scan() {
        let t = std::time::Instant::now();
        let v = bluetooth_scan(Some(10.0));
        println!("scan ({} ms): {v:?}", t.elapsed().as_millis());
    }

    #[test]
    fn profile_escapes_ssid_and_key() {
        let xml = profile_xml("Café <&> \"Net\"", "WPA2PSK", "AES", Some("p&ss<word>"));
        assert!(xml.contains("<name>Café &lt;&amp;&gt; &quot;Net&quot;</name>"));
        assert!(xml.contains("<keyMaterial>p&amp;ss&lt;word&gt;</keyMaterial>"));
        assert!(xml.contains("<authentication>WPA2PSK</authentication>"));
        let open = profile_xml("Guest", "open", "none", None);
        assert!(!open.contains("sharedKey"));
    }

    /// `cargo test live_wireless -- --ignored --nocapture`: what this PC answers.
    #[test]
    #[ignore]
    fn live_wireless() {
        for hook in ["wifi", "bluetooth"] {
            let t = std::time::Instant::now();
            let value = read(hook).map(|v| v.to_string());
            println!("{hook} ({} ms): {value:?}", t.elapsed().as_millis());
        }
        println!("scan: {:?}", wifi_scan());
        std::thread::sleep(std::time::Duration::from_secs(5));
        println!("wifi after scan: {:?}", read("wifi").map(|v| v.to_string()));
    }
}
