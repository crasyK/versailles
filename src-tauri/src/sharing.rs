//! Cast and nearby sharing (#65): the `sharing` hook. A read is a sensor
//! (see `sensors::SENSORS`); changes go through `quick_set` (control class,
//! see `quick`).
//!
//! - **Cast.** Windows has no API for a desktop app to mirror the screen to a
//!   Miracast receiver (`Windows.Media.Miracast` is receiver side,
//!   `ProjectionManager` projects a UWP view, `Windows.Media.Casting` casts a
//!   media element). What works: `casting` tells whether a Miracast output is
//!   active (`QueryDisplayConfig`, documented), and `openCast()` opens
//!   Windows' own Cast panel (`ms-settings-connectabledevices:devicediscovery`,
//!   what Win+K opens), where the user picks a receiver or disconnects.
//! - **Nearby sharing** has no API either. Its setting is the undocumented
//!   value `HKCU\Software\Microsoft\Windows\CurrentVersion\CDP\
//!   NearShareChannelUserAuthzPolicy` (0 off, 1 my devices only, 2 everyone
//!   nearby), which Settings > System > Nearby sharing reads and writes. The
//!   value is read as is (`null` when missing or unknown); `setNearby` writes
//!   it only when the key exists and fails softly otherwise.
//!
//! TODO(#65): whether the Connected Devices Platform service picks up a
//! written value at once is not confirmed (community guides say to sign out).
//! If a live test shows it only applies after sign-out, make `nearby`
//! read-only and point pages at `ms-settings:crossdevice` instead.

use crate::error::{AppError, AppResult};
use serde_json::{json, Value};

/// Windows' Cast panel (the one Win+K opens).
pub const CAST_URI: &str = "ms-settings-connectabledevices:devicediscovery";
/// Settings > System > Nearby sharing.
pub const NEARBY_SETTINGS_URI: &str = "ms-settings:crossdevice";

const CDP_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\CDP";
const NEARBY_VALUE: &str = "NearShareChannelUserAuthzPolicy";

/// Nearby-sharing modes as pages name them, with the registry value.
const NEARBY: &[(&str, u32)] = &[("off", 0), ("my-devices", 1), ("everyone", 2)];

/// Registry value → page mode (`None`: unknown).
pub fn nearby_name(value: u32) -> Option<&'static str> {
    NEARBY.iter().find(|(_, v)| *v == value).map(|(n, _)| *n)
}

/// Page mode → registry value.
pub fn nearby_value(mode: &str) -> Option<u32> {
    let mode = mode.trim().to_ascii_lowercase();
    NEARBY.iter().find(|(n, _)| *n == mode).map(|(_, v)| *v)
}

/// `DISPLAYCONFIG_OUTPUT_TECHNOLOGY_MIRACAST`.
pub const OUTPUT_MIRACAST: i32 = 15;

/// Whether any of these active display paths' output technologies is a
/// Miracast (wireless display) link.
pub fn casting_in(output_technologies: impl IntoIterator<Item = i32>) -> bool {
    output_technologies.into_iter().any(|t| t == OUTPUT_MIRACAST)
}

pub fn read() -> AppResult<Value> {
    Ok(json!({
        "casting": imp::casting(),
        "nearby": imp::nearby().and_then(nearby_name),
    }))
}

/// Open Windows' Cast panel.
pub fn open_cast() -> AppResult<()> {
    crate::quick::shell_open(CAST_URI)
}

