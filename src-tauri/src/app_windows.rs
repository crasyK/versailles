//! App windows: the `windows` hook lists the top-level windows a taskbar
//! would show and pushes their changes (opened, closed, focused, changed);
//! `windows-control` focuses, minimizes, restores, maximizes, closes, moves
//! or resizes one of them.
//!
//! Versailles' own windows, tool windows, owned popups and cloaked windows
//! (other virtual desktops, suspended store apps) are never listed, and
//! control actions only touch windows the list would show.

use crate::error::{AppError, AppResult};
use crate::layout::Rect;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// How often the watcher re-reads the window list while a piece declares
/// `windows`.
pub const POLL: Duration = Duration::from_millis(250);

/// One app window as pages see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// Window handle; pass it back to `windows-control`.
    pub id: i64,
    pub title: String,
    /// Executable name without extension (`chrome`, `Code`).
    pub app: Option<String>,
    /// Full path of the executable.
    pub exe: Option<String>,
    pub pid: u32,
    /// Display number of its monitor (as in Windows display settings).
    pub monitor: Option<u32>,
    pub minimized: bool,
    pub maximized: bool,
    /// The app window with focus. Clicking a Versailles window (a taskbar)
    /// does not take it away.
    pub focused: bool,
    /// Physical screen pixels; for a minimized window, where it restores to.
    pub rect: Rect,
    /// It flashed its taskbar button (wants attention) and has not been
    /// activated since (see `attention`).
    pub attention: bool,
    /// `data:image/png;base64,…` (32×32) when asked for, if the window has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl WindowInfo {
    /// Whether anything a change is pushed for differs. `rect` alone does not
    /// count: dragging a window would flood every listener.
    fn same_state(&self, other: &WindowInfo) -> bool {
        self.title == other.title
            && self.minimized == other.minimized
            && self.maximized == other.maximized
            && self.focused == other.focused
            && self.monitor == other.monitor
            && self.attention == other.attention
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Opened,
    Closed,
    Focused,
    /// Title, minimized / maximized, focus lost, monitor, or attention.
    Changed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub kind: ChangeKind,
    /// The window now (its last known state for `Closed`).
    pub window: WindowInfo,
}

/// Changes from one window list to the next: closed first, then opened,
/// changed and focused (so a listener sees the new focus last).
pub fn diff(prev: &[WindowInfo], next: &[WindowInfo]) -> Vec<Change> {
    let mut out = Vec::new();
    for old in prev {
        if !next.iter().any(|w| w.id == old.id) {
            out.push(Change { kind: ChangeKind::Closed, window: old.clone() });
        }
    }
    let mut focused = Vec::new();
    for now in next {
        match prev.iter().find(|w| w.id == now.id) {
            None => out.push(Change { kind: ChangeKind::Opened, window: now.clone() }),
            Some(old) if old.same_state(now) => {}
            Some(old) if now.focused && !old.focused => {
                focused.push(Change { kind: ChangeKind::Focused, window: now.clone() })
            }
            Some(_) => out.push(Change { kind: ChangeKind::Changed, window: now.clone() }),
        }
    }
    out.extend(focused);
    out
}

/// Push payload for one change: `{ type, id, window }`.
pub fn change_json(change: &Change) -> Value {
    json!({
        "type": change.kind,
        "id": change.window.id,
        "window": change.window,
    })
}

/// What decides whether a top-level window is listed (taskbar rules).
#[derive(Debug, Clone, Copy, Default)]
pub struct Candidate {
    pub visible: bool,
    /// DWM cloaked: on another virtual desktop, or a suspended store app.
    pub cloaked: bool,
    pub tool_window: bool,
    pub app_window: bool,
    pub no_activate: bool,
    pub owned: bool,
    pub has_title: bool,
    /// Belongs to Versailles.
    pub own_process: bool,
    /// The shell's desktop / taskbar windows.
    pub shell: bool,
}

pub fn listed(c: &Candidate) -> bool {
    if !c.visible || c.cloaked || c.own_process || c.shell || !c.has_title {
        return false;
    }
    // WS_EX_APPWINDOW forces a taskbar button; otherwise tool windows,
    // owned popups and no-activate windows have none.
    c.app_window || !(c.tool_window || c.owned || c.no_activate)
}

