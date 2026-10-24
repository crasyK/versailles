//! Recently opened files (#57): Start's "Recommended" list.
//!
//! Windows drops a shortcut into `%APPDATA%\Microsoft\Windows\Recent` for
//! each file or folder opened through the shell. Newest first by the
//! shortcut's write time, targets read with `IShellLinkW` (no `Resolve`, so
//! nothing searches the disk or the network), network paths and missing
//! targets skipped.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_LIMIT: usize = 30;
pub const MAX_LIMIT: usize = 100;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RecentFile {
    /// `"file"` or `"folder"`.
    pub kind: &'static str,
    pub path: String,
    pub name: String,
    /// The folder it is in.
    pub folder: String,
    /// When it was opened (the shortcut's write time), ms since 1970 (UTC).
    pub opened: i64,
}

/// One shortcut in the Recent folder.
#[derive(Debug, Clone)]
pub(crate) struct Shortcut {
    pub lnk: PathBuf,
    pub opened: i64,
}

fn recent_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Microsoft").join("Windows").join("Recent"))
}

/// Newest first; on a tie, by name so the order is stable.
pub(crate) fn order(mut list: Vec<Shortcut>) -> Vec<Shortcut> {
    list.sort_by(|a, b| b.opened.cmp(&a.opened).then_with(|| a.lnk.cmp(&b.lnk)));
    list
}

fn ms(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn shortcuts(dir: &Path) -> Vec<Shortcut> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .map(|e| Shortcut {
            opened: e.metadata().ok().and_then(|m| m.modified().ok()).map(ms).unwrap_or(0),
            lnk: e.path(),
        })
        .collect()
}

/// Turn resolved targets into results: network paths, missing targets and
/// duplicates skipped, at most `limit`. `resolve` reads a shortcut's target.
pub(crate) fn collect(
    list: Vec<Shortcut>,
    limit: usize,
    mut resolve: impl FnMut(&Path) -> Option<String>,
) -> Vec<RecentFile> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for s in order(list) {
        if out.len() >= limit {
            break;
        }
        let Some(target) = resolve(&s.lnk) else {
            continue;
        };
        // Never touch a network path: even a stat sends the user's NTLM hash.
        if target.is_empty() || crate::apps::denied_target(&target).is_some() || !Path::new(&target).is_absolute() {
            continue;
        }
        if !seen.insert(target.to_lowercase()) {
            continue;
        }
        let p = Path::new(&target);
        let Ok(meta) = std::fs::metadata(p) else {
            continue; // gone
        };
        out.push(RecentFile {
            kind: if meta.is_dir() { "folder" } else { "file" },
            name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| target.clone()),
            folder: p.parent().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            path: target,
            opened: s.opened,
        });
    }
    out
}

