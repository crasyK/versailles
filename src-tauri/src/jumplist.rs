//! Jump lists (#68): an app window's tasks, custom categories and
//! recent / frequent items, as Explorer's right-click menu on a taskbar
//! button shows them, and opening one.
//!
//! Windows has no API for reading another app's whole jump list; Versailles
//! puts it together from three parts, read-only and in process:
//!
//! - **The app's id.** `IApplicationResolver::GetAppIDForWindow`, the
//!   undocumented shell object Explorer groups taskbar buttons with (also
//!   used by Open-Shell and 7+ Taskbar Tweaker), else the window's own
//!   `System.AppUserModel.ID` (`SHGetPropertyStoreForWindow`).
//! - **Recent and frequent items:** `IApplicationDocumentLists`, the
//!   documented reader for an app's automatic destinations.
//! - **Tasks and custom categories:** what the app set with
//!   `ICustomDestinationList`, stored in
//!   `%APPDATA%\Microsoft\Windows\Recent\CustomDestinations\<hash>.customDestinations-ms`
//!   (hash: CRC-64 of the upper-cased id, see [`appid_hash`]). The container
//!   is undocumented but simple (libyal's "Jump lists format"); each entry is
//!   a shell link that Windows' own `ShellLink` object reads back
//!   (`IPersistStream::Load`), so the link format is never parsed here.
//!   Only shell links are loaded: an entry of another class ends the read.

use serde::Serialize;

/// The jump list file name of app id `appid`: CRC-64 (reflected polynomial
/// 0x92C64265D32139A4, initial value all ones, no final xor) of its
/// upper-cased UTF-16LE text, in lower-case hex without leading zeros.
/// Checked: `Microsoft.Windows.Explorer` → `f01b4d95cf55d32a`.
pub fn appid_hash(appid: &str) -> String {
    let mut crc = u64::MAX;
    for unit in appid.to_uppercase().encode_utf16() {
        for byte in unit.to_le_bytes() {
            crc ^= byte as u64;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0x92C6_4265_D321_39A4 } else { crc >> 1 };
            }
        }
    }
    format!("{crc:x}")
}

/// A category of a `.customDestinations-ms` file.
#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    /// The app's own category, with its name.
    Custom(String),
    /// Where Windows' Recent (2) or Frequent (1) list goes.
    Known(u32),
    /// The app's tasks.
    Tasks,
}

const FOOTER: u32 = 0xBABF_FBAB;
const MAX_SECTIONS: u32 = 64;
const MAX_ENTRIES: u32 = 256;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// Read a `.customDestinations-ms` file. Each entry is a CLSID and the
/// object's data; `load(clsid, rest)` reads one object from the start of
/// `rest` and says how many bytes it took. Reading stops at the first thing
/// it cannot follow (a damaged file, an object `load` refuses), keeping what
/// came before.
pub fn parse_custom<T>(data: &[u8], mut load: impl FnMut(&[u8; 16], &[u8]) -> Option<(usize, T)>) -> Vec<(Section, Vec<T>)> {
    let mut out = Vec::new();
    if u32_at(data, 0) != Some(2) {
        return out;
    }
    let Some(sections) = u32_at(data, 4) else {
        return out;
    };
    let mut at = 12;
    for _ in 0..sections.min(MAX_SECTIONS) {
        let Some(kind) = u32_at(data, at) else {
            return out;
        };
        at += 4;
        let (section, count) = match kind {
            0 => {
                let Some(len) = u16_at(data, at) else {
                    return out;
                };
                at += 2;
                let Some(raw) = data.get(at..at + len as usize * 2) else {
                    return out;
                };
                at += len as usize * 2;
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                let Some(count) = u32_at(data, at) else {
                    return out;
                };
                at += 4;
                (Section::Custom(String::from_utf16_lossy(&units)), count)
            }
            1 => {
                let Some(id) = u32_at(data, at) else {
                    return out;
                };
                at += 4;
                (Section::Known(id), 0)
            }
            2 => {
                let Some(count) = u32_at(data, at) else {
                    return out;
                };
                at += 4;
                (Section::Tasks, count)
            }
            _ => return out,
        };
        let mut items = Vec::new();
        for _ in 0..count.min(MAX_ENTRIES) {
            let Some(clsid) = data.get(at..at + 16).and_then(|c| <[u8; 16]>::try_from(c).ok()) else {
                out.push((section, items));
                return out;
            };
            at += 16;
            match load(&clsid, &data[at..]) {
                Some((used, item)) if used > 0 && at + used <= data.len() => {
                    at += used;
                    items.push(item);
                }
                _ => {
                    out.push((section, items));
                    return out;
                }
            }
        }
        out.push((section, items));
        if u32_at(data, at) != Some(FOOTER) {
            return out;
        }
        at += 4;
    }
    out
}