/// Window classes of the shell itself (wallpaper host, taskbars).
pub fn is_shell_class(class: &str) -> bool {
    matches!(
        class,
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

/// `windows-control` actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Focus,
    Minimize,
    Restore,
    Maximize,
    Close,
    Move,
}

/// Target of `move`: physical screen pixels, like `WindowInfo::rect`. Give
/// `x` and `y` to move, `width` and `height` to resize, or all four.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct MoveTo {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

impl MoveTo {
    /// `(position, size)` to apply; rejects half a position or size.
    pub fn parts(&self) -> AppResult<(Option<(i32, i32)>, Option<(i32, i32)>)> {
        let pos = match (self.x, self.y) {
            (Some(x), Some(y)) => Some((x, y)),
            (None, None) => None,
            _ => return Err(AppError::msg("windows: move needs both x and y")),
        };
        let size = match (self.width, self.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some((w.min(32_000), h.min(32_000))),
            (None, None) => None,
            _ => return Err(AppError::msg("windows: resize needs a positive width and height")),
        };
        if pos.is_none() && size.is_none() {
            return Err(AppError::msg("windows: move needs x / y or width / height"));
        }
        Ok((pos, size))
    }
}

/// Every listed app window, top of the z-order first.
pub fn list(icons: bool) -> AppResult<Vec<WindowInfo>> {
    imp::list(icons)
}

/// The icon of one listed window.
pub fn icon(id: i64) -> AppResult<Option<String>> {
    imp::icon(id)
}

/// Any icon handle (another process's too) as a PNG data URL, drawn at its
/// own size (16–64 px). For the tray, whose icons are handles, not windows.
#[cfg(windows)]
pub fn icon_handle_url(icon: isize) -> Option<String> {
    imp::icon_handle_url(icon)
}

/// Full executable path of a process.
#[cfg(windows)]
pub fn process_path(pid: u32) -> Option<String> {
    imp::process_path(pid)
}

/// An executable's first icon as a 32×32 PNG data URL (the audio mixer).
#[cfg(windows)]
pub fn exe_icon_url(path: &str) -> Option<String> {
    imp::exe_icon_url(path)
}

/// An error unless `id` is a window the list would show.
pub fn check_listed(id: i64) -> AppResult<()> {
    imp::check_listed(id)
}

/// The icon handle a listed window reports (owned by that app: never
/// destroy it), for the hover preview.
#[cfg(windows)]
pub fn icon_handle(id: i64) -> Option<isize> {
    imp::icon_handle(id)
}

/// Run a control action on a listed window (never one of Versailles').
pub fn control(id: i64, action: Action, to: Option<MoveTo>) -> AppResult<()> {
    let to = match action {
        Action::Move => Some(to.ok_or_else(|| AppError::msg("windows: move needs x / y or width / height"))?.parts()?),
        _ => None,
    };
    imp::control(id, action, to)
}

/// Whether some piece on the page declares `windows`.
fn declared(app: &AppHandle) -> bool {
    let state = app.state::<crate::state::AppState>();
    crate::desktop::page_catalog(&state)
        .pieces
        .iter()
        .any(|p| p.hooks.iter().any(|h| h == "windows"))
}

/// Background watcher: while some piece declares `windows`, read the list
/// and push each change to the windows of those pieces.
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: Option<Vec<WindowInfo>> = None;
        // The shell hook for attention runs while the poll does.
        let mut listening = false;
        loop {
            std::thread::sleep(POLL);
            if !declared(&app) {
                // The next declaration starts from a fresh list.
                last = None;
                if listening {
                    listening = false;
                    crate::attention::stop();
                }
                std::thread::sleep(Duration::from_millis(750));
                continue;
            }
            if !listening {
                listening = true;
                crate::attention::start();
            }
            let Ok(now) = list(false) else {
                continue;
            };
            if let Some(prev) = last.as_ref() {
                for change in diff(prev, &now) {
                    crate::push::to_hook(&app, "windows", &change_json(&change));
                }
            }
            last = Some(now);
        }
    });
}

