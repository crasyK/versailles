//! Windows' own VPN profiles (#63): the `vpn` hook. A read is a sensor (see
//! `sensors::SENSORS`); connect and disconnect go through `quick_set`
//! (control class, see `quick`).
//!
//! Windows' built-in VPN profiles (IKEv2, SSTP, L2TP, PPTP, Automatic) are
//! entries in the RAS phone books (`%APPDATA%\Microsoft\Network\Connections\
//! Pbk\rasphone.pbk` for the user, the same under `%ProgramData%` for all
//! users), read and dialed with the documented RAS API:
//!
//! - List: `RasEnumEntriesW` over both phone books; state: `RasEnumConnectionsW`
//!   and `RasGetConnectStatusW`.
//! - Connect: `RasDialW` with the credentials Windows saved for the entry
//!   (`RasGetEntryDialParamsW` hands back a handle RasDial accepts, never the
//!   password itself). Without saved credentials, for EAP or certificate
//!   profiles, and when Windows rejects the saved ones, Windows' own dial
//!   dialog opens instead (`rasphone -d`), which asks for them.
//! - VPN-app (plugin) profiles and names that are not in a phone book (a
//!   Store app's own profile): Settings > Network > VPN opens instead.
//! - Disconnect: `RasHangUpW`.
//!
//! Pages only ever get profile names and their state: no user names,
//! passwords, servers or phone books. Credentials are never logged.
//! Third-party VPN apps (WireGuard, Tailscale, ProtonVPN, OpenVPN) are not
//! Windows VPN profiles and are not listed.

use crate::error::{AppError, AppResult};
use serde_json::{json, Value};

/// Settings > Network & internet > VPN.
pub const SETTINGS_URI: &str = "ms-settings:network-vpn";

/// One phone-book entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    /// The phone book file it is in.
    pub phonebook: String,
    /// In the all-users phone book (else the user's own).
    pub all_users: bool,
}

/// An active RAS connection: its entry name and whether it is fully up
/// (`false`: still dialing or authenticating).
#[derive(Debug, Clone, PartialEq)]
pub struct Active {
    pub name: String,
    pub connected: bool,
}

/// A fixed-size UTF-16 field up to its NUL.
pub fn wide_field(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Copy `s` into a fixed-size UTF-16 field, NUL-terminated. `false` when it
/// does not fit (nothing is truncated silently).
pub fn put_field(buf: &mut [u16], s: &str) -> bool {
    let w: Vec<u16> = s.encode_utf16().collect();
    if w.len() + 1 > buf.len() {
        return false;
    }
    buf[..w.len()].copy_from_slice(&w);
    buf[w.len()..].iter_mut().for_each(|c| *c = 0);
    true
}

/// Entries as pages see them: one per name (a name in both phone books
/// shows once; the user's own wins, as RAS dials it first), sorted by name,
/// each with its state.
pub fn profiles_json(entries: &[Entry], active: &[Active]) -> Value {
    let mut names: Vec<&Entry> = Vec::new();
    for e in entries.iter().filter(|e| !e.name.is_empty()) {
        match names.iter_mut().find(|n| n.name.eq_ignore_ascii_case(&e.name)) {
            Some(n) if n.all_users && !e.all_users => *n = e,
            Some(_) => {}
            None => names.push(e),
        }
    }
    names.sort_by_key(|e| e.name.to_lowercase());
    let profiles: Vec<Value> = names
        .iter()
        .map(|e| {
            let conn = active.iter().find(|a| a.name.eq_ignore_ascii_case(&e.name));
            json!({
                "name": e.name,
                "connected": conn.is_some_and(|a| a.connected),
                "connecting": conn.is_some_and(|a| !a.connected),
            })
        })
        .collect();
    json!({ "profiles": profiles })
}

/// How to connect an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dial {
    /// `RasDialW` with the saved credentials.
    Saved,
    /// Windows' dial dialog (`rasphone -d`): it asks for what is missing.
    Dialog,
    /// A VPN app's (plugin) profile: only Settings can connect it.
    Settings,
}

