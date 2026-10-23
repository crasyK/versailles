//! Notification-area icons (#35): the `tray` hook lists the icons apps put
//! in the system tray, pushes their changes, and clicks one for a bar.
//!
//! The ManagedShell approach (Cairo, RetroBar), in process, nothing
//! injected: while Versailles hides the Windows taskbar and some piece
//! declares `tray`, a topmost window of class `Shell_TrayWnd` of our own sits
//! in front of Explorer's. `Shell_NotifyIcon` looks the taskbar up by class,
//! so apps' icon calls arrive here as `WM_COPYDATA`; a `TaskbarCreated`
//! broadcast makes running apps add theirs again. Every message is also
//! forwarded to Explorer's taskbar, so it keeps the same icons (and shows
//! balloons as toasts): when we stop or crash, nothing is lost. Docked bars'
//! AppBar messages pass straight through.
//!
//! Explorer draws Windows' own volume / network / battery icons itself; two
//! of them still come through `Shell_NotifyIcon` and are marked `system`.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIM_SETFOCUS: u32 = 3;
pub const NIM_SETVERSION: u32 = 4;

const NIF_MESSAGE: u32 = 0x01;
const NIF_ICON: u32 = 0x02;
const NIF_TIP: u32 = 0x04;
const NIF_STATE: u32 = 0x08;
const NIF_INFO: u32 = 0x10;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x01;

/// First field of every `Shell_NotifyIcon` copy (`SHELLTRAYDATA`).
const TRAY_MAGIC: u32 = 0x3475_3423;

const WM_CONTEXTMENU: u32 = 0x007B;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
/// `NIN_SELECT` (`WM_USER`): a version 3+ icon was picked with the mouse.
const NIN_SELECT: u32 = 0x0400;

/// One `Shell_NotifyIcon` call as shell32 hands it to the taskbar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notify {
    pub message: u32,
    pub hwnd: i64,
    pub uid: u32,
    pub flags: u32,
    pub callback: u32,
    pub hicon: i64,
    pub tip: String,
    pub state: u32,
    pub state_mask: u32,
    /// `uVersion` for `NIM_SETVERSION`; the same field is the balloon
    /// timeout when `NIF_INFO` is set.
    pub version: u32,
    pub guid: Option<String>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// A 32-bit handle widened like `LongToHandle` (sign-extended).
fn handle_at(b: &[u8], at: usize) -> Option<i64> {
    u32_at(b, at).map(|v| v as i32 as i64)
}

/// A NUL-terminated UTF-16 field of `chars` characters (as much as is there).
fn wstr_at(b: &[u8], at: usize, chars: usize) -> String {
    let end = (at + chars * 2).min(b.len());
    let units: Vec<u16> = b
        .get(at..end)
        .unwrap_or_default()
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn guid_at(b: &[u8], at: usize) -> Option<String> {
    let bytes: [u8; 16] = b.get(at..at + 16)?.try_into().ok()?;
    (bytes != [0; 16]).then(|| uuid::Uuid::from_bytes_le(bytes).to_string())
}

/// `SHELLTRAYDATA`: magic, message, then a `NOTIFYICONDATAW` whose handles
/// are 32 bits wide (so the layout is the same for 32- and 64-bit apps).
pub fn parse(data: &[u8]) -> Option<Notify> {
    if u32_at(data, 0)? != TRAY_MAGIC {
        return None;
    }
    let message = u32_at(data, 4)?;
    let n = data.get(8..)?;
    let flags = u32_at(n, 12)?;
    Some(Notify {
        message,
        hwnd: handle_at(n, 4)?,
        uid: u32_at(n, 8)?,
        flags,
        callback: u32_at(n, 16)?,
        hicon: handle_at(n, 20)?,
        tip: wstr_at(n, 24, 128),
        state: u32_at(n, 280).unwrap_or(0),
        state_mask: u32_at(n, 284).unwrap_or(0),
        version: u32_at(n, 800).unwrap_or(0),
        guid: if flags & NIF_GUID != 0 { guid_at(n, 936) } else { None },
    })
}

/// `Shell_NotifyIconGetRect`: which icon, and which corner (1 top-left,
/// 2 bottom-right) the app asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct RectQuery {
    pub corner: u32,
    pub hwnd: i64,
    pub uid: u32,
    pub guid: Option<String>,
}

/// `WINNOTIFYICONIDENTIFIER`: magic, message, size, padding, then the icon.
pub fn parse_rect_query(data: &[u8]) -> Option<RectQuery> {
    Some(RectQuery {
        corner: u32_at(data, 4)?,
        hwnd: handle_at(data, 16)?,
        uid: u32_at(data, 20)?,
        guid: guid_at(data, 24),
    })
}

/// One tray icon as pages see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayIcon {
    /// Versailles' number for the icon (for this run); pass it to `click`.
    pub id: u32,
    /// The tooltip (may hold several lines).
    pub tip: String,
    /// Executable name without extension of the app that owns it.
    pub app: Option<String>,
    pub exe: Option<String>,
    pub pid: u32,
    /// The app's own id for the icon, when it gives one.
    pub guid: Option<String>,
    /// One of Windows' own icons, owned by Explorer (volume, battery).
    pub system: bool,
    /// Whether the app takes clicks (it gave a callback message).
    pub clickable: bool,
    /// `data:image/png;base64,…` at the icon's own size (16–64 px).
    pub icon: Option<String>,
    #[serde(skip)]
    hwnd: i64,
    #[serde(skip)]
    uid: u32,
    #[serde(skip)]
    callback: u32,
    #[serde(skip)]
    version: u32,
    /// `NIS_HIDDEN`: kept, but not listed.
    #[serde(skip)]
    hidden: bool,
    /// Where it was last clicked (`Shell_NotifyIconGetRect` answers it).
    #[serde(skip)]
    point: Option<(i32, i32)>,
}