/// PNG of `width` × `height` RGBA pixels (stored, not compressed: icons are
/// small and this needs no compression library).
pub fn png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let row = width as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&rgba[y * row..(y + 1) * row]);
    }
    // zlib stream of stored deflate blocks (at most 65535 bytes each).
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(if i + 1 == blocks.len() { 1 } else { 0 });
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

/// BGRA pixels drawn by GDI (premultiplied alpha) → straight RGBA. With no
/// alpha at all (an old icon), `opaque` (from the icon's mask) decides.
pub fn bgra_to_rgba(bgra: &[u8], opaque: Option<&[bool]>) -> Vec<u8> {
    let has_alpha = bgra.chunks(4).any(|p| p[3] != 0);
    let mut out = Vec::with_capacity(bgra.len());
    for (i, p) in bgra.chunks(4).enumerate() {
        let (b, g, r, a) = (p[0], p[1], p[2], p[3]);
        if has_alpha {
            let un = |c: u8| -> u8 {
                if a == 0 {
                    0
                } else {
                    ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
                }
            };
            out.extend_from_slice(&[un(r), un(g), un(b), a]);
        } else {
            let a = match opaque {
                Some(mask) if !mask.get(i).copied().unwrap_or(true) => 0,
                _ => 255,
            };
            out.extend_from_slice(&[r, g, b, a]);
        }
    }
    out
}

#[cfg(windows)]
mod imp {
    use super::*;
    use base64::Engine;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use windows::core::{BOOL, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW, GetObjectW, BITMAP,
        MonitorFromWindow, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HGDIOBJ, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Shell::ExtractIconExW;
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, DestroyIcon, DrawIconEx, EnumWindows, GetClassLongPtrW, GetClassNameW,
        GetForegroundWindow, GetIconInfo, GetWindow, GetWindowLongPtrW, GetWindowPlacement, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
        PostMessageW, SendMessageTimeoutW, SetForegroundWindow, SetWindowPos, ShowWindow, DI_MASK,
        DI_NORMAL, GCLP_HICON, GCLP_HICONSM, GWL_EXSTYLE, GW_OWNER, HICON, ICONINFO, ICON_BIG, ICON_SMALL2,
        SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_MAXIMIZE,
        SW_MINIMIZE, SW_RESTORE, WINDOWPLACEMENT, WM_CLOSE, WM_GETICON, WS_EX_APPWINDOW,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    const ICON_PX: i32 = 32;

    fn hwnd(id: i64) -> HWND {
        HWND(id as isize as *mut _)
    }

    fn id_of(h: HWND) -> i64 {
        h.0 as isize as i64
    }

    /// The last listed window that had focus. While a Versailles window (a
    /// taskbar being clicked) is in front, that app still counts as focused.
    fn last_focus() -> &'static Mutex<isize> {
        static LAST: OnceLock<Mutex<isize>> = OnceLock::new();
        LAST.get_or_init(|| Mutex::new(0))
    }

    /// Executable path by pid, kept while the process has windows.
    fn exe_cache() -> &'static Mutex<HashMap<u32, Option<String>>> {
        static MAP: OnceLock<Mutex<HashMap<u32, Option<String>>>> = OnceLock::new();
        MAP.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Icon data URLs by (window, icon handle) — an app that changes its
    /// icon gets a new entry.
    fn icon_cache() -> &'static Mutex<HashMap<(isize, usize), Option<String>>> {
        static MAP: OnceLock<Mutex<HashMap<(isize, usize), Option<String>>>> = OnceLock::new();
        MAP.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn pid_of(h: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        pid
    }

