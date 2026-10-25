//! Accessibility toggles (#66): the `accessibility` hook. A read is a sensor
//! (see `sensors::SENSORS`); `set(name, on)` goes through `quick_set`
//! (control class, see `quick`).
//!
//! | Toggle | Read | Change |
//! | ------ | ---- | ------ |
//! | `stickyKeys` | `SPI_GETSTICKYKEYS` | `SPI_SETSTICKYKEYS` (documented, immediate) |
//! | `contrast` | `SPI_GETHIGHCONTRAST` | on: `SPI_SETHIGHCONTRAST` (documented; Windows applies the last contrast theme, which takes seconds and switches pages to CSS `forced-colors`); off: opens Settings > Accessibility > Contrast themes, since whether Windows brings back a custom theme after a programmatic switch is not confirmed |
//! | `magnifier` | `Magnify.exe` running | on: start it; off: Win+Esc, Magnifier's own shortcut |
//! | `captions` | `LiveCaptions.exe` running | on: start it; off: Win+Ctrl+L, Live captions' own shortcut |
//! | `colorFilter` | `HKCU\Software\Microsoft\ColorFiltering\Active` | **undocumented**: `atbroker /colorfiltershortcut`, which toggles; run only when the state differs, then checked |
//!
//! Injected shortcuts carry the keyboard hook's tag (`keys::send_tagged`) so
//! Versailles' own Win-key handling passes them on untouched. Each value is
//! `null` when it cannot be read (an app that is not installed).

use crate::error::{AppError, AppResult};
use serde_json::{json, Value};

/// Settings > Accessibility > Contrast themes.
pub const CONTRAST_SETTINGS_URI: &str = "ms-settings:easeofaccess-highcontrast";

/// The toggles, as pages name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Magnifier,
    Contrast,
    Captions,
    ColorFilter,
    StickyKeys,
}

impl Feature {
    pub const ALL: [Feature; 5] =
        [Feature::Magnifier, Feature::Contrast, Feature::Captions, Feature::ColorFilter, Feature::StickyKeys];

    pub fn name(self) -> &'static str {
        match self {
            Feature::Magnifier => "magnifier",
            Feature::Contrast => "contrast",
            Feature::Captions => "captions",
            Feature::ColorFilter => "colorFilter",
            Feature::StickyKeys => "stickyKeys",
        }
    }

    /// A page's name (`"stickyKeys"`; any case, `-` / `_` ignored).
    pub fn parse(name: &str) -> Option<Feature> {
        let key: String = name.chars().filter(|c| *c != '-' && *c != '_').collect::<String>().to_ascii_lowercase();
        Feature::ALL.into_iter().find(|f| f.name().to_ascii_lowercase() == key)
    }
}

/// `SKF_STICKYKEYSON`.
pub const STICKYKEYS_ON: u32 = 0x1;
/// `HCF_HIGHCONTRASTON`.
pub const HIGHCONTRAST_ON: u32 = 0x1;

/// `flags` with `bit` set or cleared; every other bit kept.
pub fn with_flag(flags: u32, bit: u32, on: bool) -> u32 {
    if on {
        flags | bit
    } else {
        flags & !bit
    }
}

/// The page payload from each toggle's state.
pub fn state_json(read: impl Fn(Feature) -> Option<bool>) -> Value {
    let mut out = serde_json::Map::new();
    for f in Feature::ALL {
        out.insert(f.name().into(), json!(read(f)));
    }
    Value::Object(out)
}

pub fn read() -> AppResult<Value> {
    // One process snapshot answers both apps: it is most of a read's cost,
    // and this is a sensor read every 2 s.
    let running = imp::apps_running();
    Ok(state_json(|f| match f {
        Feature::Magnifier | Feature::Captions => imp::installed(f).then(|| running.contains(&f)),
        _ => imp::get(f),
    }))
}