/// Set nearby sharing: `"off"`, `"my-devices"` or `"everyone"`.
pub fn set_nearby(mode: &str) -> AppResult<()> {
    let value = nearby_value(mode)
        .ok_or_else(|| AppError::msg(format!("sharing: unknown nearby mode '{mode}' (off, my-devices, everyone)")))?;
    if imp::nearby().is_none() {
        return Err(AppError::msg(format!(
            "sharing: nearby sharing is not set up on this PC (open {NEARBY_SETTINGS_URI})"
        )));
    }
    imp::write_nearby(value)?;
    if imp::nearby() != Some(value) {
        return Err(AppError::msg(format!("sharing: Windows did not keep the nearby setting (open {NEARBY_SETTINGS_URI})")));
    }
    Ok(())
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Devices::Display::{
        GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
        QDC_ONLY_ACTIVE_PATHS,
    };
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD,
        RRF_RT_REG_DWORD,
    };

    fn wide_z(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    /// Output technologies of the active display paths.
    fn active_outputs() -> Option<Vec<i32>> {
        unsafe {
            let (mut paths, mut modes) = (0u32, 0u32);
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes) != ERROR_SUCCESS {
                return None;
            }
            let mut path = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
            let mut mode = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
            let r = QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut paths, path.as_mut_ptr(), &mut modes, mode.as_mut_ptr(), None);
            if r != ERROR_SUCCESS {
                return None;
            }
            path.truncate(paths as usize);
            Some(path.iter().map(|p| p.targetInfo.outputTechnology.0).collect())
        }
    }

    /// `None` when the display configuration cannot be read.
    pub fn casting() -> Option<bool> {
        active_outputs().map(casting_in)
    }

    pub fn nearby() -> Option<u32> {
        let (key, name) = (wide_z(CDP_KEY), wide_z(NEARBY_VALUE));
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        status.is_ok().then_some(value)
    }

    pub fn write_nearby(value: u32) -> AppResult<()> {
        let (path, name) = (wide_z(CDP_KEY), wide_z(NEARBY_VALUE));
        let mut key = HKEY::default();
        // Open, never create: no CDP key means Windows has not set it up.
        let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, KEY_SET_VALUE, &mut key) };
        if opened.is_err() {
            return Err(AppError::msg(format!("sharing: cannot open the nearby sharing setting ({opened:?})")));
        }
        tracing::info!("sharing: nearby sharing -> {value} ({NEARBY_VALUE}, undocumented)");
        let wrote = unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_DWORD, Some(&value.to_le_bytes())) };
        unsafe {
            let _ = RegCloseKey(key);
        }
        if wrote.is_err() {
            return Err(AppError::msg(format!("sharing: Windows refused the nearby sharing setting ({wrote:?})")));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn casting() -> Option<bool> {
        None
    }
    pub fn nearby() -> Option<u32> {
        None
    }
    pub fn write_nearby(_: u32) -> AppResult<()> {
        Err(AppError::msg("sharing is only available on Windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearby_mapping() {
        assert_eq!(nearby_name(0), Some("off"));
        assert_eq!(nearby_name(1), Some("my-devices"));
        assert_eq!(nearby_name(2), Some("everyone"));
        assert_eq!(nearby_name(3), None);
        assert_eq!(nearby_value("everyone"), Some(2));
        assert_eq!(nearby_value(" My-Devices "), Some(1));
        assert_eq!(nearby_value("off"), Some(0));
        assert_eq!(nearby_value("friends"), None);
        for (name, value) in NEARBY {
            assert_eq!(nearby_name(*value), Some(*name));
        }
    }

    #[test]
    fn miracast_detection() {
        // HDMI (5), internal (0x8000_0000 as i32), DisplayPort embedded (11).
        assert!(!casting_in([5, i32::MIN, 11]));
        assert!(casting_in([i32::MIN, OUTPUT_MIRACAST]));
        assert!(!casting_in([]));
    }

    #[cfg(windows)]
    #[test]
    fn miracast_constant_matches_windows() {
        use windows::Win32::Devices::Display::{DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_MIRACAST};
        assert_eq!(OUTPUT_MIRACAST, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_MIRACAST.0);
        assert!(!casting_in([DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL.0]));
    }

    /// `cargo test live_sharing -- --ignored --nocapture`: read-only. Never
    /// writes the nearby setting.
    #[test]
    #[ignore]
    fn live_sharing() {
        println!("sharing: {:?}", read().map(|v| v.to_string()));
    }
}