    fn title_of(h: HWND) -> String {
        unsafe {
            let len = GetWindowTextLengthW(h).max(0) as usize;
            if len == 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len + 1];
            let n = GetWindowTextW(h, &mut buf).max(0) as usize;
            String::from_utf16_lossy(&buf[..n])
        }
    }

    fn class_of(h: HWND) -> String {
        let mut buf = [0u16; 128];
        let n = unsafe { GetClassNameW(h, &mut buf) }.max(0) as usize;
        String::from_utf16_lossy(&buf[..n])
    }

    fn cloaked(h: HWND) -> bool {
        let mut value = 0u32;
        unsafe {
            DwmGetWindowAttribute(
                h,
                DWMWA_CLOAKED,
                &mut value as *mut u32 as *mut _,
                std::mem::size_of::<u32>() as u32,
            )
        }
        .is_ok()
            && value != 0
    }

    fn candidate(h: HWND) -> Candidate {
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            Candidate {
                visible: IsWindowVisible(h).as_bool(),
                cloaked: cloaked(h),
                tool_window: ex & WS_EX_TOOLWINDOW.0 != 0,
                app_window: ex & WS_EX_APPWINDOW.0 != 0,
                no_activate: ex & WS_EX_NOACTIVATE.0 != 0,
                owned: GetWindow(h, GW_OWNER).is_ok(),
                has_title: GetWindowTextLengthW(h) > 0,
                own_process: pid_of(h) == std::process::id(),
                shell: is_shell_class(&class_of(h)),
            }
        }
    }

    fn exe_path(pid: u32) -> Option<String> {
        if let Some(hit) = exe_cache().lock().unwrap().get(&pid) {
            return hit.clone();
        }
        let path = process_path(pid);
        exe_cache().lock().unwrap().insert(pid, path.clone());
        path
    }

    pub fn process_path(pid: u32) -> Option<String> {
        unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .ok()
                .and_then(|handle| {
                    let mut buf = [0u16; 1024];
                    let mut size = buf.len() as u32;
                    let ok = QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        PWSTR(buf.as_mut_ptr()),
                        &mut size,
                    )
                    .is_ok();
                    let _ = CloseHandle(handle);
                    ok.then(|| String::from_utf16_lossy(&buf[..size as usize]))
                })
        }
    }

    fn monitor_of(h: HWND) -> Option<u32> {
        unsafe {
            let mon = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if !GetMonitorInfoW(mon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
                return None;
            }
            let n = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
            let name = String::from_utf16_lossy(&info.szDevice[..n]);
            Some(crate::monitors::display_number(&name, 1))
        }
    }

    fn rect_of(h: HWND, minimized: bool) -> Rect {
        unsafe {
            let r = if minimized {
                let mut wp = WINDOWPLACEMENT {
                    length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                let _ = GetWindowPlacement(h, &mut wp);
                wp.rcNormalPosition
            } else {
                let mut r = RECT::default();
                let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h, &mut r);
                r
            };
            Rect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top }
        }
    }

    fn info(h: HWND, focused: isize, icons: bool) -> WindowInfo {
        let pid = pid_of(h);
        let exe = exe_path(pid);
        let app = exe.as_deref().and_then(|p| {
            std::path::Path::new(p)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        });
        let minimized = unsafe { IsIconic(h) }.as_bool();
        WindowInfo {
            id: id_of(h),
            title: title_of(h),
            app,
            exe: exe.clone(),
            pid,
            monitor: monitor_of(h),
            minimized,
            maximized: unsafe { IsZoomed(h) }.as_bool(),
            focused: h.0 as isize == focused,
            rect: rect_of(h, minimized),
            attention: false,
            icon: if icons { icon_for(h, exe.as_deref()) } else { None },
        }
    }

    unsafe extern "system" fn collect(h: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<HWND>);
        // Most top-level windows are hidden: skip them before the costlier checks.
        if IsWindowVisible(h).as_bool() && listed(&candidate(h)) {
            out.push(h);
        }
        BOOL(1)
    }

    fn listed_handles() -> AppResult<Vec<HWND>> {
        let mut out: Vec<HWND> = Vec::new();
        unsafe { EnumWindows(Some(collect), LPARAM(&mut out as *mut Vec<HWND> as isize)) }
            .map_err(|e| AppError::msg(format!("windows: {e}")))?;
        Ok(out)
    }

    /// The focused app window (see `last_focus`).
    fn focused(listed: &[HWND]) -> isize {
        let fg = unsafe { GetForegroundWindow() };
        let mut last = last_focus().lock().unwrap();
        if listed.iter().any(|h| *h == fg) {
            *last = fg.0 as isize;
        } else if fg.is_invalid() || pid_of(fg) != std::process::id() {
            // Focus went somewhere unlisted (the shell, a popup): nothing listed has it.
            *last = 0;
        }
        if listed.iter().any(|h| h.0 as isize == *last) {
            *last
        } else {
            0
        }
    }

    pub fn list(icons: bool) -> AppResult<Vec<WindowInfo>> {
        let handles = listed_handles()?;
        let focus = focused(&handles);
        let front = unsafe { GetForegroundWindow() };
        let mut list: Vec<WindowInfo> = handles.iter().map(|h| info(*h, focus, icons)).collect();
        // Attention lasts until the window is really in front (not just the
        // last app focused while a Versailles bar is clicked).
        for w in &mut list {
            if w.id == id_of(front) {
                crate::attention::clear(w.id);
            }
            w.attention = crate::attention::flagged(w.id);
        }
        // Forget processes and icons of windows that are gone.
        {
            let mut exes = exe_cache().lock().unwrap();
            exes.retain(|pid, _| list.iter().any(|w| w.pid == *pid));
        }
        {
            let mut cache = icon_cache().lock().unwrap();
            cache.retain(|(h, _), _| handles.iter().any(|x| x.0 as isize == *h));
        }
        Ok(list)
    }

    /// A listed window by id, or an error (unknown, gone, or not an app
    /// window — Versailles' own windows are never listed).
    fn listed_window(id: i64) -> AppResult<HWND> {
        let h = hwnd(id);
        if !unsafe { IsWindow(Some(h)) }.as_bool() {
            return Err(AppError::msg(format!("windows: no window {id}")));
        }
        if pid_of(h) == std::process::id() {
            return Err(AppError::msg("windows: Versailles' own windows cannot be controlled"));
        }
        if !listed(&candidate(h)) {
            return Err(AppError::msg(format!("windows: {id} is not an app window")));
        }
        Ok(h)
    }

    pub fn check_listed(id: i64) -> AppResult<()> {
        listed_window(id).map(|_| ())
    }

    pub fn icon_handle(id: i64) -> Option<isize> {
        let h = listed_window(id).ok()?;
        window_icon(h).map(|i| i.0 as isize)
    }

    pub fn icon(id: i64) -> AppResult<Option<String>> {
        let h = listed_window(id)?;
        let exe = exe_path(pid_of(h));
        Ok(icon_for(h, exe.as_deref()))
    }

    /// The window's icon: what it reports (`WM_GETICON`), its class icon, or
    /// its executable's first icon.
    fn window_icon(h: HWND) -> Option<HICON> {
        unsafe {
            for kind in [ICON_BIG, ICON_SMALL2, 0] {
                let mut result = 0usize;
                let ok = SendMessageTimeoutW(
                    h,
                    WM_GETICON,
                    WPARAM(kind as usize),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    50,
                    Some(&mut result as *mut usize),
                );
                if ok.0 != 0 && result != 0 {
                    return Some(HICON(result as *mut _));
                }
            }
            for index in [GCLP_HICON, GCLP_HICONSM] {
                let v = GetClassLongPtrW(h, index);
                if v != 0 {
                    return Some(HICON(v as *mut _));
                }
            }
        }
        None
    }

    fn icon_for(h: HWND, exe: Option<&str>) -> Option<String> {
        let owned = window_icon(h);
        let key = (h.0 as isize, owned.map(|i| i.0 as usize).unwrap_or(0));
        if let Some(hit) = icon_cache().lock().unwrap().get(&key) {
            return hit.clone();
        }
        let url = match owned {
            Some(icon) => icon_png(icon),
            None => exe.and_then(exe_icon_png),
        }
        .map(|png| data_url(&png));
        icon_cache().lock().unwrap().insert(key, url.clone());
        url
    }

    /// An executable's first large icon as a PNG.
    fn exe_icon_png(path: &str) -> Option<Vec<u8>> {
        unsafe {
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let mut large = HICON::default();
            if ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut large as *mut HICON), None, 1) == 0 || large.is_invalid() {
                return None;
            }
            let png = icon_png(large);
            let _ = DestroyIcon(large);
            png
        }
    }

    pub fn exe_icon_url(path: &str) -> Option<String> {
        exe_icon_png(path).map(|png| data_url(&png))
    }

    fn data_url(png: &[u8]) -> String {
        format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
    }

    pub fn icon_handle_url(icon: isize) -> Option<String> {
        let icon = HICON(icon as *mut _);
        icon_png_sized(icon, native_size(icon)).map(|png| data_url(&png))
    }

    /// Width of the icon's own bitmap, kept to 16–64 px (32 when unknown).
    fn native_size(icon: HICON) -> i32 {
        unsafe {
            let mut info = ICONINFO::default();
            if GetIconInfo(icon, &mut info).is_err() {
                return ICON_PX;
            }
            let mut bm = BITMAP::default();
            let source = if info.hbmColor.is_invalid() { info.hbmMask } else { info.hbmColor };
            let ok = GetObjectW(
                HGDIOBJ(source.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut BITMAP as *mut _),
            ) != 0;
            for b in [info.hbmColor, info.hbmMask] {
                if !b.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(b.0));
                }
            }
            if ok && bm.bmWidth > 0 {
                bm.bmWidth.clamp(16, 64)
            } else {
                ICON_PX
            }
        }
    }

    /// Draw `icon` at `px`×`px` into a DIB and read it back.
    fn draw(icon: HICON, flags: windows::Win32::UI::WindowsAndMessaging::DI_FLAGS, fill: u8, px: i32) -> Option<Vec<u8>> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return None;
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: px,
                    biHeight: -px, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return None;
            };
            let len = (px * px * 4) as usize;
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            std::ptr::write_bytes(bits as *mut u8, fill, len);
            let ok = DrawIconEx(dc, 0, 0, icon, px, px, 0, None, flags).is_ok();
            let pixels = ok.then(|| std::slice::from_raw_parts(bits as *const u8, len).to_vec());
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            pixels
        }
    }

    fn icon_png(icon: HICON) -> Option<Vec<u8>> {
        icon_png_sized(icon, ICON_PX)
    }

    fn icon_png_sized(icon: HICON, px: i32) -> Option<Vec<u8>> {
        let color = draw(icon, DI_NORMAL, 0, px)?;
        // An icon without alpha: its mask (black = opaque) gives the shape.
        let opaque: Option<Vec<bool>> = if color.chunks(4).all(|p| p[3] == 0) {
            draw(icon, DI_MASK, 0xFF, px).map(|mask| mask.chunks(4).map(|p| p[0] == 0).collect())
        } else {
            None
        };
        let rgba = bgra_to_rgba(&color, opaque.as_deref());
        Some(png_rgba(px as u32, px as u32, &rgba))
    }

    /// Bring `h` to the front. Windows only lets the foreground process do
    /// that; a click on a Versailles bar makes us foreground, and otherwise
    /// the input of the current foreground thread is borrowed briefly.
    fn focus(h: HWND) {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            if SetForegroundWindow(h).as_bool() {
                return;
            }
            let fg = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg, None);
            let me = GetCurrentThreadId();
            let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
            let _ = BringWindowToTop(h);
            let _ = SetForegroundWindow(h);
            if attached {
                let _ = AttachThreadInput(me, fg_thread, false);
            }
        }
    }

    pub fn control(id: i64, action: Action, to: Option<(Option<(i32, i32)>, Option<(i32, i32)>)>) -> AppResult<()> {
        let h = listed_window(id)?;
        unsafe {
            match action {
                Action::Focus => focus(h),
                Action::Minimize => {
                    let _ = ShowWindow(h, SW_MINIMIZE);
                }
                Action::Restore => {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                Action::Maximize => {
                    let _ = ShowWindow(h, SW_MAXIMIZE);
                }
                Action::Close => {
                    // Asks politely: the app may still prompt to save.
                    PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0))
                        .map_err(|e| AppError::msg(format!("windows: close failed: {e}")))?;
                }
                Action::Move => {
                    let (pos, size) = to.unwrap_or((None, None));
                    if IsIconic(h).as_bool() || IsZoomed(h).as_bool() {
                        let _ = ShowWindow(h, SW_RESTORE);
                    }
                    let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
                    if pos.is_none() {
                        flags |= SWP_NOMOVE;
                    }
                    if size.is_none() {
                        flags |= SWP_NOSIZE;
                    }
                    let (x, y) = pos.unwrap_or((0, 0));
                    let (w, ht) = size.unwrap_or((0, 0));
                    SetWindowPos(h, None, x, y, w, ht, flags)
                        .map_err(|e| AppError::msg(format!("windows: move failed: {e}")))?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    fn unavailable<T>() -> AppResult<T> {
        Err(AppError::msg("the windows hook is only available on Windows"))
    }
    pub fn list(_icons: bool) -> AppResult<Vec<WindowInfo>> {
        unavailable()
    }
    pub fn icon(_id: i64) -> AppResult<Option<String>> {
        unavailable()
    }
    pub fn check_listed(_id: i64) -> AppResult<()> {
        unavailable()
    }
    pub fn control(
        _id: i64,
        _action: Action,
        _to: Option<(Option<(i32, i32)>, Option<(i32, i32)>)>,
    ) -> AppResult<()> {
        unavailable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: i64, title: &str) -> WindowInfo {
        WindowInfo {
            id,
            title: title.into(),
            app: Some("app".into()),
            exe: None,
            pid: 7,
            monitor: Some(1),
            minimized: false,
            maximized: false,
            focused: false,
            rect: Rect { x: 0, y: 0, width: 800, height: 600 },
            attention: false,
            icon: None,
        }
    }

    fn kinds(changes: &[Change]) -> Vec<(ChangeKind, i64)> {
        changes.iter().map(|c| (c.kind, c.window.id)).collect()
    }

    #[test]
    fn diffs_open_close_focus_and_changes() {
        let a = win(1, "a");
        let b = win(2, "b");
        assert!(diff(&[a.clone(), b.clone()], &[a.clone(), b.clone()]).is_empty());
        // Moving a window alone is not a change.
        let moved = WindowInfo { rect: Rect { x: 50, ..a.rect }, ..a.clone() };
        assert!(diff(&[a.clone()], &[moved]).is_empty());

        let c = win(3, "c");
        assert_eq!(
            kinds(&diff(&[a.clone(), b.clone()], &[b.clone(), c.clone()])),
            vec![(ChangeKind::Closed, 1), (ChangeKind::Opened, 3)]
        );

        // Focus moves from a to b: a changed (lost it), then b focused last.
        let a_focus = WindowInfo { focused: true, ..a.clone() };
        let b_focus = WindowInfo { focused: true, ..b.clone() };
        assert_eq!(
            kinds(&diff(&[a_focus, b.clone()], &[a.clone(), b_focus])),
            vec![(ChangeKind::Changed, 1), (ChangeKind::Focused, 2)]
        );

        // A flash, then the window answered.
        let flashing = WindowInfo { attention: true, ..a.clone() };
        assert_eq!(kinds(&diff(&[a.clone()], &[flashing.clone()])), vec![(ChangeKind::Changed, 1)]);
        assert_eq!(kinds(&diff(&[flashing.clone()], &[a.clone()])), vec![(ChangeKind::Changed, 1)]);
        let answered = WindowInfo { focused: true, ..a.clone() };
        assert_eq!(kinds(&diff(&[flashing], &[answered])), vec![(ChangeKind::Focused, 1)]);

        let renamed = WindowInfo { title: "a — saved".into(), ..a.clone() };
        let min = WindowInfo { minimized: true, ..b.clone() };
        let other_monitor = WindowInfo { monitor: Some(2), ..c.clone() };
        assert_eq!(
            kinds(&diff(&[a, b, c], &[renamed, min, other_monitor])),
            vec![(ChangeKind::Changed, 1), (ChangeKind::Changed, 2), (ChangeKind::Changed, 3)]
        );
    }

    #[test]
    fn change_payload_shape() {
        let v = change_json(&Change { kind: ChangeKind::Opened, window: win(42, "Notes") });
        assert_eq!(v["type"], "opened");
        assert_eq!(v["id"], 42);
        assert_eq!(v["window"]["title"], "Notes");
        assert_eq!(v["window"]["rect"]["width"], 800);
        assert!(v["window"].get("icon").is_none());
        assert_eq!(v["window"]["attention"], false);
    }

    #[test]
    fn lists_what_a_taskbar_would() {
        let app = Candidate { visible: true, has_title: true, ..Default::default() };
        assert!(listed(&app));
        assert!(!listed(&Candidate { visible: false, ..app }));
        assert!(!listed(&Candidate { cloaked: true, ..app }));
        assert!(!listed(&Candidate { has_title: false, ..app }));
        assert!(!listed(&Candidate { tool_window: true, ..app }));
        assert!(!listed(&Candidate { owned: true, ..app }));
        assert!(!listed(&Candidate { no_activate: true, ..app }));
        // WS_EX_APPWINDOW forces a button even for an owned window...
        assert!(listed(&Candidate { owned: true, app_window: true, ..app }));
        // ...but never for ours, the shell, or a cloaked one.
        assert!(!listed(&Candidate { own_process: true, app_window: true, ..app }));
        assert!(!listed(&Candidate { shell: true, ..app }));
        assert!(!listed(&Candidate { cloaked: true, app_window: true, ..app }));
        assert!(is_shell_class("Progman"));
        assert!(is_shell_class("Shell_TrayWnd"));
        assert!(!is_shell_class("Chrome_WidgetWin_1"));
    }

    #[test]
    fn move_targets() {
        let both = MoveTo { x: Some(10), y: Some(20), width: Some(800), height: Some(600) };
        assert_eq!(both.parts().unwrap(), (Some((10, 20)), Some((800, 600))));
        let pos = MoveTo { x: Some(-1920), y: Some(0), ..Default::default() };
        assert_eq!(pos.parts().unwrap(), (Some((-1920, 0)), None));
        assert!(MoveTo { x: Some(1), ..Default::default() }.parts().is_err());
        assert!(MoveTo { width: Some(0), height: Some(10), ..Default::default() }.parts().is_err());
        assert!(MoveTo::default().parts().is_err());
        assert!(control(1, Action::Move, None).is_err());
        let a: Action = serde_json::from_str(r#""minimize""#).unwrap();
        assert_eq!(a, Action::Minimize);
        assert!(serde_json::from_str::<Action>(r#""destroy""#).is_err());
    }

    #[test]
    fn encodes_a_valid_png() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        let px = [255u8, 0, 0, 255, 0, 255, 0, 128];
        let png = png_rgba(2, 1, &px);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &2u32.to_be_bytes());
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        assert_eq!(&png[png.len() - 4..], &0xAE42_6082u32.to_be_bytes());
        // More than one stored block.
        let big = vec![7u8; 200 * 200 * 4];
        assert!(png_rgba(200, 200, &big).len() > big.len());
    }

    /// Reads the real desktop: `cargo test --lib live_window_list -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_window_list() {
        let list = list(true).expect("list");
        for w in &list {
            eprintln!(
                "{:>10} {:<24} {:<40} mon={:?} min={} max={} focus={} icon={}",
                w.id,
                w.app.as_deref().unwrap_or("?"),
                w.title.chars().take(40).collect::<String>(),
                w.monitor,
                w.minimized,
                w.maximized,
                w.focused,
                w.icon.as_ref().map(|i| i.len()).unwrap_or(0)
            );
        }
        assert!(list.iter().all(|w| w.pid != std::process::id()));
        if let Some(w) = list.first() {
            assert_eq!(icon(w.id).unwrap(), w.icon);
        }
    }

    #[test]
    fn unpremultiplies_and_uses_the_mask() {
        // Half-transparent red, premultiplied by GDI: (0,0,128,128) BGRA.
        let rgba = bgra_to_rgba(&[0, 0, 128, 128, 0, 0, 0, 0], None);
        assert_eq!(&rgba[..4], &[255, 0, 0, 128]);
        assert_eq!(&rgba[4..], &[0, 0, 0, 0]);
        // No alpha anywhere: the mask decides.
        let rgba = bgra_to_rgba(&[10, 20, 30, 0, 10, 20, 30, 0], Some(&[true, false]));
        assert_eq!(rgba, vec![30, 20, 10, 255, 30, 20, 10, 0]);
    }
}