impl TrayIcon {
    /// Whether anything a page sees differs.
    fn same_view(&self, other: &TrayIcon) -> bool {
        self.tip == other.tip
            && self.icon == other.icon
            && self.guid == other.guid
            && self.clickable == other.clickable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    /// New, or shown again after being hidden.
    Added,
    /// Tooltip, image or clickability.
    Changed,
    /// Deleted, hidden, or its app is gone.
    Removed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub kind: ChangeKind,
    pub icon: TrayIcon,
}

/// Push payload for one change: `{ type, id, icon }`.
pub fn change_json(change: &Change) -> Value {
    json!({ "type": change.kind, "id": change.icon.id, "icon": change.icon })
}

/// What the table needs from the system for a new icon or image.
pub trait Lookup {
    /// Process id and executable path of the window that owns an icon.
    fn owner(&mut self, hwnd: i64) -> (u32, Option<String>);
    /// An icon handle as a PNG data URL.
    fn image(&mut self, hicon: i64) -> Option<String>;
}

/// The icons apps have added, in the order they came.
#[derive(Debug, Default)]
pub struct Table {
    icons: Vec<TrayIcon>,
    next_id: u32,
}

/// What a click needs to reach an icon's app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub hwnd: i64,
    pub uid: u32,
    pub callback: u32,
    pub version: u32,
    pub pid: u32,
}

impl Table {
    /// An app names its icon by GUID, or by window and number.
    fn find(&self, hwnd: i64, uid: u32, guid: Option<&str>) -> Option<usize> {
        self.icons.iter().position(|i| {
            (guid.is_some() && i.guid.as_deref() == guid) || (hwnd != 0 && i.hwnd == hwnd && i.uid == uid)
        })
    }

    /// Apply one `Shell_NotifyIcon` call: what to answer the app, and the
    /// changes a page sees.
    pub fn apply(&mut self, n: &Notify, look: &mut impl Lookup) -> (bool, Vec<Change>) {
        let at = self.find(n.hwnd, n.uid, n.guid.as_deref());
        match n.message {
            NIM_ADD | NIM_MODIFY => self.add_or_modify(at, n, look),
            NIM_DELETE => match at {
                Some(i) => {
                    let icon = self.icons.remove(i);
                    let changes = if icon.hidden {
                        Vec::new()
                    } else {
                        vec![Change { kind: ChangeKind::Removed, icon }]
                    };
                    (true, changes)
                }
                None => (false, Vec::new()),
            },
            NIM_SETVERSION => {
                if n.version > 4 {
                    return (false, Vec::new());
                }
                if let Some(i) = at {
                    self.icons[i].version = n.version;
                }
                (true, Vec::new())
            }
            NIM_SETFOCUS => (at.is_some(), Vec::new()),
            _ => (false, Vec::new()),
        }
    }

    fn add_or_modify(&mut self, at: Option<usize>, n: &Notify, look: &mut impl Lookup) -> (bool, Vec<Change>) {
        // A new icon needs its window: that is where clicks go.
        if at.is_none() && n.hwnd == 0 {
            return (false, Vec::new());
        }
        let before = at.map(|i| self.icons[i].clone());
        let i = match at {
            Some(i) => i,
            None => {
                self.next_id += 1;
                let (pid, exe) = look.owner(n.hwnd);
                let file = exe.as_deref().map(std::path::Path::new);
                self.icons.push(TrayIcon {
                    id: self.next_id,
                    tip: String::new(),
                    app: file.and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()),
                    system: file
                        .and_then(|p| p.file_name())
                        .is_some_and(|f| f.eq_ignore_ascii_case("explorer.exe")),
                    exe,
                    pid,
                    guid: None,
                    clickable: false,
                    icon: None,
                    hwnd: n.hwnd,
                    uid: n.uid,
                    callback: 0,
                    version: 0,
                    hidden: false,
                    point: None,
                });
                self.icons.len() - 1
            }
        };
        let icon = &mut self.icons[i];
        if n.hwnd != 0 {
            icon.hwnd = n.hwnd;
            icon.uid = n.uid;
        }
        if n.flags & NIF_MESSAGE != 0 {
            icon.callback = n.callback;
            icon.clickable = n.callback != 0;
        }
        if n.flags & NIF_ICON != 0 {
            // An image that cannot be read keeps the one before.
            icon.icon = if n.hicon == 0 { None } else { look.image(n.hicon).or(icon.icon.take()) };
        }
        // Some apps add again with an empty tooltip after setting one
        // (Windows Security, Cursor): keep the text.
        if n.flags & NIF_TIP != 0 && !n.tip.is_empty() {
            icon.tip = n.tip.clone();
        }
        if n.flags & NIF_STATE != 0 && n.state_mask & NIS_HIDDEN != 0 {
            icon.hidden = n.state & NIS_HIDDEN != 0;
        }
        if n.flags & NIF_GUID != 0 {
            icon.guid = n.guid.clone();
        }
        if n.flags & NIF_INFO == 0 && (1..=4).contains(&n.version) {
            icon.version = n.version;
        }
        let now = icon.clone();
        let kind = match (before.filter(|b| !b.hidden), now.hidden) {
            (None, false) => Some(ChangeKind::Added),
            (Some(_), true) => Some(ChangeKind::Removed),
            (Some(b), false) if !b.same_view(&now) => Some(ChangeKind::Changed),
            _ => None,
        };
        let changes = kind.map(|kind| vec![Change { kind, icon: now }]).unwrap_or_default();
        // A modify for an icon we did not have is kept but answered as failed,
        // so the app adds it again in full (its click message included).
        (at.is_some() || n.message == NIM_ADD, changes)
    }

    /// Drop icons whose window is gone (the app crashed or quit without
    /// deleting them).
    pub fn sweep(&mut self, alive: impl Fn(i64) -> bool) -> Vec<Change> {
        let mut out = Vec::new();
        self.icons.retain(|icon| {
            if alive(icon.hwnd) {
                return true;
            }
            if !icon.hidden {
                out.push(Change { kind: ChangeKind::Removed, icon: icon.clone() });
            }
            false
        });
        out
    }

    /// The icons a page sees (not hidden), oldest first.
    pub fn list(&self) -> Vec<TrayIcon> {
        self.icons.iter().filter(|i| !i.hidden).cloned().collect()
    }

    pub fn clear(&mut self) {
        self.icons.clear();
    }

    /// The icon to click, remembering where (for `Shell_NotifyIconGetRect`).
    pub fn target(&mut self, id: u32, point: (i32, i32)) -> AppResult<Target> {
        let icon = self
            .icons
            .iter_mut()
            .find(|i| i.id == id && !i.hidden)
            .ok_or_else(|| AppError::msg(format!("tray: no icon {id}")))?;
        if icon.callback == 0 {
            return Err(AppError::msg(format!("tray: icon {id} takes no clicks")));
        }
        icon.point = Some(point);
        Ok(Target {
            hwnd: icon.hwnd,
            uid: icon.uid,
            callback: icon.callback,
            version: icon.version,
            pid: icon.pid,
        })
    }

    /// Where an icon was last clicked, for an app placing its pop-up.
    pub fn point_of(&self, q: &RectQuery) -> Option<Option<(i32, i32)>> {
        self.find(q.hwnd, q.uid, q.guid.as_deref()).map(|i| self.icons[i].point)
    }
}