/// Expand `%VAR%`s in a raw shortcut path (unknown ones stay as they are).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn expand(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `limit` most recently opened files and folders that still exist.
pub fn recent_files(limit: usize) -> Result<Vec<RecentFile>, String> {
    let dir = recent_dir().ok_or("no Recent folder")?;
    let limit = limit.clamp(1, MAX_LIMIT);
    imp::with_resolver(|resolve| collect(shortcuts(&dir), limit, resolve))
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::path::Path;
    use windows::core::{Interface, HRESULT, HSTRING};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    use super::expand;

    pub fn with_resolver<T>(f: impl FnOnce(&mut dyn FnMut(&Path) -> Option<String>) -> T) -> Result<T, String> {
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let result = (|| {
            let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| format!("recent: {e}"))?;
            let file: IPersistFile = link.cast().map_err(|e| format!("recent: {e}"))?;
            let mut resolve = |lnk: &Path| -> Option<String> {
                unsafe {
                    file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
                    let mut buf = [0u16; 1024];
                    // RAWPATH: no resolving (no disk or network search).
                    // `IShellLinkW::GetPath` is the first method after
                    // IUnknown's three; the crate projects it only with the
                    // `Win32_Storage_FileSystem` feature (for the unused
                    // WIN32_FIND_DATAW argument), so call the slot directly.
                    type GetPath = unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut c_void, u32) -> HRESULT;
                    let vtable = *(link.as_raw() as *const *const usize);
                    let get_path: GetPath = std::mem::transmute(*vtable.add(3));
                    get_path(link.as_raw(), buf.as_mut_ptr(), buf.len() as i32, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
                        .ok()
                        .ok()?;
                    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                    let raw = String::from_utf16_lossy(&buf[..end]);
                    (!raw.is_empty()).then(|| expand(&raw))
                }
            };
            Ok(f(&mut resolve))
        })();
        if com.is_ok() {
            unsafe { CoUninitialize() };
        }
        result
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;
    pub fn with_resolver<T>(_: impl FnOnce(&mut dyn FnMut(&Path) -> Option<String>) -> T) -> Result<T, String> {
        Err("recent files are only available on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, opened: i64) -> Shortcut {
        Shortcut { lnk: PathBuf::from(format!(r"C:\R\{name}.lnk")), opened }
    }

    #[test]
    fn newest_first_with_a_stable_tie_break() {
        let out = order(vec![s("b", 10), s("a", 30), s("c", 10), s("d", 20)]);
        let names: Vec<String> =
            out.iter().map(|x| x.lnk.file_stem().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["a", "d", "b", "c"]);
    }

    /// A real shortcut, written and read back through the shell.
    #[cfg(windows)]
    #[test]
    fn reads_a_shortcut_target() {
        use windows::core::{Interface, HSTRING};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        let dir = std::env::temp_dir().join(format!("versailles-lnk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target file.txt");
        std::fs::write(&target, "x").unwrap();
        let lnk = dir.join("target file.txt.lnk");
        std::thread::spawn({
            let (target, lnk) = (target.clone(), lnk.clone());
            move || unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                link.SetPath(&HSTRING::from(target.as_os_str())).unwrap();
                link.cast::<IPersistFile>().unwrap().Save(&HSTRING::from(lnk.as_os_str()), true).unwrap();
            }
        })
        .join()
        .unwrap();
        let list = vec![Shortcut { lnk: lnk.clone(), opened: 5 }];
        let out = std::thread::spawn(move || imp::with_resolver(|resolve| collect(list, 10, resolve)))
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].path.to_lowercase(), target.to_string_lossy().to_lowercase());
        assert_eq!(out[0].name, "target file.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expands_environment_variables() {
        std::env::set_var("VERSAILLES_RECENT_TEST", r"C:\Data");
        assert_eq!(expand(r"%VERSAILLES_RECENT_TEST%\a.txt"), r"C:\Data\a.txt");
        assert_eq!(expand(r"C:\100%\x"), r"C:\100%\x");
        assert_eq!(expand(r"%NO_SUCH_VAR_VERSAILLES%\x"), r"%NO_SUCH_VAR_VERSAILLES%\x");
        assert_eq!(expand("plain"), "plain");
    }

    #[test]
    fn skips_missing_network_and_duplicate_targets() {
        let dir = std::env::temp_dir().join(format!("versailles-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, "x").unwrap();
        let file_s = file.to_string_lossy().into_owned();
        let dir_s = dir.to_string_lossy().into_owned();
        let targets = std::collections::HashMap::from([
            ("new", file_s.clone()),
            ("dup", file_s.to_uppercase()),
            ("unc", r"\\server\share\x.docx".to_string()),
            ("gone", dir.join("deleted.txt").to_string_lossy().into_owned()),
            ("folder", dir_s.clone()),
            ("relative", "notes.txt".to_string()),
        ]);
        let list = vec![s("dup", 50), s("new", 60), s("unc", 55), s("gone", 40), s("folder", 30), s("relative", 20), s("blank", 10)];
        let out = collect(list, 30, |lnk| targets.get(lnk.file_stem()?.to_str()?).cloned());
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0].path, file_s);
        assert_eq!(out[0].kind, "file");
        assert_eq!(out[0].name, "notes.txt");
        assert_eq!(out[0].opened, 60);
        assert_eq!(out[1].path, dir_s);
        assert_eq!(out[1].kind, "folder");
        // The limit counts results, not shortcuts.
        let one = collect(vec![s("gone", 9), s("new", 1)], 1, |lnk| targets.get(lnk.file_stem()?.to_str()?).cloned());
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].path, file_s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