/// `RASEO_RequireEAP` (`dwfOptions`).
pub const OPT_REQUIRE_EAP: u32 = 0x0002_0000;
/// `RASEO2_RequireMachineCertificates` (`dwfOptions2`).
pub const OPT2_MACHINE_CERTS: u32 = 0x0040_0000;
/// `RASEO2_IsThirdPartyProfile` (`dwfOptions2`): a VPN plugin app's profile.
pub const OPT2_THIRD_PARTY: u32 = 0x0800_0000;

/// Pick the way to connect from the entry's options and whether Windows
/// saved a password for it.
pub fn dial_kind(options: u32, options2: u32, has_password: bool) -> Dial {
    if options2 & OPT2_THIRD_PARTY != 0 {
        Dial::Settings
    } else if options & OPT_REQUIRE_EAP != 0 || options2 & OPT2_MACHINE_CERTS != 0 || !has_password {
        Dial::Dialog
    } else {
        Dial::Saved
    }
}

/// RasDial errors Windows' own dialog can resolve by asking the user:
/// wrong or expired credentials, interaction required, certificates.
pub fn needs_dialog(code: u32) -> bool {
    matches!(
        code,
        691 // ERROR_AUTHENTICATION_FAILURE
            | 703 // ERROR_INTERACTIVE_MODE
            | 709 // ERROR_CHANGING_PASSWORD
            | 798 // ERROR_NO_CERTIFICATE
            | 13801 // IKE: authentication credentials unacceptable
            | 13806 // IKE: no valid machine certificate
    )
}

/// What `connect` / `disconnect` did when it is not just the new state.
fn opened(what: &str) -> Value {
    let mut v = json!({ "ok": true });
    v[what] = json!(true);
    v
}

pub fn read() -> AppResult<Value> {
    let entries = imp::entries()?;
    let active = imp::active();
    Ok(profiles_json(&entries, &active))
}

/// Connect `name`. Blocking (a dial takes seconds): call off the main
/// thread. `Ok(None)`: dialed, answer the new state; `Ok(Some(..))`:
/// Windows' dialog or Settings opened instead.
pub fn connect(name: &str) -> AppResult<Option<Value>> {
    let entries = imp::entries()?;
    let Some(entry) = entries.iter().find(|e| e.name == name).or_else(|| entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)))
    else {
        // Not a phone-book entry: a VPN app's profile Settings knows about.
        crate::quick::shell_open(SETTINGS_URI)?;
        return Ok(Some(opened("settings")));
    };
    if imp::active().iter().any(|a| a.name.eq_ignore_ascii_case(&entry.name)) {
        return Ok(None); // already connected or dialing
    }
    match imp::dial_kind(entry)? {
        Dial::Settings => {
            crate::quick::shell_open(SETTINGS_URI)?;
            Ok(Some(opened("settings")))
        }
        Dial::Dialog => {
            imp::dialog(entry)?;
            Ok(Some(opened("dialog")))
        }
        Dial::Saved => match imp::dial(entry) {
            Ok(()) => Ok(None),
            Err(code) if needs_dialog(code) => {
                tracing::info!("vpn: '{}' needs credentials (RAS error {code}): opening Windows' dialog", entry.name);
                imp::dialog(entry)?;
                Ok(Some(opened("dialog")))
            }
            Err(code) => Err(AppError::msg(format!("vpn: '{}' did not connect: {}", entry.name, imp::error_text(code)))),
        },
    }
}