/// Mouse buttons a page can send. They map to browser events one to one:
/// `click` → `left`, `contextmenu` → `right`, `dblclick` → `double` (the
/// second half of a double click: the browser already sent the first click).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Left,
    Right,
    Double,
    Middle,
}

/// The callback messages Explorer sends for a click, in order. Version 3+
/// icons also get `NIN_SELECT` / `WM_CONTEXTMENU` (documented for version 4;
/// Explorer sends them to 3 as well).
pub fn click_messages(button: Button, version: u32) -> Vec<u32> {
    let v3 = version >= 3;
    let mut out = match button {
        Button::Left => vec![WM_LBUTTONDOWN, WM_LBUTTONUP],
        Button::Right => vec![WM_RBUTTONDOWN, WM_RBUTTONUP],
        Button::Double => vec![WM_LBUTTONDBLCLK, WM_LBUTTONUP],
        Button::Middle => vec![WM_MBUTTONDOWN, WM_MBUTTONUP],
    };
    match button {
        Button::Left if v3 => out.push(NIN_SELECT),
        Button::Right if v3 => out.push(WM_CONTEXTMENU),
        _ => {}
    }
    out
}

/// `wParam` / `lParam` of a callback message. Version 4: the point in
/// `wParam`, message and icon number in `lParam`; older: number, message.
pub fn callback_params(version: u32, uid: u32, message: u32, x: i32, y: i32) -> (usize, isize) {
    if version >= 4 {
        let point = (x as u16 as u32) | ((y as u16 as u32) << 16);
        let lparam = (message & 0xFFFF) | ((uid & 0xFFFF) << 16);
        (point as usize, lparam as isize)
    } else {
        (uid as usize, message as isize)
    }
}

/// Coalesce one push into a pending batch: a `changed` for an icon that
/// already waits (added or changed) only updates that entry.
pub fn merge(batch: &mut Vec<Value>, push: Value) {
    if push["type"] == "changed" {
        if let Some(prev) = batch
            .iter_mut()
            .find(|p| p["id"] == push["id"] && (p["type"] == "added" || p["type"] == "changed"))
        {
            prev["icon"] = push["icon"].clone();
            return;
        }
    }
    batch.push(push);
}

fn table() -> &'static Mutex<Table> {
    static TABLE: OnceLock<Mutex<Table>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(Table::default()))
}

/// `tray_list`: `{ active, icons }`. `active` is false while Explorer's own
/// taskbar holds the icons (it is not hidden, or no piece declares `tray`).
pub fn state() -> Value {
    json!({ "active": imp::active(), "icons": table().lock().unwrap().list() })
}

/// Click icon `id` at `at` (physical screen pixels; the cursor by default).
pub fn click(id: u32, button: Button, at: Option<(i32, i32)>) -> AppResult<()> {
    imp::click(id, button, at)
}

/// Whether some piece on the page declares `tray`.
fn declared(app: &AppHandle) -> bool {
    let state = app.state::<crate::state::AppState>();
    crate::desktop::page_catalog(&state)
        .pieces
        .iter()
        .any(|p| p.hooks.iter().any(|h| h == "tray"))
}

/// Background watcher: run the tray while this Versailles hides the Windows
/// taskbar and some piece declares `tray`; otherwise Explorer keeps it.
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let want = crate::taskbar::hiding() && declared(&app);
        if want && !imp::running() {
            let push_app = app.clone();
            imp::start(Box::new(move |push| crate::push::to_hook(&push_app, "tray", &push)));
        } else if !want && imp::running() {
            imp::stop();
        }
    });
}

