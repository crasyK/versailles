//! Keyboard layouts (#60): the `keyboard` hook.
//!
//! Reads `{ layouts: [{ id, name, short, lang, layout }], current }`: the
//! installed input languages (`GetKeyboardLayoutList`) and the one the app in
//! front types with. `id` is the layout handle (HKL) as 8 hex digits.
//! Windows has no event for another process's layout change, so the sensor
//! watcher reads it every 0.5 s and pushes a change.
//!
//! `keyboard.set(id)` (control class) switches the app in front like
//! Win+Space: it posts `WM_INPUTLANGCHANGEREQUEST` to that app's focus
//! window (`ActivateKeyboardLayout` only reaches our own threads). A click
//! on a Versailles bar may bring the bar to the front, so the target is the
//! last app window in front that is not ours. Windows of an app running as
//! administrator ignore the request (UIPI), as do the few apps that do not
//! pass it to `DefWindowProc`; `current` then stays, and the SDK answers
//! `ok: false`.

use crate::error::{AppError, AppResult};
use serde::Serialize;
use serde_json::{json, Value};

/// One installed input language and keyboard layout.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Layout {
    /// The HKL as 8 hex digits (`"04070407"`); pass it to `set`.
    pub id: String,
    /// The language in Windows' display language (`"Deutsch (Deutschland)"`).
    pub name: String,
    /// The language code in capitals (`"DE"`), for a pill.
    pub short: String,
    /// The locale name (`"de-DE"`).
    pub lang: String,
    /// The keyboard layout's name (`"German"`), when Windows has one.
    pub layout: Option<String>,
}

/// Low 32 bits of an HKL as 8 hex digits. A 64-bit HKL with a layout id in
/// its high word is sign-extended (`0xFFFFFFFFF0C00409`); its low 32 bits
/// name it on both 32- and 64-bit Windows.
pub fn hkl_id(raw: isize) -> String {
    format!("{:08X}", raw as usize as u32)
}

/// The language id in an HKL's low word.
pub fn lang_id(raw: isize) -> u16 {
    (raw as usize & 0xFFFF) as u16
}

/// `"de-DE"` → `"DE"`, `"zh-Hans-CN"` → `"ZH"`; `"?"` without a name.
pub fn short_name(locale: &str) -> String {
    let lang = locale.split(['-', '_']).next().unwrap_or("").trim();
    if lang.is_empty() {
        "?".into()
    } else {
        lang.to_ascii_uppercase()
    }
}

/// Where to find an HKL's layout under `HKLM\SYSTEM\CurrentControlSet\
/// Control\Keyboard Layouts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Klid {
    /// The key name itself (`"00000407"`, an IME's `"E0010411"`).
    Key(String),
    /// The key whose `Layout Id` value is this (a variant such as
    /// German (IBM)), from an HKL high word `0xFxxx`.
    LayoutId(String),
}

pub fn klid(raw: isize) -> Klid {
    let v = raw as usize as u32;
    let high = (v >> 16) as u16;
    match high & 0xF000 {
        0xE000 => Klid::Key(format!("{v:08X}")),
        0xF000 => Klid::LayoutId(format!("{:04X}", high & 0x0FFF)),
        _ => Klid::Key(format!("{high:08X}")),
    }
}

/// A layout id from a page: hex digits, an optional `0x`, any case.
pub fn normalize_id(id: &str) -> Option<String> {
    let t = id.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if t.is_empty() || t.len() > 8 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("{:0>8}", t.to_ascii_uppercase()))
}

pub fn state_json(layouts: &[Layout], current: Option<String>) -> Value {
    json!({ "layouts": layouts, "current": current })
}

/// The installed layouts and the current one.
pub fn read() -> AppResult<Value> {
    imp::read()
}

