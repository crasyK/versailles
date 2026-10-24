//! Mobile hotspot (#64): the `hotspot` hook. A read is a sensor (see
//! `sensors::SENSORS`; Windows has no change event, so it is polled); on and
//! off go through `quick_set` (control class, see `quick`).
//!
//! Documented WinRT: `NetworkOperatorTetheringManager` for the internet
//! connection profile, the API behind Settings > Mobile hotspot. It works
//! from an unpackaged, unelevated process (the `wiFiControl` capability is
//! only enforced for packaged apps). The hotspot shares the connection the
//! PC is online with; offline there is nothing to share (`supported: false`,
//! `reason: "no-internet"`).
//!
//! Privacy: the access point's configuration (`GetCurrentAccessPointConfiguration`)
//! also holds the network password. It is never read: pages get the state
//! and the client counts only.

use crate::error::{AppError, AppResult};
use serde_json::{json, Value};

/// Settings > Network & internet > Mobile hotspot.
pub const SETTINGS_URI: &str = "ms-settings:network-mobilehotspot";

/// `TetheringCapability` → why the hotspot is not available (`None`: it is).
pub fn capability_reason(capability: i32) -> Option<&'static str> {
    Some(match capability {
        0 => return None,
        1 => "group-policy",
        2 => "hardware",
        3 => "operator",
        4 => "edition",
        5 => "app-missing",
        7 => "system",
        _ => "unknown",
    })
}

/// `TetheringOperationalState` → `on` (`None`: unknown or switching).
pub fn state_on(state: i32) -> Option<bool> {
    match state {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// `TetheringOperationStatus` of a start / stop: `Ok` for success (and
/// "already on"), else the reason in words.
pub fn operation_result(status: i32, detail: &str) -> Result<(), String> {
    let reason = match status {
        0 | 9 => return Ok(()),
        2 => "the mobile broadband device is off",
        3 => "Wi-Fi is off",
        4 | 5 => "the mobile operator did not allow it",
        6 => "Windows is already switching it",
        7 => "Bluetooth is off",
        8 => "the connection has no internet",
        10 => "a radio restriction (airplane mode?) blocks it",
        11 => "the Wi-Fi adapter cannot share on this band",
        _ => "an unknown reason",
    };
    let detail = detail.trim();
    Err(if detail.is_empty() { reason.to_string() } else { format!("{reason}: {detail}") })
}

pub fn read() -> AppResult<Value> {
    imp::read()
}

/// Turn the hotspot on or off. Blocking: call off the main thread.
pub fn set(on: bool) -> AppResult<()> {
    imp::set(on)
}

fn unsupported(reason: &str) -> Value {
    json!({ "supported": false, "on": null, "busy": false, "clients": null, "maxClients": null, "reason": reason })
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::NetworkOperators::{NetworkOperatorTetheringManager, TetheringOperationalState};

    fn err(e: windows::core::Error) -> AppError {
        AppError::msg(format!("hotspot: {e}"))
    }

    fn com() {
        // Short-lived blocking threads: keep the MTA (and windows-rs' cached
        // WinRT factories) alive between them.
        crate::quick::keep_com_alive();
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    /// The tethering manager for the internet connection, or why there is none.
    fn manager() -> Result<NetworkOperatorTetheringManager, &'static str> {
        com();
        let profile = NetworkInformation::GetInternetConnectionProfile().map_err(|_| "no-internet")?;
        let capability = NetworkOperatorTetheringManager::GetTetheringCapabilityFromConnectionProfile(&profile)
            .map_err(|_| "unknown")?;
        if let Some(reason) = capability_reason(capability.0) {
            return Err(reason);
        }
        NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).map_err(|_| "unknown")
    }

    pub fn read() -> AppResult<Value> {
        let m = match manager() {
            Ok(m) => m,
            Err(reason) => return Ok(unsupported(reason)),
        };
        let state = m.TetheringOperationalState().map(|s| s.0).unwrap_or(0);
        Ok(json!({
            "supported": true,
            "on": state_on(state),
            "busy": state == TetheringOperationalState::InTransition.0,
            "clients": m.ClientCount().ok(),
            "maxClients": m.MaxClientCount().ok(),
            "reason": null,
        }))
    }

    pub fn set(on: bool) -> AppResult<()> {
        let m = manager().map_err(|reason| {
            AppError::msg(match reason {
                "no-internet" => "hotspot: the PC has no internet connection to share".to_string(),
                r => format!("hotspot: not available on this PC ({r}; see {SETTINGS_URI})"),
            })
        })?;
        let state = m.TetheringOperationalState().map(|s| s.0).unwrap_or(0);
        if state_on(state) == Some(on) {
            return Ok(());
        }
        tracing::info!("hotspot: turning {}", if on { "on" } else { "off" });
        let op = if on { m.StartTetheringAsync() } else { m.StopTetheringAsync() };
        let result = op.and_then(|op| op.get()).map_err(err)?;
        let status = result.Status().map(|s| s.0).unwrap_or(1);
        let detail = result.AdditionalErrorMessage().map(|s| s.to_string()).unwrap_or_default();
        operation_result(status, &detail).map_err(|why| {
            AppError::msg(format!("hotspot: Windows could not turn it {} ({why})", if on { "on" } else { "off" }))
        })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn read() -> AppResult<Value> {
        Ok(unsupported("system"))
    }
    pub fn set(_: bool) -> AppResult<()> {
        Err(AppError::msg("hotspot is only available on Windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_and_state() {
        assert_eq!(capability_reason(0), None);
        assert_eq!(capability_reason(1), Some("group-policy"));
        assert_eq!(capability_reason(7), Some("system"));
        assert_eq!(capability_reason(42), Some("unknown"));
        assert_eq!(state_on(1), Some(true));
        assert_eq!(state_on(2), Some(false));
        assert_eq!(state_on(3), None, "switching");
        assert_eq!(state_on(0), None);
    }

    #[test]
    fn operation_results() {
        assert!(operation_result(0, "").is_ok());
        assert!(operation_result(9, "").is_ok(), "already on is fine");
        assert_eq!(operation_result(3, ""), Err("Wi-Fi is off".into()));
        assert_eq!(operation_result(11, " try 5 GHz "), Err("the Wi-Fi adapter cannot share on this band: try 5 GHz".into()));
        assert!(operation_result(99, "").unwrap_err().contains("unknown"));
    }

    #[cfg(windows)]
    #[test]
    fn values_match_windows() {
        use windows::Networking::NetworkOperators::{TetheringCapability, TetheringOperationStatus, TetheringOperationalState};
        assert_eq!(capability_reason(TetheringCapability::Enabled.0), None);
        assert_eq!(capability_reason(TetheringCapability::DisabledByGroupPolicy.0), Some("group-policy"));
        assert_eq!(state_on(TetheringOperationalState::On.0), Some(true));
        assert_eq!(state_on(TetheringOperationalState::Off.0), Some(false));
        assert!(operation_result(TetheringOperationStatus::AlreadyOn.0, "").is_ok());
        assert!(operation_result(TetheringOperationStatus::WiFiDeviceOff.0, "").is_err());
    }

    /// `cargo test live_hotspot -- --ignored --nocapture`: read-only. Never
    /// starts or stops the hotspot.
    #[test]
    #[ignore]
    fn live_hotspot() {
        let t = std::time::Instant::now();
        let value = read().map(|v| v.to_string());
        println!("hotspot ({} ms): {value:?}", t.elapsed().as_millis());
    }
}