/// Turn one toggle on or off. Blocking (a contrast theme takes seconds):
/// call off the main thread. `Ok(Some(..))`: Settings opened instead.
pub fn set(name: &str, on: bool) -> AppResult<Option<Value>> {
    let f = Feature::parse(name).ok_or_else(|| {
        AppError::msg(format!(
            "accessibility: unknown toggle '{name}' (magnifier, contrast, captions, colorFilter, stickyKeys)"
        ))
    })?;
    let now = imp::get(f);
    if now == Some(on) {
        return Ok(None);
    }
    tracing::info!("accessibility: {} -> {}", f.name(), if on { "on" } else { "off" });
    match f {
        Feature::StickyKeys => imp::set_sticky(on).map(|_| None),
        Feature::Contrast if on => imp::contrast_on().map(|_| None),
        Feature::Contrast => {
            crate::quick::shell_open(CONTRAST_SETTINGS_URI)?;
            Ok(Some(json!({ "ok": true, "settings": true })))
        }
        Feature::Magnifier | Feature::Captions => {
            if now.is_none() {
                return Err(AppError::msg(format!("accessibility: {} is not installed on this PC", f.name())));
            }
            imp::toggle_app(f, on)?;
            // Answer the new state: give the app a moment to start or close.
            let changed = imp::wait_for(f, on);
            if !changed && !on {
                return Err(AppError::msg(format!(
                    "accessibility: {} did not close (its shortcut may have been blocked by an elevated window)",
                    f.name()
                )));
            }
            Ok(None)
        }
        Feature::ColorFilter => {
            imp::toggle_color_filter()?;
            if !imp::wait_for(f, on) {
                return Err(AppError::msg(
                    "accessibility: Windows did not switch colour filters (an undocumented switch; use Settings > Accessibility > Color filters)",
                ));
            }
            Ok(None)
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows::Win32::UI::Accessibility::{HIGHCONTRASTW, HIGHCONTRASTW_FLAGS, STICKYKEYS, STICKYKEYS_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETHIGHCONTRAST, SPI_GETSTICKYKEYS,
        SPI_SETHIGHCONTRAST, SPI_SETSTICKYKEYS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    fn system32(exe: &str) -> std::path::PathBuf {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        std::path::Path::new(&windir).join("System32").join(exe)
    }

    fn app_exe(f: Feature) -> &'static str {
        match f {
            Feature::Magnifier => "Magnify.exe",
            _ => "LiveCaptions.exe",
        }
    }

    /// Processes that own a top-level window (Magnifier and Live Captions
    /// always do while they run), each with its executable's file name. A
    /// process snapshot cost ~10 ms of CPU per read; this enumerates windows
    /// and asks only new processes for their name.
    fn window_processes() -> Vec<String> {
        use std::collections::{HashMap, HashSet};
        use std::sync::{Mutex, OnceLock};
        use windows::core::BOOL;
        use windows::Win32::Foundation::{HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};
        unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
            let pids = &mut *(l.0 as *mut HashSet<u32>);
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid != 0 {
                pids.insert(pid);
            }
            BOOL(1)
        }
        static NAMES: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
        let mut pids: HashSet<u32> = HashSet::new();
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut pids as *mut HashSet<u32> as isize));
        }
        let mut names = NAMES.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
        names.retain(|pid, _| pids.contains(pid));
        for pid in pids {
            names.entry(pid).or_insert_with(|| {
                crate::app_windows::process_path(pid)
                    .and_then(|p| std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_default()
            });
        }
        names.values().cloned().collect()
    }

    /// Which of these file names run (with a window).
    fn running_any(exes: &[&str]) -> Vec<bool> {
        let running = window_processes();
        exes.iter().map(|exe| running.iter().any(|n| n.eq_ignore_ascii_case(exe))).collect()
    }

    /// Whether a process with this file name runs (with a window).
    fn running(exe: &str) -> bool {
        running_any(&[exe])[0]
    }

    /// Whether the app behind a toggle (Magnifier, Live Captions) is on this PC.
    pub fn installed(f: Feature) -> bool {
        system32(app_exe(f)).is_file()
    }

    /// The app toggles whose app runs.
    pub fn apps_running() -> Vec<Feature> {
        let apps = [Feature::Magnifier, Feature::Captions];
        let exes = apps.map(app_exe);
        let found = running_any(&exes);
        apps.into_iter().zip(found).filter(|(_, on)| *on).map(|(f, _)| f).collect()
    }

    fn sticky_flags() -> Option<u32> {
        let mut sk = STICKYKEYS { cbSize: std::mem::size_of::<STICKYKEYS>() as u32, ..Default::default() };
        unsafe {
            SystemParametersInfoW(
                SPI_GETSTICKYKEYS,
                sk.cbSize,
                Some(&mut sk as *mut _ as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .ok()?;
        Some(sk.dwFlags.0)
    }

    fn contrast_flags() -> Option<u32> {
        let mut hc = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
        unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                hc.cbSize,
                Some(&mut hc as *mut _ as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .ok()?;
        Some(hc.dwFlags.0)
    }

    fn color_filter() -> Option<bool> {
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                windows::core::w!("Software\\Microsoft\\ColorFiltering"),
                windows::core::w!("Active"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        // Never set: colour filters have never been on.
        Some(status.is_ok() && value != 0)
    }

    pub fn get(f: Feature) -> Option<bool> {
        match f {
            Feature::StickyKeys => sticky_flags().map(|fl| fl & STICKYKEYS_ON != 0),
            Feature::Contrast => contrast_flags().map(|fl| fl & HIGHCONTRAST_ON != 0),
            Feature::ColorFilter => color_filter(),
            Feature::Magnifier | Feature::Captions => {
                let exe = app_exe(f);
                system32(exe).is_file().then(|| running(exe))
            }
        }
    }

    fn update() -> SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS {
        SPIF_UPDATEINIFILE | SPIF_SENDCHANGE
    }

    pub fn set_sticky(on: bool) -> AppResult<()> {
        let flags = sticky_flags().ok_or_else(|| AppError::msg("accessibility: cannot read sticky keys"))?;
        let mut sk = STICKYKEYS {
            cbSize: std::mem::size_of::<STICKYKEYS>() as u32,
            dwFlags: STICKYKEYS_FLAGS(with_flag(flags, STICKYKEYS_ON, on)),
        };
        unsafe { SystemParametersInfoW(SPI_SETSTICKYKEYS, sk.cbSize, Some(&mut sk as *mut _ as *mut _), update()) }
            .map_err(|e| AppError::msg(format!("accessibility: Windows refused sticky keys ({e})")))
    }

    pub fn contrast_on() -> AppResult<()> {
        let flags = contrast_flags().ok_or_else(|| AppError::msg("accessibility: cannot read the contrast theme"))?;
        // No scheme: Windows applies the contrast theme last used.
        let mut hc = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            dwFlags: HIGHCONTRASTW_FLAGS(with_flag(flags, HIGHCONTRAST_ON, true)),
            lpszDefaultScheme: windows::core::PWSTR::null(),
        };
        unsafe { SystemParametersInfoW(SPI_SETHIGHCONTRAST, hc.cbSize, Some(&mut hc as *mut _ as *mut _), update()) }
            .map_err(|e| AppError::msg(format!("accessibility: Windows refused the contrast theme ({e})")))
    }

    const VK_LWIN: u16 = 0x5B;
    const VK_LCONTROL: u16 = 0xA2;
    const VK_ESCAPE: u16 = 0x1B;
    const VK_L: u16 = 0x4C;

    /// Press `keys` in order, then release them in reverse.
    fn chord(keys: &[u16]) {
        let mut events: Vec<(u16, bool)> = keys.iter().map(|k| (*k, false)).collect();
        events.extend(keys.iter().rev().map(|k| (*k, true)));
        crate::keys::send_tagged(&events);
    }

    pub fn toggle_app(f: Feature, on: bool) -> AppResult<()> {
        if on {
            // ShellExecute: Magnifier is a uiAccess app, which CreateProcess
            // from an ordinary process cannot start.
            let exe = system32(app_exe(f));
            return crate::quick::shell_open(&exe.to_string_lossy());
        }
        match f {
            Feature::Magnifier => chord(&[VK_LWIN, VK_ESCAPE]),
            _ => chord(&[VK_LWIN, VK_LCONTROL, VK_L]),
        }
        Ok(())
    }

    pub fn toggle_color_filter() -> AppResult<()> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = std::process::Command::new(system32("atbroker.exe"))
            .args(["/colorfiltershortcut", "/resettransferkeys"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| AppError::msg(format!("accessibility: could not run atbroker ({e})")))?;
        if !status.success() {
            tracing::debug!("accessibility: atbroker answered {status}");
        }
        Ok(())
    }

    /// Wait (up to 3 s) until `f` reads `on`.
    pub fn wait_for(f: Feature, on: bool) -> bool {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if get(f) == Some(on) {
                return true;
            }
            if std::time::Instant::now() >= end {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    fn unavailable() -> AppResult<()> {
        Err(AppError::msg("accessibility is only available on Windows"))
    }
    pub fn get(_: Feature) -> Option<bool> {
        None
    }
    pub fn installed(_: Feature) -> bool {
        false
    }
    pub fn apps_running() -> Vec<Feature> {
        Vec::new()
    }
    pub fn set_sticky(_: bool) -> AppResult<()> {
        unavailable()
    }
    pub fn contrast_on() -> AppResult<()> {
        unavailable()
    }
    pub fn toggle_app(_: Feature, _: bool) -> AppResult<()> {
        unavailable()
    }
    pub fn toggle_color_filter() -> AppResult<()> {
        unavailable()
    }
    pub fn wait_for(_: Feature, _: bool) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for f in Feature::ALL {
            assert_eq!(Feature::parse(f.name()), Some(f));
        }
        assert_eq!(Feature::parse("sticky-keys"), Some(Feature::StickyKeys));
        assert_eq!(Feature::parse("COLOR_FILTER"), Some(Feature::ColorFilter));
        assert_eq!(Feature::parse("narrator"), None);
    }

    #[test]
    fn flags() {
        // This PC's sticky keys: 0x1FA (available, hotkey, ... but not on).
        assert_eq!(with_flag(0x1FA, STICKYKEYS_ON, true), 0x1FB);
        assert_eq!(with_flag(0x1FB, STICKYKEYS_ON, false), 0x1FA);
        assert_eq!(with_flag(0x1FA, STICKYKEYS_ON, false), 0x1FA, "already off: unchanged");
        // Contrast flags 126 (0x7E): every other bit survives.
        assert_eq!(with_flag(126, HIGHCONTRAST_ON, true), 127);
    }

    #[test]
    fn payload() {
        let v = state_json(|f| match f {
            Feature::StickyKeys => Some(true),
            Feature::Captions => None,
            _ => Some(false),
        });
        assert_eq!(v, json!({ "magnifier": false, "contrast": false, "captions": null, "colorFilter": false, "stickyKeys": true }));
    }

    #[cfg(windows)]
    #[test]
    fn flags_match_windows() {
        use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, SKF_STICKYKEYSON};
        assert_eq!(STICKYKEYS_ON, SKF_STICKYKEYSON.0);
        assert_eq!(HIGHCONTRAST_ON, HCF_HIGHCONTRASTON.0);
    }

    #[test]
    fn unknown_toggle_is_refused() {
        assert!(set("narrator", true).unwrap_err().to_string().contains("unknown toggle"));
    }

    /// `cargo test live_accessibility -- --ignored --nocapture`: read-only.
    #[test]
    #[ignore]
    fn live_accessibility() {
        let t = std::time::Instant::now();
        let value = read().map(|v| v.to_string());
        println!("accessibility ({} ms): {value:?}", t.elapsed().as_millis());
    }

    /// `cargo test live_sticky_keys -- --ignored --nocapture`: turns sticky
    /// keys on and straight back to what it was.
    #[test]
    #[ignore]
    fn live_sticky_keys() {
        let before = imp::get(Feature::StickyKeys).expect("sticky keys readable");
        println!("sticky keys before: {before}");
        println!("set {}: {:?}", !before, set("stickyKeys", !before));
        println!("sticky keys now: {:?}", imp::get(Feature::StickyKeys));
        println!("restore {before}: {:?}", set("stickyKeys", before));
        assert_eq!(imp::get(Feature::StickyKeys), Some(before));
    }
}