/// Disconnect `name` (nothing to do when it is not connected).
pub fn disconnect(name: &str) -> AppResult<()> {
    imp::hang_up(name)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::NetworkManagement::Rras::{
        RasDialW, RasEnumConnectionsW, RasEnumEntriesW, RasGetConnectStatusW, RasGetEntryDialParamsW,
        RasGetEntryPropertiesW, RasGetErrorStringW, RasHangUpW, HRASCONN, RASCONNSTATUSW, RASCONNW, RASCS_Connected,
        RASDIALPARAMSW, RASENTRYNAMEW, RASENTRYW, REN_AllUsers,
    };

    const ERROR_BUFFER_TOO_SMALL: u32 = 603;
    const ERROR_INVALID_HANDLE: u32 = 6;

    fn wide_z(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    pub fn entries() -> AppResult<Vec<Entry>> {
        let size = std::mem::size_of::<RASENTRYNAMEW>();
        let mut buf = vec![RASENTRYNAMEW { dwSize: size as u32, ..Default::default() }; 4];
        for _ in 0..3 {
            let (mut bytes, mut count) = ((buf.len() * size) as u32, 0u32);
            let r = unsafe { RasEnumEntriesW(PCWSTR::null(), PCWSTR::null(), Some(buf.as_mut_ptr()), &mut bytes, &mut count) };
            match r {
                0 => {
                    return Ok(buf[..count as usize]
                        .iter()
                        .map(|e| Entry {
                            name: wide_field(&e.szEntryName),
                            phonebook: wide_field(&e.szPhonebookPath),
                            all_users: e.dwFlags == REN_AllUsers,
                        })
                        .collect())
                }
                ERROR_BUFFER_TOO_SMALL => {
                    let n = (bytes as usize).div_ceil(size).max(buf.len() + 1);
                    buf = vec![RASENTRYNAMEW { dwSize: size as u32, ..Default::default() }; n];
                }
                r => return Err(AppError::msg(format!("vpn: could not list the VPN profiles (RAS error {r})"))),
            }
        }
        Err(AppError::msg("vpn: the VPN profile list kept changing"))
    }

    fn connections() -> Vec<RASCONNW> {
        let size = std::mem::size_of::<RASCONNW>();
        let fresh = |n: usize| {
            let mut v = vec![RASCONNW::default(); n];
            v.iter_mut().for_each(|c| c.dwSize = size as u32);
            v
        };
        let mut buf = fresh(4);
        for _ in 0..3 {
            let (mut bytes, mut count) = ((buf.len() * size) as u32, 0u32);
            match unsafe { RasEnumConnectionsW(Some(buf.as_mut_ptr()), &mut bytes, &mut count) } {
                0 => {
                    buf.truncate(count as usize);
                    return buf;
                }
                ERROR_BUFFER_TOO_SMALL => buf = fresh((bytes as usize).div_ceil(size).max(buf.len() + 1)),
                _ => return Vec::new(),
            }
        }
        Vec::new()
    }

    fn status(h: HRASCONN) -> Result<bool, u32> {
        let mut st = RASCONNSTATUSW { dwSize: std::mem::size_of::<RASCONNSTATUSW>() as u32, ..Default::default() };
        match unsafe { RasGetConnectStatusW(h, &mut st) } {
            0 => Ok(st.rasconnstate == RASCS_Connected),
            e => Err(e),
        }
    }

    pub fn active() -> Vec<Active> {
        connections()
            .iter()
            .map(|c| Active { name: wide_field(&{ c.szEntryName }), connected: status(c.hrasconn).unwrap_or(false) })
            .collect()
    }

    fn phonebook(entry: &Entry) -> Vec<u16> {
        wide_z(&entry.phonebook)
    }

    /// The entry's `dwfOptions` / `dwfOptions2`.
    fn options(entry: &Entry) -> AppResult<(u32, u32)> {
        let size = std::mem::size_of::<RASENTRYW>();
        let (pbk, name) = (phonebook(entry), wide_z(&entry.name));
        // The entry is variable-length (alternate numbers follow it): an
        // 8-byte aligned buffer of the size RAS asks for.
        let mut want = size as u32;
        for _ in 0..3 {
            let mut buf = vec![0u64; (want as usize).div_ceil(8)];
            let ptr = buf.as_mut_ptr() as *mut RASENTRYW;
            unsafe { (*ptr).dwSize = size as u32 };
            let mut got = want;
            let r = unsafe {
                RasGetEntryPropertiesW(PCWSTR(pbk.as_ptr()), PCWSTR(name.as_ptr()), Some(ptr), &mut got, None, None)
            };
            match r {
                0 => return Ok(unsafe { ((*ptr).dwfOptions, (*ptr).dwfOptions2) }),
                ERROR_BUFFER_TOO_SMALL if got > want => want = got,
                r => return Err(AppError::msg(format!("vpn: could not read '{}' ({})", entry.name, error_text(r)))),
            }
        }
        Err(AppError::msg(format!("vpn: could not read '{}'", entry.name)))
    }

    /// Wipe what may hold a credential handle before the memory is reused.
    fn wipe(p: &mut RASDIALPARAMSW) {
        let raw = p as *mut RASDIALPARAMSW as *mut u8;
        for i in 0..std::mem::size_of::<RASDIALPARAMSW>() {
            unsafe { std::ptr::write_volatile(raw.add(i), 0) };
        }
    }

    /// The saved dial parameters (user name, password handle, domain).
    /// `None` when the entry name does not fit or RAS has none.
    fn saved_params(entry: &Entry) -> Option<(RASDIALPARAMSW, bool)> {
        let mut p = RASDIALPARAMSW { dwSize: std::mem::size_of::<RASDIALPARAMSW>() as u32, ..Default::default() };
        if !put_field(&mut p.szEntryName, &entry.name) {
            return None;
        }
        let pbk = phonebook(entry);
        let mut has_password = BOOL(0);
        if unsafe { RasGetEntryDialParamsW(PCWSTR(pbk.as_ptr()), &mut p, &mut has_password) } != 0 {
            wipe(&mut p);
            return None;
        }
        Some((p, has_password.as_bool()))
    }

    pub fn dial_kind(entry: &Entry) -> AppResult<Dial> {
        let (o1, o2) = options(entry)?;
        let has_password = match saved_params(entry) {
            Some((mut p, has)) => {
                wipe(&mut p);
                has
            }
            None => false,
        };
        Ok(super::dial_kind(o1, o2, has_password))
    }

    /// `RasDialW` with the saved credentials, synchronously. `Err(code)`
    /// on failure (the half-open connection is hung up).
    pub fn dial(entry: &Entry) -> Result<(), u32> {
        let Some((mut p, _)) = saved_params(entry) else {
            return Err(703); // nothing saved: interactive
        };
        let pbk = phonebook(entry);
        tracing::info!("vpn: dialing '{}'", entry.name);
        let mut h = HRASCONN::default();
        let r = unsafe { RasDialW(None, PCWSTR(pbk.as_ptr()), &p, 0, None, &mut h) };
        wipe(&mut p);
        if r == 0 {
            return Ok(());
        }
        if !h.0.is_null() {
            hang_up_handle(h);
        }
        Err(r)
    }

    /// Windows' own dial dialog for the entry; it asks for credentials.
    pub fn dialog(entry: &Entry) -> AppResult<()> {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let exe = std::path::Path::new(&windir).join("System32").join("rasphone.exe");
        let mut cmd = std::process::Command::new(exe);
        if !entry.phonebook.is_empty() {
            cmd.arg("-f").arg(&entry.phonebook);
        }
        // One argument each, never through a shell: the name is user text.
        cmd.arg("-d").arg(&entry.name);
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| AppError::msg(format!("vpn: could not open Windows' VPN dialog ({e})")))
    }

    /// Hang up and wait (up to 3 s) until RAS has released the connection.
    fn hang_up_handle(h: HRASCONN) {
        unsafe { RasHangUpW(h) };
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < end {
            if status(h) == Err(ERROR_INVALID_HANDLE) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    pub fn hang_up(name: &str) -> AppResult<()> {
        let Some(conn) = connections().into_iter().find(|c| wide_field(&{ c.szEntryName }).eq_ignore_ascii_case(name)) else {
            return Ok(());
        };
        tracing::info!("vpn: hanging up '{name}'");
        let r = unsafe { RasHangUpW(conn.hrasconn) };
        if r != 0 {
            return Err(AppError::msg(format!("vpn: could not disconnect '{name}': {}", error_text(r))));
        }
        hang_up_handle(conn.hrasconn);
        Ok(())
    }

    /// Windows' text for a RAS error (in the system's language).
    pub fn error_text(code: u32) -> String {
        let mut buf = [0u16; 512];
        if unsafe { RasGetErrorStringW(code, &mut buf) } == 0 {
            let text = wide_field(&buf).trim().to_string();
            if !text.is_empty() {
                return format!("{text} (RAS error {code})");
            }
        }
        format!("RAS error {code}")
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    fn unavailable<T>() -> AppResult<T> {
        Err(AppError::msg("vpn is only available on Windows"))
    }
    pub fn entries() -> AppResult<Vec<Entry>> {
        unavailable()
    }
    pub fn active() -> Vec<Active> {
        Vec::new()
    }
    pub fn dial_kind(_: &Entry) -> AppResult<Dial> {
        unavailable()
    }
    pub fn dial(_: &Entry) -> Result<(), u32> {
        Err(0)
    }
    pub fn dialog(_: &Entry) -> AppResult<()> {
        unavailable()
    }
    pub fn hang_up(_: &str) -> AppResult<()> {
        unavailable()
    }
    pub fn error_text(code: u32) -> String {
        format!("RAS error {code}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, all_users: bool) -> Entry {
        Entry { name: name.into(), phonebook: if all_users { "C:\\ProgramData\\pbk" } else { "C:\\Users\\me\\pbk" }.into(), all_users }
    }

    #[test]
    fn wide_fields() {
        let mut buf = [0xFFFFu16; 8];
        assert!(put_field(&mut buf, "Work"));
        assert_eq!(wide_field(&buf), "Work");
        assert!(buf[4..].iter().all(|c| *c == 0));
        assert!(!put_field(&mut buf, "eight ch"), "no room for the NUL");
        assert_eq!(wide_field(&[0x57, 0x6F]), "Wo"); // no NUL: the whole field
    }

    #[test]
    fn profiles_merge_phonebooks_and_state() {
        let entries = [entry("Work", true), entry("work", false), entry("Home lab", false), entry("", false)];
        let active = [Active { name: "WORK".into(), connected: true }];
        let v = profiles_json(&entries, &active);
        let p = v["profiles"].as_array().unwrap();
        assert_eq!(p.len(), 2, "one per name, empty names skipped: {v}");
        assert_eq!(p[0]["name"], "Home lab");
        assert_eq!(p[0]["connected"], false);
        assert_eq!(p[1]["name"], "work", "the user's entry wins");
        assert_eq!(p[1]["connected"], true);
        assert_eq!(p[1]["connecting"], false);
        let v = profiles_json(&entries[2..3], &[Active { name: "Home lab".into(), connected: false }]);
        assert_eq!(v["profiles"][0]["connecting"], true);
        assert_eq!(profiles_json(&[], &[]), json!({ "profiles": [] }));
    }

    #[test]
    fn dial_choice() {
        assert_eq!(dial_kind(0, 0, true), Dial::Saved);
        assert_eq!(dial_kind(0, 0, false), Dial::Dialog);
        assert_eq!(dial_kind(OPT_REQUIRE_EAP, 0, true), Dial::Dialog);
        assert_eq!(dial_kind(0, OPT2_MACHINE_CERTS, true), Dial::Dialog);
        assert_eq!(dial_kind(0, OPT2_THIRD_PARTY, true), Dial::Settings);
        assert_eq!(dial_kind(OPT_REQUIRE_EAP, OPT2_THIRD_PARTY, false), Dial::Settings);
        assert!(needs_dialog(691) && needs_dialog(703) && needs_dialog(13801));
        assert!(!needs_dialog(0) && !needs_dialog(800), "server unreachable: the dialog would not help");
    }

    #[cfg(windows)]
    #[test]
    fn option_bits_match_windows() {
        use windows::Win32::NetworkManagement::Rras::{RASEO2_IsThirdPartyProfile, RASEO2_RequireMachineCertificates, RASEO_RequireEAP};
        assert_eq!(OPT_REQUIRE_EAP, RASEO_RequireEAP);
        assert_eq!(OPT2_MACHINE_CERTS, RASEO2_RequireMachineCertificates);
        assert_eq!(OPT2_THIRD_PARTY, RASEO2_IsThirdPartyProfile);
    }

    /// `cargo test live_vpn -- --ignored --nocapture`: read-only, the
    /// profiles and connections here. Never dials.
    #[test]
    #[ignore]
    fn live_vpn() {
        println!("vpn: {:?}", read().map(|v| v.to_string()));
    }
}