/// Stop the tray and hand the icons back to Explorer (before the taskbar
/// comes back, and on exit).
pub fn release() {
    imp::stop();
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Instant;
    use windows::core::w;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
        GetCursorPos, GetWindowLongW, GetWindowThreadProcessId, IsWindow, MsgWaitForMultipleObjects, PeekMessageW,
        PostMessageW, RegisterClassW, RegisterWindowMessageW, SendMessageTimeoutW, SendNotifyMessageW,
        SetWindowLongW, SetWindowPos, TranslateMessage, CS_DBLCLKS, GWL_STYLE, HWND_BROADCAST, HWND_TOPMOST, MSG,
        PM_REMOVE, QS_ALLINPUT, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        WINDOWPOS, WINDOW_EX_STYLE, WM_ACTIVATEAPP, WM_COMMAND, WM_COPYDATA, WM_USER, WM_WINDOWPOSCHANGED,
        WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        WS_VISIBLE,
    };

    /// How often we check that our window is still the one apps find.
    const TICK: Duration = Duration::from_millis(100);
    const SWEEP: Duration = Duration::from_secs(2);
    /// Pushes within this long go out together (animated icons).
    const BATCH: Duration = Duration::from_millis(100);
    /// Explorer's message for the taskbar to reposition itself; it must be
    /// posted, not sent (ManagedShell).
    const WM_TRAY_POST: u32 = WM_USER + 372;

    struct Running {
        stop: Arc<AtomicBool>,
        thread: JoinHandle<()>,
    }

    fn slot() -> &'static Mutex<Option<Running>> {
        static SLOT: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
        SLOT.get_or_init(|| Mutex::new(None))
    }

    fn sink() -> &'static Mutex<Option<Sender<Value>>> {
        static SINK: OnceLock<Mutex<Option<Sender<Value>>>> = OnceLock::new();
        SINK.get_or_init(|| Mutex::new(None))
    }

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    /// Explorer's taskbar, where everything is forwarded.
    static EXPLORER: AtomicIsize = AtomicIsize::new(0);
    static TASKBAR_CREATED: AtomicIsize = AtomicIsize::new(0);

    fn hwnd(h: i64) -> HWND {
        HWND(h as isize as *mut _)
    }

    pub fn active() -> bool {
        ACTIVE.load(Ordering::SeqCst)
    }

    pub fn running() -> bool {
        slot().lock().unwrap().as_ref().is_some_and(|r| !r.thread.is_finished())
    }

    pub fn start(push: Box<dyn Fn(Value) + Send>) {
        let mut slot = slot().lock().unwrap();
        if slot.as_ref().is_some_and(|r| !r.thread.is_finished()) {
            return;
        }
        let (tx, rx) = channel();
        *sink().lock().unwrap() = Some(tx);
        std::thread::spawn(move || pusher(rx, push));
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        match std::thread::Builder::new()
            .name("versailles-tray".into())
            .spawn(move || run(&flag))
        {
            Ok(thread) => *slot = Some(Running { stop, thread }),
            Err(err) => tracing::warn!("tray: thread failed to start: {err}"),
        }
    }

    pub fn stop() {
        let Some(r) = slot().lock().unwrap().take() else {
            return;
        };
        r.stop.store(true, Ordering::SeqCst);
        let _ = r.thread.join();
    }

    fn emit(push: Value) {
        if let Some(tx) = sink().lock().unwrap().as_ref() {
            let _ = tx.send(push);
        }
    }

    /// Deliver pushes off the tray thread, a batch at a time. Ends when the
    /// tray stops (the sender is dropped).
    fn pusher(rx: Receiver<Value>, push: Box<dyn Fn(Value) + Send>) {
        while let Ok(first) = rx.recv() {
            let mut batch = vec![first];
            let until = Instant::now() + BATCH;
            let mut open = true;
            while let Some(left) = until.checked_duration_since(Instant::now()) {
                match rx.recv_timeout(left) {
                    Ok(next) => merge(&mut batch, next),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        open = false;
                        break;
                    }
                }
            }
            for p in batch {
                push(p);
            }
            if !open {
                break;
            }
        }
    }

    struct Win;

    impl Lookup for Win {
        fn owner(&mut self, h: i64) -> (u32, Option<String>) {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd(h), Some(&mut pid)) };
            (pid, crate::app_windows::process_path(pid))
        }
        fn image(&mut self, hicon: i64) -> Option<String> {
            crate::app_windows::icon_handle_url(hicon as isize)
        }
    }

    fn explorer() -> Option<HWND> {
        let cached = hwnd(EXPLORER.load(Ordering::SeqCst) as i64);
        if !cached.is_invalid() && unsafe { IsWindow(Some(cached)) }.as_bool() {
            return Some(cached);
        }
        let found = crate::taskbar::explorer_tray()?;
        EXPLORER.store(found.0 as isize, Ordering::SeqCst);
        Some(found)
    }

    /// Hand a message to Explorer's taskbar and return its answer.
    unsafe fn forward(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        let Some(explorer) = explorer() else {
            return DefWindowProcW(h, msg, w, l);
        };
        if msg == WM_TRAY_POST {
            let _ = PostMessageW(Some(explorer), msg, w, l);
            return DefWindowProcW(h, msg, w, l);
        }
        let mut result = 0usize;
        SendMessageTimeoutW(explorer, msg, w, l, SMTO_ABORTIFHUNG, 4000, Some(&mut result));
        LRESULT(result as isize)
    }

    fn on_notify(data: &[u8]) -> bool {
        let Some(n) = parse(data) else {
            return false;
        };
        let (ok, changes) = table().lock().unwrap().apply(&n, &mut Win);
        for c in &changes {
            match c.kind {
                ChangeKind::Added | ChangeKind::Removed => tracing::info!(
                    "tray: {:?} {} (id {})",
                    c.kind,
                    c.icon.app.as_deref().unwrap_or("?"),
                    c.icon.id
                ),
                ChangeKind::Changed => {}
            }
            emit(change_json(c));
        }
        ok
    }

    fn cursor() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
    }

    fn make_long(x: i32, y: i32) -> isize {
        ((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize
    }

    /// Answer an app placing its pop-up next to its icon: a small square
    /// around where the icon was last clicked (or the cursor).
    fn on_rect_query(data: &[u8]) -> Option<isize> {
        let q = parse_rect_query(data)?;
        let point = table().lock().unwrap().point_of(&q)?;
        let (x, y) = point.or_else(cursor)?;
        const HALF: i32 = 12;
        match q.corner {
            1 => Some(make_long(x - HALF, y - HALF)),
            2 => Some(make_long(x + HALF, y + HALF)),
            _ => None,
        }
    }

    unsafe extern "system" fn tray_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_COPYDATA if l.0 != 0 => {
                let cds = &*(l.0 as *const COPYDATASTRUCT);
                let data: &[u8] = if cds.lpData.is_null() {
                    &[]
                } else {
                    std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize)
                };
                match cds.dwData {
                    // Shell_NotifyIcon: our answer; Explorer gets a copy.
                    1 => {
                        let ours = on_notify(data);
                        forward(h, msg, w, l);
                        return LRESULT(ours as isize);
                    }
                    3 => {
                        if let Some(r) = on_rect_query(data) {
                            return LRESULT(r);
                        }
                    }
                    // 0: AppBar messages (docked bars), answered by Explorer.
                    _ => {}
                }
            }
            WM_WINDOWPOSCHANGED if l.0 != 0 => {
                let pos = &*(l.0 as *const WINDOWPOS);
                if pos.flags.contains(SWP_SHOWWINDOW) {
                    SetWindowLongW(h, GWL_STYLE, GetWindowLongW(h, GWL_STYLE) & !(WS_VISIBLE.0 as i32));
                }
            }
            _ => {}
        }
        // Our own broadcast already reached Explorer.
        if msg as isize == TASKBAR_CREATED.load(Ordering::SeqCst) {
            return DefWindowProcW(h, msg, w, l);
        }
        if msg == WM_COPYDATA || msg == WM_ACTIVATEAPP || msg == WM_COMMAND || msg >= WM_USER {
            return forward(h, msg, w, l);
        }
        DefWindowProcW(h, msg, w, l)
    }

    unsafe extern "system" fn notify_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        DefWindowProcW(h, msg, w, l)
    }

    /// Our `Shell_TrayWnd` with its `TrayNotifyWnd` child (some apps look
    /// for it). Never visible.
    fn create() -> Option<(HWND, Option<HWND>)> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
            // Registering again (a second start) fails harmlessly.
            for (class, proc) in [
                (w!("Shell_TrayWnd"), tray_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT),
                (w!("TrayNotifyWnd"), notify_proc),
            ] {
                RegisterClassW(&WNDCLASSW {
                    style: CS_DBLCLKS,
                    lpfnWndProc: Some(proc),
                    hInstance: instance,
                    lpszClassName: class,
                    ..Default::default()
                });
            }
            let tray = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("Shell_TrayWnd"),
                w!(""),
                WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|err| tracing::warn!("tray: window failed: {err}"))
            .ok()?;
            let child = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("TrayNotifyWnd"),
                w!(""),
                WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                0,
                0,
                Some(tray),
                None,
                Some(instance),
                None,
            )
            .ok();
            Some((tray, child))
        }
    }

    fn raise(tray: HWND) {
        unsafe {
            let _ = SetWindowPos(tray, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    /// Tell every app the taskbar is new: they add their icons again (to
    /// whichever `Shell_TrayWnd` comes first).
    fn broadcast_taskbar_created() {
        unsafe {
            let msg = TASKBAR_CREATED.load(Ordering::SeqCst) as u32;
            if msg != 0 {
                let _ = SendNotifyMessageW(HWND_BROADCAST, msg, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn pump(timeout: Duration) {
        unsafe {
            MsgWaitForMultipleObjects(None, false, timeout.as_millis() as u32, QS_ALLINPUT);
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn sweep() {
        let changes = table()
            .lock()
            .unwrap()
            .sweep(|h| unsafe { IsWindow(Some(hwnd(h))) }.as_bool());
        for c in &changes {
            tracing::info!("tray: {} is gone (id {})", c.icon.app.as_deref().unwrap_or("?"), c.icon.id);
            emit(change_json(c));
        }
    }

    /// The tray thread: own the window until `stop`, then hand back.
    fn run(stop: &AtomicBool) {
        TASKBAR_CREATED.store(unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) } as isize, Ordering::SeqCst);
        let Some((tray, child)) = create() else {
            *sink().lock().unwrap() = None;
            return;
        };
        raise(tray);
        ACTIVE.store(true, Ordering::SeqCst);
        emit(json!({ "type": "active", "active": true }));
        broadcast_taskbar_created();
        tracing::info!("tray: holding the notification icons");

        let mut swept = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            // Explorer's taskbar can come back in front (shown for Start, or
            // restarted): apps must keep finding ours first.
            if unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.ok() != Some(tray) {
                raise(tray);
            }
            if swept.elapsed() >= SWEEP {
                sweep();
                swept = Instant::now();
            }
            pump(TICK);
        }

        unsafe {
            if let Some(child) = child {
                let _ = DestroyWindow(child);
            }
            let _ = DestroyWindow(tray);
        }
        let held = {
            let mut t = table().lock().unwrap();
            let n = t.list().len();
            t.clear();
            n
        };
        ACTIVE.store(false, Ordering::SeqCst);
        emit(json!({ "type": "active", "active": false }));
        *sink().lock().unwrap() = None;
        // Explorer has every icon already (each call was forwarded); this
        // catches any it missed.
        broadcast_taskbar_created();
        tracing::info!("tray: stopped; {held} icons back with Explorer");
    }

    pub fn click(id: u32, button: Button, at: Option<(i32, i32)>) -> AppResult<()> {
        if !active() {
            return Err(AppError::msg(
                "tray: Explorer holds the icons (Versailles lists them only while it hides the taskbar)",
            ));
        }
        let point = at.or_else(cursor).unwrap_or((0, 0));
        let t = table().lock().unwrap().target(id, point)?;
        let owner = hwnd(t.hwnd);
        unsafe {
            if !IsWindow(Some(owner)).as_bool() {
                return Err(AppError::msg(format!("tray: the app of icon {id} is gone")));
            }
            // Lets the app bring its menu or window to the front.
            let _ = AllowSetForegroundWindow(t.pid);
            for m in click_messages(button, t.version) {
                let (wp, lp) = callback_params(t.version, t.uid, m, point.0, point.1);
                SendNotifyMessageW(owner, t.callback, WPARAM(wp), LPARAM(lp))
                    .map_err(|e| AppError::msg(format!("tray: click failed: {e}")))?;
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn active() -> bool {
        false
    }
    pub fn running() -> bool {
        false
    }
    pub fn start(_push: Box<dyn Fn(Value) + Send>) {}
    pub fn stop() {}
    pub fn click(_id: u32, _button: Button, _at: Option<(i32, i32)>) -> AppResult<()> {
        Err(AppError::msg("the tray hook is only available on Windows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;

    impl Lookup for Fake {
        fn owner(&mut self, hwnd: i64) -> (u32, Option<String>) {
            match hwnd {
                0x10 => (8, Some(r"C:\Windows\explorer.exe".into())),
                _ => (7, Some(r"C:\Apps\Tailscale\tailscale-ipn.exe".into())),
            }
        }
        fn image(&mut self, hicon: i64) -> Option<String> {
            (hicon > 0).then(|| format!("data:image/png;base64,{hicon}"))
        }
    }

    /// A `SHELLTRAYDATA` as shell32 sends it (NOTIFYICONDATAW, 956 bytes).
    fn bytes(n: &Notify) -> Vec<u8> {
        let mut b = vec![0u8; 8 + 956];
        let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, 0, TRAY_MAGIC);
        put(&mut b, 4, n.message);
        put(&mut b, 8, 956);
        put(&mut b, 12, n.hwnd as u32);
        put(&mut b, 16, n.uid);
        put(&mut b, 20, n.flags);
        put(&mut b, 24, n.callback);
        put(&mut b, 28, n.hicon as u32);
        for (i, u) in n.tip.encode_utf16().enumerate() {
            b[32 + i * 2..34 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        put(&mut b, 8 + 280, n.state);
        put(&mut b, 8 + 284, n.state_mask);
        put(&mut b, 8 + 800, n.version);
        if let Some(g) = &n.guid {
            let g = uuid::Uuid::parse_str(g).unwrap();
            b[8 + 936..8 + 952].copy_from_slice(&g.to_bytes_le());
        }
        b
    }

    fn note(message: u32, hwnd: i64, uid: u32, flags: u32) -> Notify {
        Notify { message, hwnd, uid, flags, ..Default::default() }
    }

    fn add(hwnd: i64, tip: &str) -> Notify {
        Notify {
            callback: 0x403,
            hicon: 5,
            tip: tip.into(),
            ..note(NIM_ADD, hwnd, 0, NIF_MESSAGE | NIF_ICON | NIF_TIP)
        }
    }

    fn kinds(changes: &[Change]) -> Vec<(ChangeKind, u32)> {
        changes.iter().map(|c| (c.kind, c.icon.id)).collect()
    }

    #[test]
    fn parses_shell_tray_data() {
        let guid = "fbed1924-0f0e-4ae6-87d3-246dcd41eee1";
        let n = Notify {
            hwnd: 0x1067a,
            hicon: -0x5000,
            guid: Some(guid.into()),
            state: 1,
            state_mask: 1,
            version: 4,
            ..add(0x1067a, "Tailscale: Connected.\nClick for options.")
        };
        let n = Notify { flags: n.flags | NIF_GUID | NIF_STATE, ..n };
        assert_eq!(parse(&bytes(&n)), Some(n.clone()));
        // The GUID only counts with NIF_GUID.
        let plain = Notify { flags: NIF_TIP, ..n.clone() };
        assert_eq!(parse(&bytes(&plain)).unwrap().guid, None);
        // Wrong magic, too short.
        let mut bad = bytes(&n);
        bad[0] = 0;
        assert!(parse(&bad).is_none());
        assert!(parse(&bytes(&n)[..20]).is_none());
        // A short copy still gives the fields that are there.
        let short = parse(&bytes(&n)[..8 + 40]).unwrap();
        assert_eq!((short.hwnd, short.tip.as_str(), short.version), (0x1067a, "Tailscal", 0));
    }

    #[test]
    fn parses_rect_queries() {
        let mut b = vec![0u8; 40];
        b[4..8].copy_from_slice(&2u32.to_le_bytes());
        b[16..20].copy_from_slice(&0x20796u32.to_le_bytes());
        b[20..24].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(
            parse_rect_query(&b),
            Some(RectQuery { corner: 2, hwnd: 0x20796, uid: 3, guid: None })
        );
    }

    #[test]
    fn adds_changes_hides_and_deletes() {
        let mut t = Table::default();
        let (ok, ch) = t.apply(&add(0x20, "Tailscale"), &mut Fake);
        assert!(ok);
        assert_eq!(kinds(&ch), vec![(ChangeKind::Added, 1)]);
        let icon = &ch[0].icon;
        assert_eq!(icon.app.as_deref(), Some("tailscale-ipn"));
        assert_eq!((icon.pid, icon.system, icon.clickable), (7, false, true));
        assert_eq!(icon.icon.as_deref(), Some("data:image/png;base64,5"));

        // Same tooltip again: nothing to push. A new one: changed.
        let same = Notify { message: NIM_MODIFY, ..add(0x20, "Tailscale") };
        assert_eq!(t.apply(&same, &mut Fake), (true, vec![]));
        let tip = Notify { tip: "Tailscale: Connected".into(), ..note(NIM_MODIFY, 0x20, 0, NIF_TIP) };
        let (ok, ch) = t.apply(&tip, &mut Fake);
        assert!(ok);
        assert_eq!(kinds(&ch), vec![(ChangeKind::Changed, 1)]);
        assert_eq!(ch[0].icon.tip, "Tailscale: Connected");
        // A modify without NIF_ICON keeps the image.
        assert!(ch[0].icon.icon.is_some());

        // Hidden: removed for pages, but kept; shown again: added.
        let hide = Notify { state: 1, state_mask: 1, ..note(NIM_MODIFY, 0x20, 0, NIF_STATE) };
        assert_eq!(kinds(&t.apply(&hide, &mut Fake).1), vec![(ChangeKind::Removed, 1)]);
        assert!(t.list().is_empty());
        let show = Notify { state: 0, state_mask: 1, ..hide.clone() };
        assert_eq!(kinds(&t.apply(&show, &mut Fake).1), vec![(ChangeKind::Added, 1)]);

        let (ok, ch) = t.apply(&note(NIM_DELETE, 0x20, 0, 0), &mut Fake);
        assert!(ok);
        assert_eq!(kinds(&ch), vec![(ChangeKind::Removed, 1)]);
        assert!(!t.apply(&note(NIM_DELETE, 0x20, 0, 0), &mut Fake).0);
        // Without a window an icon cannot be added.
        assert!(!t.apply(&add(0, "ghost"), &mut Fake).0);
    }

    #[test]
    fn unknown_modify_is_kept_but_failed() {
        // Windows Security answers TaskbarCreated with a modify (no click
        // message), then adds in full when that fails.
        let mut t = Table::default();
        let modify = Notify { tip: "Windows-Sicherheit".into(), hicon: 3, ..note(NIM_MODIFY, 0x30, 100, NIF_TIP | NIF_ICON) };
        let (ok, ch) = t.apply(&modify, &mut Fake);
        assert!(!ok);
        assert_eq!(kinds(&ch), vec![(ChangeKind::Added, 1)]);
        assert!(!ch[0].icon.clickable);
        let full = Notify { uid: 100, ..add(0x30, "") };
        let (ok, ch) = t.apply(&full, &mut Fake);
        assert!(ok);
        assert_eq!(kinds(&ch), vec![(ChangeKind::Changed, 1)]);
        assert!(ch[0].icon.clickable);
        // Its add carries an empty tooltip: the one from before stays.
        assert_eq!(ch[0].icon.tip, "Windows-Sicherheit");
        assert_eq!(t.list().len(), 1);
    }

    #[test]
    fn matches_by_guid_and_tracks_versions() {
        let guid = Some("0d7ad4b6-a17f-155b-1945-07efff7fea9b".to_string());
        let mut t = Table::default();
        let first = Notify { guid: guid.clone(), flags: add(0, "").flags | NIF_GUID, ..add(0x40, "WhatsApp") };
        t.apply(&first, &mut Fake);
        // Later calls name the icon by GUID alone.
        let v4 = Notify { guid: guid.clone(), version: 4, ..note(NIM_SETVERSION, 0, 0, NIF_GUID) };
        assert!(t.apply(&v4, &mut Fake).0);
        assert_eq!(t.target(1, (5, 6)).unwrap().version, 4);
        assert!(!t.apply(&Notify { version: 5, ..v4.clone() }, &mut Fake).0);
        let tip = Notify { guid: guid.clone(), tip: "WhatsApp (2)".into(), ..note(NIM_MODIFY, 0, 0, NIF_GUID | NIF_TIP) };
        assert_eq!(kinds(&t.apply(&tip, &mut Fake).1), vec![(ChangeKind::Changed, 1)]);
        // uVersion on an add counts; with NIF_INFO it is the balloon timeout.
        let mut t = Table::default();
        t.apply(&Notify { version: 3, ..add(0x50, "spacedesk") }, &mut Fake);
        assert_eq!(t.target(1, (0, 0)).unwrap().version, 3);
        let balloon = Notify { version: 2, ..note(NIM_MODIFY, 0x50, 0, NIF_INFO) };
        t.apply(&balloon, &mut Fake);
        assert_eq!(t.target(1, (0, 0)).unwrap().version, 3);
    }

    #[test]
    fn marks_explorer_icons_and_sweeps_dead_ones() {
        let mut t = Table::default();
        let (_, ch) = t.apply(&add(0x10, "Lautsprecher: 42%"), &mut Fake);
        assert!(ch[0].icon.system);
        t.apply(&add(0x20, "Tailscale"), &mut Fake);
        let gone = t.sweep(|h| h != 0x20);
        assert_eq!(kinds(&gone), vec![(ChangeKind::Removed, 2)]);
        assert_eq!(t.list().len(), 1);
    }

    #[test]
    fn targets_clickable_icons_and_remembers_the_point() {
        let mut t = Table::default();
        t.apply(&add(0x20, "Tailscale"), &mut Fake);
        let silent = Notify { callback: 0, ..add(0x30, "Lautsprecher") };
        t.apply(&silent, &mut Fake);
        let target = t.target(1, (1500, 1180)).unwrap();
        assert_eq!(target, Target { hwnd: 0x20, uid: 0, callback: 0x403, version: 0, pid: 7 });
        let q = RectQuery { corner: 1, hwnd: 0x20, uid: 0, guid: None };
        assert_eq!(t.point_of(&q), Some(Some((1500, 1180))));
        assert_eq!(t.point_of(&RectQuery { hwnd: 0x99, ..q }), None);
        assert!(t.target(2, (0, 0)).is_err()); // no callback message
        assert!(t.target(9, (0, 0)).is_err());
    }

    #[test]
    fn click_messages_follow_the_icon_version() {
        assert_eq!(click_messages(Button::Left, 0), vec![WM_LBUTTONDOWN, WM_LBUTTONUP]);
        assert_eq!(click_messages(Button::Left, 4), vec![WM_LBUTTONDOWN, WM_LBUTTONUP, NIN_SELECT]);
        assert_eq!(click_messages(Button::Right, 3), vec![WM_RBUTTONDOWN, WM_RBUTTONUP, WM_CONTEXTMENU]);
        assert_eq!(click_messages(Button::Right, 0), vec![WM_RBUTTONDOWN, WM_RBUTTONUP]);
        assert_eq!(click_messages(Button::Double, 4), vec![WM_LBUTTONDBLCLK, WM_LBUTTONUP]);
        assert_eq!(click_messages(Button::Middle, 4), vec![WM_MBUTTONDOWN, WM_MBUTTONUP]);
        let b: Button = serde_json::from_str(r#""double""#).unwrap();
        assert_eq!(b, Button::Double);
        assert!(serde_json::from_str::<Button>(r#""hover""#).is_err());

        // Version 4: point in wParam (16-bit each, negative on a left monitor),
        // message and icon number in lParam.
        let (wp, lp) = callback_params(4, 3, WM_CONTEXTMENU, -1200, 1180);
        assert_eq!(wp & 0xFFFF, (-1200i32 as u16) as usize);
        assert_eq!(wp >> 16, 1180);
        assert_eq!(lp, (WM_CONTEXTMENU | (3 << 16)) as isize);
        assert_eq!(callback_params(0, 3, WM_LBUTTONUP, 10, 20), (3, WM_LBUTTONUP as isize));
    }

    #[test]
    fn batches_changes_per_icon() {
        let icon = |n: u32| json!({ "tip": n });
        let mut batch = vec![json!({ "type": "added", "id": 1, "icon": icon(1) })];
        merge(&mut batch, json!({ "type": "changed", "id": 1, "icon": icon(2) }));
        merge(&mut batch, json!({ "type": "changed", "id": 2, "icon": icon(3) }));
        merge(&mut batch, json!({ "type": "changed", "id": 2, "icon": icon(4) }));
        merge(&mut batch, json!({ "type": "removed", "id": 1, "icon": icon(2) }));
        merge(&mut batch, json!({ "type": "active", "active": false }));
        let shape: Vec<(String, Value)> = batch
            .iter()
            .map(|p| (p["type"].as_str().unwrap().to_string(), p["icon"]["tip"].clone()))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("added".into(), json!(2)),
                ("changed".into(), json!(4)),
                ("removed".into(), json!(2)),
                ("active".into(), Value::Null)
            ]
        );
    }

    #[test]
    fn change_payload_shape() {
        let mut t = Table::default();
        let (_, ch) = t.apply(&add(0x20, "Tailscale"), &mut Fake);
        let v = change_json(&ch[0]);
        assert_eq!(v["type"], "added");
        assert_eq!(v["id"], 1);
        assert_eq!(v["icon"]["tip"], "Tailscale");
        assert_eq!(v["icon"]["clickable"], true);
        // Handles and click details stay in the host.
        assert!(v["icon"].get("hwnd").is_none());
        assert!(v["icon"].get("callback").is_none());
    }

    /// Takes over the real tray for a few seconds and prints what arrives:
    /// `cargo test --lib live_tray -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_tray() {
        imp::start(Box::new(|push| {
            eprintln!(
                "push {} {} {}",
                push["type"],
                push["icon"]["app"],
                push["icon"]["tip"].as_str().unwrap_or("").replace('\n', " | ")
            )
        }));
        std::thread::sleep(Duration::from_secs(4));
        let s = state();
        assert_eq!(s["active"], true);
        for i in s["icons"].as_array().unwrap() {
            eprintln!(
                "{:>3} {:<28} system={:<5} clickable={:<5} icon={:>6} {}",
                i["id"],
                i["app"].as_str().unwrap_or("?"),
                i["system"],
                i["clickable"],
                i["icon"].as_str().map(|s| s.len()).unwrap_or(0),
                i["tip"].as_str().unwrap_or("").replace('\n', " | ")
            );
        }
        imp::stop();
        assert_eq!(state()["active"], false);
    }
}