/// `CLSID_ShellLink` as it is stored (a GUID's little-endian bytes).
pub const SHELL_LINK: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// One entry as pages see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    /// Pass it to `openJump` (`"task:0"`, `"cat:1:3"`, `"recent:2"`,
    /// `"frequent:0"`); valid until the list changes.
    pub key: String,
    pub title: String,
    /// The file, folder or program it opens, when it has a path.
    pub path: Option<String>,
    /// A line between tasks (nothing to open).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub separator: bool,
    /// 16×16 PNG data URL, only when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// One custom category.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Category {
    pub name: String,
    pub items: Vec<Item>,
}

/// A window's jump list.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JumpList {
    /// The app's id (AppUserModelID, or the one Windows derives from its path).
    pub app_id: String,
    pub tasks: Vec<Item>,
    pub categories: Vec<Category>,
    pub recent: Vec<Item>,
    pub frequent: Vec<Item>,
    /// Windows' own lists the app shows, in its order: `"recent"`,
    /// `"frequent"`. An app that set no list of its own shows `recent`.
    pub known: Vec<&'static str>,
}

/// Where a key points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Task(usize),
    Category(usize, usize),
    Recent(usize),
    Frequent(usize),
}

impl Key {
    pub fn parse(key: &str) -> Option<Key> {
        let parts: Vec<&str> = key.trim().split(':').collect();
        let num = |i: usize| -> Option<usize> { parts.get(i)?.parse().ok() };
        Some(match (parts.first().copied()?, parts.len()) {
            ("task", 2) => Key::Task(num(1)?),
            ("cat", 3) => Key::Category(num(1)?, num(2)?),
            ("recent", 2) => Key::Recent(num(1)?),
            ("frequent", 2) => Key::Frequent(num(1)?),
            _ => return None,
        })
    }

    pub fn text(&self) -> String {
        match self {
            Key::Task(i) => format!("task:{i}"),
            Key::Category(c, i) => format!("cat:{c}:{i}"),
            Key::Recent(i) => format!("recent:{i}"),
            Key::Frequent(i) => format!("frequent:{i}"),
        }
    }
}