/// Switch the app in front to layout `id` (control class). Waits briefly
/// for the app to take it; whether it did shows in `current`.
pub fn set(id: &str) -> AppResult<()> {
    let id = normalize_id(id).ok_or_else(|| AppError::msg(format!("keyboard: '{id}' is not a layout id")))?;
    imp::set(&id)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL};
    use windows::Win32::UI::Shell::SHLoadIndirectString;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsWindow, PostMessageW, GUITHREADINFO,
        WM_INPUTLANGCHANGEREQUEST,
    };

    const LAYOUTS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts";

    /// The last app window in front that is not Versailles'.
    fn last_target() -> &'static Mutex<isize> {
        static LAST: OnceLock<Mutex<isize>> = OnceLock::new();
        LAST.get_or_init(|| Mutex::new(0))
    }

    /// Names per HKL: the registry and locale reads run once.
    fn names() -> &'static Mutex<HashMap<isize, Layout>> {
        static MAP: OnceLock<Mutex<HashMap<isize, Layout>>> = OnceLock::new();
        MAP.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn pid_of(h: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        pid
    }

    /// The window the app in front types into, and its thread.
    fn focus_target() -> Option<(HWND, u32)> {
        unsafe {
            let fg = GetForegroundWindow();
            let mut last = last_target().lock().unwrap();
            if !fg.is_invalid() && pid_of(fg) != std::process::id() {
                *last = fg.0 as isize;
            }
            let target = HWND(*last as *mut _);
            if *last == 0 || !IsWindow(Some(target)).as_bool() {
                *last = 0;
                return None;
            }
            drop(last);
            let thread = GetWindowThreadProcessId(target, None);
            let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
            // A store app's frame belongs to ApplicationFrameHost; its focus
            // window is the app's own.
            let focus = if GetGUIThreadInfo(thread, &mut info).is_ok() {
                [info.hwndFocus, info.hwndActive].into_iter().find(|h| !h.is_invalid()).unwrap_or(target)
            } else {
                target
            };
            Some((focus, GetWindowThreadProcessId(focus, None)))
        }
    }

    fn installed() -> Vec<HKL> {
        unsafe {
            let n = GetKeyboardLayoutList(None);
            if n <= 0 {
                return Vec::new();
            }
            let mut list = vec![HKL::default(); n as usize];
            let got = GetKeyboardLayoutList(Some(&mut list)).max(0) as usize;
            list.truncate(got);
            list
        }
    }

    fn wide(buf: &[u16]) -> String {
        let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n])
    }

    fn locale_name(lcid: u16) -> String {
        let mut buf = [0u16; 85];
        let n = unsafe { LCIDToLocaleName(lcid as u32, Some(&mut buf), 0) };
        if n > 0 {
            wide(&buf)
        } else {
            String::new()
        }
    }

    fn display_name(locale: &str) -> Option<String> {
        if locale.is_empty() {
            return None;
        }
        let name = HSTRING::from(locale);
        let mut buf = [0u16; 256];
        let n = unsafe { GetLocaleInfoEx(&name, LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut buf)) };
        (n > 0).then(|| wide(&buf)).filter(|s| !s.is_empty())
    }

    fn reg_string(key: &str, value: PCWSTR) -> Option<String> {
        let key = HSTRING::from(key);
        let mut buf = [0u16; 512];
        let mut size = (buf.len() * 2) as u32;
        let ok = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &key,
                value,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
        }
        .is_ok();
        ok.then(|| wide(&buf)).filter(|s| !s.is_empty())
    }

    /// The key under `Keyboard Layouts` whose `Layout Id` is `id`.
    fn key_by_layout_id(id: &str) -> Option<String> {
        unsafe {
            let mut root = HKEY::default();
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, &HSTRING::from(LAYOUTS_KEY), None, KEY_READ, &mut root).is_err() {
                return None;
            }
            let mut found = None;
            for index in 0.. {
                let mut name = [0u16; 64];
                let mut len = name.len() as u32;
                if RegEnumKeyExW(root, index, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None)
                    .is_err()
                {
                    break;
                }
                let key = String::from_utf16_lossy(&name[..len as usize]);
                let layout_id = reg_string(&format!(r"{LAYOUTS_KEY}\{key}"), w!("Layout Id"));
                if layout_id.is_some_and(|v| v.eq_ignore_ascii_case(id)) {
                    found = Some(key);
                    break;
                }
            }
            let _ = RegCloseKey(root);
            found
        }
    }

    /// The layout's name: `Layout Display Name` (a resource string in the
    /// display language), else `Layout Text`.
    fn layout_name(raw: isize) -> Option<String> {
        let key = match klid(raw) {
            Klid::Key(k) => k,
            Klid::LayoutId(id) => key_by_layout_id(&id)?,
        };
        let path = format!(r"{LAYOUTS_KEY}\{key}");
        if let Some(indirect) = reg_string(&path, w!("Layout Display Name")) {
            let mut buf = [0u16; 256];
            if unsafe { SHLoadIndirectString(&HSTRING::from(indirect.as_str()), &mut buf, None) }.is_ok() {
                let s = wide(&buf);
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        reg_string(&path, w!("Layout Text"))
    }

    fn describe(hkl: HKL) -> Layout {
        let raw = hkl.0 as isize;
        if let Some(hit) = names().lock().unwrap().get(&raw) {
            return hit.clone();
        }
        let lang = locale_name(lang_id(raw));
        let layout = Layout {
            id: hkl_id(raw),
            name: display_name(&lang).unwrap_or_else(|| lang.clone()),
            short: short_name(&lang),
            lang,
            layout: layout_name(raw),
        };
        names().lock().unwrap().insert(raw, layout.clone());
        layout
    }

    fn current_layout() -> Option<HKL> {
        let (_, thread) = focus_target()?;
        let hkl = unsafe { GetKeyboardLayout(thread) };
        (!hkl.is_invalid()).then_some(hkl)
    }

    pub fn read() -> AppResult<Value> {
        let layouts: Vec<Layout> = installed().into_iter().map(describe).collect();
        let current = current_layout().map(|h| hkl_id(h.0 as isize));
        Ok(state_json(&layouts, current))
    }

    pub fn set(id: &str) -> AppResult<()> {
        let hkl = installed()
            .into_iter()
            .find(|h| hkl_id(h.0 as isize) == id)
            .ok_or_else(|| AppError::msg(format!("keyboard: no installed layout '{id}'")))?;
        let (focus, thread) =
            focus_target().ok_or_else(|| AppError::msg("keyboard: no app window to switch"))?;
        if unsafe { GetKeyboardLayout(thread) } == hkl {
            return Ok(());
        }
        unsafe { PostMessageW(Some(focus), WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(hkl.0 as isize)) }
            .map_err(|e| AppError::msg(format!("keyboard: {e}")))?;
        // The app switches when it handles the message; an elevated app
        // never does (UIPI drops the post silently).
        let until = Instant::now() + Duration::from_millis(400);
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(25));
            if unsafe { GetKeyboardLayout(thread) } == hkl {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn read() -> AppResult<Value> {
        Err(AppError::msg("the keyboard hook is only available on Windows"))
    }
    pub fn set(_id: &str) -> AppResult<()> {
        read().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_hkl_ids() {
        assert_eq!(hkl_id(0x0407_0407), "04070407");
        assert_eq!(hkl_id(0x0409_0409), "04090409");
        // A sign-extended 64-bit HKL with a layout id (German (IBM)).
        assert_eq!(hkl_id(0xFFFF_FFFF_F0C0_0409_u64 as i64 as isize), "F0C00409");
        assert_eq!(hkl_id(0x0000_0409), "00000409");
        assert_eq!(lang_id(0x0407_0407), 0x0407);
        assert_eq!(lang_id(0xFFFF_FFFF_F0C0_0409_u64 as i64 as isize), 0x0409);
    }

    #[test]
    fn short_names() {
        assert_eq!(short_name("de-DE"), "DE");
        assert_eq!(short_name("en-US"), "EN");
        assert_eq!(short_name("zh-Hans-CN"), "ZH");
        assert_eq!(short_name("sr-Latn-RS"), "SR");
        assert_eq!(short_name("haw"), "HAW");
        assert_eq!(short_name(""), "?");
    }

    #[test]
    fn layout_keys() {
        assert_eq!(klid(0x0407_0407), Klid::Key("00000407".into()));
        // Language de-DE with the US layout.
        assert_eq!(klid(0x0409_0407), Klid::Key("00000409".into()));
        assert_eq!(klid(0xFFFF_FFFF_F0C0_0409_u64 as i64 as isize), Klid::LayoutId("00C0".into()));
        assert_eq!(klid(0xE001_0411_u32 as isize), Klid::Key("E0010411".into()));
    }

    #[test]
    fn normalizes_page_ids() {
        assert_eq!(normalize_id("04070407").as_deref(), Some("04070407"));
        assert_eq!(normalize_id(" 0xf0c00409 ").as_deref(), Some("F0C00409"));
        assert_eq!(normalize_id("409").as_deref(), Some("00000409"));
        assert!(normalize_id("").is_none());
        assert!(normalize_id("123456789").is_none());
        assert!(normalize_id("de-DE").is_none());
    }

    #[test]
    fn state_shape() {
        let de = Layout {
            id: "04070407".into(),
            name: "Deutsch (Deutschland)".into(),
            short: "DE".into(),
            lang: "de-DE".into(),
            layout: Some("Deutsch".into()),
        };
        let v = state_json(&[de], Some("04070407".into()));
        assert_eq!(v["current"], "04070407");
        assert_eq!(v["layouts"][0]["short"], "DE");
        assert_eq!(v["layouts"][0]["lang"], "de-DE");
        assert_eq!(v["layouts"][0]["layout"], "Deutsch");
        assert_eq!(state_json(&[], None)["current"], Value::Null);
    }

    /// Reads this machine's layouts: `cargo test --lib live_keyboard -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_keyboard() {
        let v = read().expect("read");
        eprintln!("{}", serde_json::to_string_pretty(&v).unwrap());
        assert!(v["layouts"].as_array().is_some_and(|l| !l.is_empty()));
    }
}