/// The known lists named in the file's sections, in order; `recent` when the
/// app set no list (no file).
pub fn known_lists(sections: Option<&[Section]>) -> Vec<&'static str> {
    let Some(sections) = sections else {
        return vec!["recent"];
    };
    let mut out = Vec::new();
    for s in sections {
        let name = match s {
            Section::Known(1) => "frequent",
            Section::Known(2) => "recent",
            _ => continue,
        };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// How many recent / frequent items to read (Explorer shows about ten).
pub const KNOWN_ITEMS: u32 = 12;

/// The jump list of listed window `id`; `icons` adds 16×16 icons.
pub fn read(id: i64, icons: bool) -> crate::error::AppResult<JumpList> {
    crate::app_windows::check_listed(id)?;
    imp::read(id, icons)
}

/// Open entry `key` of window `id`'s jump list, as clicking it in Explorer's
/// jump list does.
pub fn open(id: i64, key: &str) -> crate::error::AppResult<()> {
    crate::app_windows::check_listed(id)?;
    let key = Key::parse(key).ok_or_else(|| crate::error::AppError::msg(format!("jump list: bad key '{key}'")))?;
    imp::open(id, key)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::error::{AppError, AppResult};
    use std::ffi::c_void;
    use windows::core::{Interface, GUID, HRESULT, HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{HWND, PROPERTYKEY};
    use windows::Win32::System::Com::StructuredStorage::{PropVariantToBoolean, PropVariantToStringAlloc};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistStream, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, STREAM_SEEK_CUR,
    };
    use windows::Win32::UI::Shell::Common::IObjectArray;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
    use windows::Win32::UI::Shell::{
        ApplicationDocumentLists, IApplicationDocumentLists, IShellItem, IShellLinkW, SHCreateMemStream,
        SHGetIDListFromObject, SHLoadIndirectString, ShellExecuteExW, ShellLink, ADLT_FREQUENT, ADLT_RECENT,
        APPDOCLISTTYPE, SEE_MASK_FLAG_NO_UI, SEE_MASK_INVOKEIDLIST, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
        SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// The shell's application resolver (undocumented; Windows 8 and later).
    /// Signatures as 7+ Taskbar Tweaker and Open-Shell declare them; only
    /// the first three methods are used.
    mod resolver {
        #![allow(non_snake_case)]
        use windows::core::{BOOL, HRESULT, PWSTR};
        use windows::Win32::Foundation::HWND;
        #[windows_core::interface("de25675a-72de-44b4-9373-05170450c140")]
        pub unsafe trait IApplicationResolver: windows::core::IUnknown {
            fn GetAppIDForShortcut(&self, item: *mut core::ffi::c_void, id: *mut PWSTR) -> HRESULT;
            fn GetAppIDForShortcutObject(
                &self,
                link: *mut core::ffi::c_void,
                item: *mut core::ffi::c_void,
                id: *mut PWSTR,
            ) -> HRESULT;
            fn GetAppIDForWindow(
                &self,
                hwnd: HWND,
                id: *mut PWSTR,
                pinning_prevented: *mut BOOL,
                explicit: *mut BOOL,
                embedded_shortcut_valid: *mut BOOL,
            ) -> HRESULT;
        }

        /// The generated methods are private to this module.
        pub unsafe fn for_window(r: &IApplicationResolver, hwnd: HWND) -> windows::core::Result<PWSTR> {
            let mut id = PWSTR::null();
            let (mut a, mut b, mut c) = (BOOL(0), BOOL(0), BOOL(0));
            r.GetAppIDForWindow(hwnd, &mut id, &mut a, &mut b, &mut c).ok()?;
            Ok(id)
        }
    }

    const CLSID_APPLICATION_RESOLVER: GUID = GUID::from_u128(0x660b90c8_73a9_4b58_8cae_355b7f55341b);
    /// `System.AppUserModel.ID`.
    const PKEY_APP_ID: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };
    /// `System.AppUserModel.IsDestListSeparator`.
    const PKEY_SEPARATOR: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 6,
    };
    /// `System.Title`: a task's name.
    const PKEY_TITLE: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xf29f85e0_4ff9_1068_ab91_08002b27b3d9),
        pid: 2,
    };
    /// A jump list file larger than this is not read.
    const MAX_FILE: u64 = 4 << 20;
    const ICON_PX: i32 = 16;

    fn err(context: &str) -> impl Fn(windows::core::Error) -> AppError + '_ {
        move |e| AppError::msg(format!("jump list: {context}: {e}"))
    }

    /// Shell objects want a single-threaded apartment: run `f` on a thread
    /// of its own (the caller is a blocking pool thread).
    fn on_sta<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
        std::thread::spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
            let out = f();
            if com.is_ok() {
                unsafe { CoUninitialize() };
            }
            out
        })
        .join()
        .map_err(|_| AppError::msg("jump list: reader failed"))?
    }

    unsafe fn take(p: PWSTR) -> Option<String> {
        if p.is_null() {
            return None;
        }
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s.filter(|s| !s.is_empty())
    }

    fn app_id(id: i64) -> Option<String> {
        let h = HWND(id as isize as *mut c_void);
        unsafe {
            let resolver: windows::core::Result<resolver::IApplicationResolver> =
                CoCreateInstance(&CLSID_APPLICATION_RESOLVER, None, CLSCTX_INPROC_SERVER);
            if let Some(found) = resolver.ok().and_then(|r| resolver::for_window(&r, h).ok()).and_then(|p| take(p)) {
                return Some(found);
            }
            let store: IPropertyStore = SHGetPropertyStoreForWindow(h).ok()?;
            let value = store.GetValue(&PKEY_APP_ID).ok()?;
            take(PropVariantToStringAlloc(&value).ok()?)
        }
    }

    fn string_prop(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
        unsafe {
            let value = store.GetValue(key).ok()?;
            take(PropVariantToStringAlloc(&value).ok()?)
        }
    }

    /// `@dll,-id` strings (a task title from a resource) in the user's language.
    fn indirect(text: String) -> String {
        if !text.starts_with('@') {
            return text;
        }
        let mut buf = [0u16; 512];
        match unsafe { SHLoadIndirectString(&HSTRING::from(text.as_str()), &mut buf, None) } {
            Ok(()) => {
                let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                String::from_utf16_lossy(&buf[..end])
            }
            Err(_) => text,
        }
    }

    fn wide(buf: &[u16]) -> String {
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    /// `IShellLinkW::GetPath`: the crate projects it only with the
    /// `Win32_Storage_FileSystem` feature (for an unused argument), so call
    /// its slot (the first after IUnknown's three), as `recent` does.
    fn link_path(link: &IShellLinkW) -> Option<String> {
        type GetPath = unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut c_void, u32) -> HRESULT;
        let mut buf = [0u16; 1024];
        unsafe {
            let vtable = *(link.as_raw() as *const *const usize);
            let get_path: GetPath = std::mem::transmute(*vtable.add(3));
            get_path(link.as_raw(), buf.as_mut_ptr(), buf.len() as i32, std::ptr::null_mut(), 0).ok().ok()?;
        }
        Some(wide(&buf)).filter(|s| !s.is_empty())
    }

    /// A link from the file, and what a page sees of it.
    struct Entry {
        link: IShellLinkW,
        title: String,
        path: Option<String>,
        separator: bool,
    }

    /// Read one shell link from the start of `rest` (see `parse_custom`).
    fn load_link(clsid: &[u8; 16], rest: &[u8]) -> Option<(usize, Entry)> {
        if *clsid != SHELL_LINK {
            return None;
        }
        unsafe {
            let stream = SHCreateMemStream(Some(rest))?;
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            link.cast::<IPersistStream>().ok()?.Load(&stream).ok()?;
            let mut used = 0u64;
            stream.Seek(0, STREAM_SEEK_CUR, Some(&mut used)).ok()?;
            let store = link.cast::<IPropertyStore>().ok();
            let separator = store
                .as_ref()
                .and_then(|s| s.GetValue(&PKEY_SEPARATOR).ok())
                .and_then(|v| PropVariantToBoolean(&v).ok())
                .is_some_and(|b| b.as_bool());
            let path = link_path(&link);
            let title = store
                .as_ref()
                .and_then(|s| string_prop(s, &PKEY_TITLE))
                .map(indirect)
                .or_else(|| {
                    let mut buf = [0u16; 512];
                    link.GetDescription(&mut buf).ok().map(|_| wide(&buf)).filter(|s| !s.is_empty())
                })
                .or_else(|| {
                    path.as_deref()
                        .and_then(|p| std::path::Path::new(p).file_stem())
                        .map(|s| s.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            Some((used as usize, Entry { link, title, path, separator }))
        }
    }

    fn custom_file(app_id: &str) -> Option<std::path::PathBuf> {
        let dir = dirs::data_dir()?.join(r"Microsoft\Windows\Recent\CustomDestinations");
        Some(dir.join(format!("{}.customDestinations-ms", appid_hash(app_id))))
    }

    /// The app's own lists, or `None` when it set none.
    fn custom(app_id: &str) -> Option<Vec<(Section, Vec<Entry>)>> {
        let file = custom_file(app_id)?;
        if std::fs::metadata(&file).ok()?.len() > MAX_FILE {
            return None;
        }
        let data = std::fs::read(&file).ok()?;
        Some(parse_custom(&data, load_link))
    }

    /// A recent or frequent entry: a shell item (or, rarely, a link).
    struct Known {
        item: Option<IShellItem>,
        link: Option<IShellLinkW>,
        title: String,
        path: Option<String>,
    }

    fn known(app_id: &str, list: APPDOCLISTTYPE) -> Vec<Known> {
        let mut out = Vec::new();
        unsafe {
            let docs: IApplicationDocumentLists =
                match CoCreateInstance(&ApplicationDocumentLists, None, CLSCTX_INPROC_SERVER) {
                    Ok(d) => d,
                    Err(_) => return out,
                };
            if docs.SetAppID(&HSTRING::from(app_id)).is_err() {
                return out;
            }
            let Ok(items) = docs.GetList::<IObjectArray>(list, KNOWN_ITEMS) else {
                return out;
            };
            for i in 0..items.GetCount().unwrap_or(0) {
                if let Ok(item) = items.GetAt::<IShellItem>(i) {
                    let title = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok().and_then(|p| take(p)).unwrap_or_default();
                    let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok().and_then(|p| take(p));
                    out.push(Known { item: Some(item), link: None, title, path });
                } else if let Ok(link) = items.GetAt::<IShellLinkW>(i) {
                    let path = link_path(&link);
                    let title = path
                        .as_deref()
                        .and_then(|p| std::path::Path::new(p).file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    out.push(Known { item: None, link: Some(link), title, path });
                }
            }
        }
        out
    }

    fn icon_of(path: Option<&str>, icons: bool) -> Option<String> {
        if !icons {
            return None;
        }
        crate::apps::path_icon(path?, ICON_PX)
    }

    fn item(key: Key, title: &str, path: Option<&String>, separator: bool, icons: bool) -> Item {
        Item {
            key: key.text(),
            title: title.to_string(),
            path: path.cloned(),
            separator,
            icon: if separator { None } else { icon_of(path.map(|p| p.as_str()), icons) },
        }
    }

    fn known_items(list: &[Known], make: fn(usize) -> Key, icons: bool) -> Vec<Item> {
        list.iter().enumerate().map(|(i, k)| item(make(i), &k.title, k.path.as_ref(), false, icons)).collect()
    }

    pub fn read(id: i64, icons: bool) -> AppResult<JumpList> {
        on_sta(move || {
            let app_id = app_id(id).ok_or_else(|| AppError::msg("jump list: Windows knows no app id for this window"))?;
            let sections = custom(&app_id);
            let mut tasks = Vec::new();
            let mut categories = Vec::new();
            for (section, entries) in sections.iter().flatten() {
                match section {
                    Section::Tasks => tasks.extend(
                        entries
                            .iter()
                            .enumerate()
                            .map(|(i, e)| item(Key::Task(i), &e.title, e.path.as_ref(), e.separator, icons)),
                    ),
                    Section::Custom(name) => {
                        let c = categories.len();
                        categories.push(Category {
                            name: indirect(name.clone()),
                            items: entries
                                .iter()
                                .enumerate()
                                .map(|(i, e)| item(Key::Category(c, i), &e.title, e.path.as_ref(), e.separator, icons))
                                .collect(),
                        });
                    }
                    Section::Known(_) => {}
                }
            }
            let names: Option<Vec<Section>> = sections.as_ref().map(|s| s.iter().map(|(s, _)| s.clone()).collect());
            Ok(JumpList {
                recent: known_items(&known(&app_id, ADLT_RECENT), Key::Recent, icons),
                frequent: known_items(&known(&app_id, ADLT_FREQUENT), Key::Frequent, icons),
                known: known_lists(names.as_deref()),
                app_id,
                tasks,
                categories,
            })
        })
    }

    /// Run `pidl` (with a link's arguments, folder and window state) the
    /// way Explorer opens a jump list entry.
    unsafe fn execute(pidl: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST, args: &[u16], dir: &[u16], show: i32) -> AppResult<()> {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpIDList: pidl as *mut c_void,
            lpParameters: if args.first().is_some_and(|c| *c != 0) { PCWSTR(args.as_ptr()) } else { PCWSTR::null() },
            lpDirectory: if dir.first().is_some_and(|c| *c != 0) { PCWSTR(dir.as_ptr()) } else { PCWSTR::null() },
            nShow: show,
            ..Default::default()
        };
        let result = ShellExecuteExW(&mut info).map_err(err("open"));
        CoTaskMemFree(Some(pidl as *const c_void));
        result
    }

    unsafe fn open_link(link: &IShellLinkW) -> AppResult<()> {
        let pidl = link.GetIDList().map_err(err("link target"))?;
        let mut args = [0u16; 2048];
        let mut dir = [0u16; 1024];
        let _ = link.GetArguments(&mut args);
        let _ = link.GetWorkingDirectory(&mut dir);
        let show = link.GetShowCmd().map(|s| s.0).unwrap_or(SW_SHOWNORMAL.0);
        execute(pidl, &args, &dir, if show == 0 { SW_SHOWNORMAL.0 } else { show })
    }

    pub fn open(id: i64, key: Key) -> AppResult<()> {
        on_sta(move || unsafe {
            let app_id = app_id(id).ok_or_else(|| AppError::msg("jump list: Windows knows no app id for this window"))?;
            let gone = || AppError::msg(format!("jump list: no entry {} (the list changed)", key.text()));
            let known_entry = |list, i: usize| -> AppResult<()> {
                let entries = known(&app_id, list);
                let k = entries.get(i).ok_or_else(gone)?;
                match (&k.item, &k.link) {
                    (Some(item), _) => {
                        let pidl = SHGetIDListFromObject(item).map_err(err("item"))?;
                        execute(pidl, &[0], &[0], SW_SHOWNORMAL.0)
                    }
                    (None, Some(link)) => open_link(link),
                    (None, None) => Err(gone()),
                }
            };
            match key {
                Key::Recent(i) => known_entry(ADLT_RECENT, i),
                Key::Frequent(i) => known_entry(ADLT_FREQUENT, i),
                Key::Task(_) | Key::Category(..) => {
                    let sections = custom(&app_id).ok_or_else(gone)?;
                    let entry = match key {
                        Key::Task(i) => sections.iter().filter(|(s, _)| *s == Section::Tasks).flat_map(|(_, e)| e).nth(i),
                        Key::Category(c, i) => sections
                            .iter()
                            .filter(|(s, _)| matches!(s, Section::Custom(_)))
                            .nth(c)
                            .and_then(|(_, e)| e.get(i)),
                        _ => None,
                    }
                    .ok_or_else(gone)?;
                    if entry.separator {
                        return Err(AppError::msg("jump list: a separator opens nothing"));
                    }
                    open_link(&entry.link)
                }
            }
        })
    }

    #[cfg(test)]
    pub fn app_id_of(id: i64) -> Option<String> {
        on_sta(move || Ok(app_id(id))).ok().flatten()
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use crate::error::{AppError, AppResult};
    pub fn read(_id: i64, _icons: bool) -> AppResult<JumpList> {
        Err(AppError::msg("jump lists are only available on Windows"))
    }
    pub fn open(_id: i64, _key: Key) -> AppResult<()> {
        Err(AppError::msg("jump lists are only available on Windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_app_ids_like_windows() {
        // File names seen in a real profile's Recent folders.
        assert_eq!(appid_hash("Microsoft.Windows.Explorer"), "f01b4d95cf55d32a");
        assert_eq!(appid_hash("Chrome"), "5d696d521de238c3");
        assert_eq!(appid_hash("MSEdge"), "ccba5a5986c77e43");
        assert_eq!(appid_hash("Anysphere.Cursor"), "8346f26264213ffa");
        // Case does not matter.
        assert_eq!(appid_hash("chrome"), appid_hash("CHROME"));
    }

    /// A file as `ICustomDestinationList` writes it, with `n`-byte fake objects.
    fn file(sections: &[(u32, &str, &[&[u8]])]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(2u32.to_le_bytes());
        b.extend((sections.len() as u32).to_le_bytes());
        b.extend(0u32.to_le_bytes());
        for (kind, name, entries) in sections {
            b.extend(kind.to_le_bytes());
            match kind {
                0 => {
                    let units: Vec<u16> = name.encode_utf16().collect();
                    b.extend((units.len() as u16).to_le_bytes());
                    units.iter().for_each(|u| b.extend(u.to_le_bytes()));
                    b.extend((entries.len() as u32).to_le_bytes());
                }
                1 => b.extend(2u32.to_le_bytes()),
                _ => b.extend((entries.len() as u32).to_le_bytes()),
            }
            for e in entries.iter() {
                b.extend(SHELL_LINK);
                b.extend(*e);
            }
            b.extend(FOOTER.to_le_bytes());
        }
        b
    }

    /// Fake objects: a length byte, then that many bytes.
    fn fake(clsid: &[u8; 16], rest: &[u8]) -> Option<(usize, Vec<u8>)> {
        if *clsid != SHELL_LINK {
            return None;
        }
        let n = *rest.first()? as usize;
        Some((n + 1, rest.get(1..n + 1)?.to_vec()))
    }

    #[test]
    fn reads_tasks_categories_and_known_lists() {
        let data = file(&[
            (2, "", &[&[2, 7, 7], &[1, 9]]),
            (0, "Pinned", &[&[1, 5]]),
            (1, "", &[]),
        ]);
        let got = parse_custom(&data, fake);
        assert_eq!(
            got,
            vec![
                (Section::Tasks, vec![vec![7, 7], vec![9]]),
                (Section::Custom("Pinned".into()), vec![vec![5]]),
                (Section::Known(2), vec![]),
            ]
        );
        let names: Vec<Section> = got.into_iter().map(|(s, _)| s).collect();
        assert_eq!(known_lists(Some(&names)), vec!["recent"]);
        assert_eq!(known_lists(Some(&[Section::Known(1), Section::Known(2)])), vec!["frequent", "recent"]);
        assert_eq!(known_lists(Some(&[Section::Tasks])), Vec::<&str>::new());
        assert_eq!(known_lists(None), vec!["recent"]);
    }

    #[test]
    fn stops_at_what_it_cannot_follow() {
        // Wrong version, empty, truncated.
        assert!(parse_custom(&[1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0], fake).is_empty());
        assert!(parse_custom(&[], fake).is_empty());
        let data = file(&[(2, "", &[&[1, 1], &[1, 2]]), (2, "", &[&[1, 3]])]);
        let cut = &data[..data.len() - 10];
        assert_eq!(parse_custom(cut, fake), vec![(Section::Tasks, vec![vec![1], vec![2]]), (Section::Tasks, vec![])]);
        // An object of another class ends the read, keeping what came before.
        let mut odd = file(&[(2, "", &[&[1, 1], &[1, 2]])]);
        let second = 12 + 8 + 16 + 2;
        odd[second] = 0xFF;
        assert_eq!(parse_custom(&odd, fake), vec![(Section::Tasks, vec![vec![1]])]);
        // A loader that claims more than there is.
        let liar = |_: &[u8; 16], _: &[u8]| Some((10_000usize, ()));
        assert_eq!(parse_custom(&file(&[(2, "", &[&[1, 1]])]), liar), vec![(Section::Tasks, vec![])]);
    }

    #[test]
    fn keys_round_trip() {
        for k in [Key::Task(0), Key::Category(2, 5), Key::Recent(3), Key::Frequent(11)] {
            assert_eq!(Key::parse(&k.text()), Some(k));
        }
        for bad in ["", "task", "task:x", "cat:1", "recent:1:2", "pinned:0", "task:1:"] {
            assert_eq!(Key::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn payload_shape() {
        let list = JumpList {
            app_id: "Chrome".into(),
            tasks: vec![Item { key: "task:0".into(), title: "New window".into(), path: None, separator: false, icon: None }],
            categories: vec![],
            recent: vec![],
            frequent: vec![],
            known: vec!["recent"],
        };
        let v = serde_json::to_value(&list).unwrap();
        assert_eq!(v["appId"], "Chrome");
        assert_eq!(v["tasks"][0]["key"], "task:0");
        assert!(v["tasks"][0]["path"].is_null());
        assert!(v["tasks"][0].get("separator").is_none());
        assert!(v["tasks"][0].get("icon").is_none());
    }

    /// Reads the real desktop: `cargo test --lib live_jump_lists -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_jump_lists() {
        for w in crate::app_windows::list(false).expect("list") {
            println!("{} [{}] app id {:?}", w.title.chars().take(40).collect::<String>(), w.id, imp::app_id_of(w.id));
            match read(w.id, true) {
                Ok(j) => {
                    for t in &j.tasks {
                        println!("  task {} {:?} {:?} icon={}", t.key, t.title, t.path, t.icon.is_some());
                    }
                    for c in &j.categories {
                        println!("  category {:?}: {} items", c.name, c.items.len());
                    }
                    println!("  recent {} frequent {} known {:?}", j.recent.len(), j.frequent.len(), j.known);
                    for r in j.recent.iter().take(3) {
                        println!("    {:?} {:?}", r.title, r.path);
                    }
                }
                Err(e) => println!("  {e}"),
            }
        }
    }
}
