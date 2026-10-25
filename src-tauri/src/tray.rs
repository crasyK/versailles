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
//!
//! The same window carries the `TaskbandHWND` property apps' TaskbarList
//! objects look for, naming a window of ours: taskbar progress and overlay
//! badges (`taskband`) reach Versailles and go on to Explorer's taskband.
//! So the tray also runs for a page that declares only `windows`.

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
/// A version 4 icon asks for the standard tooltip (it has no pop-up of its own).
const NIF_SHOWTIP: u32 = 0x80;
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
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_MOUSEHOVER: u32 = 0x02A1;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// `NIN_SELECT` (`WM_USER`): a version 3+ icon was picked with the mouse.
const NIN_SELECT: u32 = 0x0400;
/// `NIN_KEYSELECT`: picked with Enter or Space.
const NIN_KEYSELECT: u32 = 0x0401;
/// A version 4 icon may show its own pop-up instead of a tooltip.
const NIN_POPUPOPEN: u32 = 0x0406;
const NIN_POPUPCLOSE: u32 = 0x0407;
/// Half the side of the square we tell apps their icon is (around the point
/// it was last clicked or hovered at).
const HALF: i32 = 12;

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
    /// Balloon (`NIF_INFO` only): `szInfoTitle`, `szInfo`, `dwInfoFlags`.
    pub info_title: String,
    pub info: String,
    pub info_flags: u32,
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
    let info = flags & NIF_INFO != 0;
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
        info_title: if info { wstr_at(n, 804, 64) } else { String::new() },
        info: if info { wstr_at(n, 288, 256) } else { String::new() },
        info_flags: if info { u32_at(n, 932).unwrap_or(0) } else { 0 },
    })
}

/// The balloon icon a page sees (`dwInfoFlags & NIIF_ICON_MASK`).
fn balloon_icon(flags: u32) -> &'static str {
    match flags & 0x0F {
        1 => "info",
        2 => "warning",
        3 => "error",
        4 => "user",
        _ => "none",
    }
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

/// `APPBARMSGDATAV3`, what `SHAppBarMessage` copies to the taskbar: an
/// `APPBARDATA` with 32-bit handles (40 bytes), the message, then a shared
/// memory handle and the process it belongs to.
pub const APPBAR_MSG_LEN: usize = 64;
/// `APPBARDATA` as it sits in that shared memory.
pub const APPBAR_DATA_LEN: usize = 40;
pub const ABM_QUERYPOS: u32 = 2;
pub const ABM_SETPOS: u32 = 3;
pub const ABM_GETTASKBARPOS: u32 = 5;

/// The AppBar message and its shared memory `(handle, process)`, for the
/// messages that answer through it.
pub fn appbar_shared(data: &[u8]) -> Option<(u32, i64, u32)> {
    if data.len() < APPBAR_MSG_LEN {
        return None;
    }
    let message = u32_at(data, 40)?;
    if !matches!(message, ABM_QUERYPOS | ABM_SETPOS | ABM_GETTASKBARPOS) {
        return None;
    }
    let handle = i64::from_le_bytes(data.get(48..56)?.try_into().ok()?);
    Some((message, handle, u32_at(data, 56)?))
}

/// The same AppBar message pointing at other shared memory.
pub fn with_shared(data: &[u8], handle: i64, pid: u32) -> Vec<u8> {
    let mut out = data.to_vec();
    out[48..56].copy_from_slice(&handle.to_le_bytes());
    out[56..60].copy_from_slice(&pid.to_le_bytes());
    out
}

/// One tray icon as pages see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayIcon {
    /// Versailles' number for the icon (for this run); pass it to `click`.
    pub id: u32,
    /// The tooltip (may hold several lines).
    pub tip: String,
    /// Whether Windows would show `tip` as a tooltip: always below version 4;
    /// a version 4 icon asks with `NIF_SHOWTIP` (otherwise it draws its own
    /// pop-up on hover, or none).
    pub show_tip: bool,
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
    /// `NIF_SHOWTIP` as the app last set it.
    #[serde(skip)]
    tip_flag: bool,
    /// `NIS_HIDDEN`: kept, but not listed.
    #[serde(skip)]
    hidden: bool,
    /// Where it was last clicked or hovered (`Shell_NotifyIconGetRect`
    /// answers a square around it).
    #[serde(skip)]
    point: Option<(i32, i32)>,
}

impl TrayIcon {
    /// Whether anything a page sees differs.
    fn same_view(&self, other: &TrayIcon) -> bool {
        self.tip == other.tip
            && self.show_tip == other.show_tip
            && self.icon == other.icon
            && self.guid == other.guid
            && self.clickable == other.clickable
    }

    fn update_show_tip(&mut self) {
        self.show_tip = self.tip_flag || self.version < 4;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    /// New, or shown again after being hidden.
    Added,
    /// Tooltip (or whether to show it), image or clickability.
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
                let Some(i) = at else {
                    return (true, Vec::new());
                };
                let before = self.icons[i].clone();
                let icon = &mut self.icons[i];
                icon.version = n.version;
                // Version 4 hides the standard tooltip unless it asks for it.
                icon.update_show_tip();
                let changes = if !icon.hidden && !before.same_view(icon) {
                    vec![Change { kind: ChangeKind::Changed, icon: icon.clone() }]
                } else {
                    Vec::new()
                };
                (true, changes)
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
                    show_tip: true,
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
                    tip_flag: false,
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
        // NIF_SHOWTIP comes with the tooltip (or the add); a modify that only
        // changes the image keeps it.
        if n.message == NIM_ADD || n.flags & (NIF_TIP | NIF_SHOWTIP) != 0 {
            icon.tip_flag = n.flags & NIF_SHOWTIP != 0;
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
        icon.update_show_tip();
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

    /// The balloon an add or modify shows, as a push: `{ type: "balloon", id,
    /// title, text, kind }`. Call after `apply`. Explorer gets the same call
    /// and shows it as a Windows notification (and tells the app what the
    /// user did with it): this is for pages that show it too.
    pub fn balloon(&self, n: &Notify) -> Option<Value> {
        if !matches!(n.message, NIM_ADD | NIM_MODIFY) || n.flags & NIF_INFO == 0 {
            return None;
        }
        // Empty title and text take a balloon down.
        if n.info_title.is_empty() && n.info.is_empty() {
            return None;
        }
        let icon = &self.icons[self.find(n.hwnd, n.uid, n.guid.as_deref())?];
        if icon.hidden {
            return None;
        }
        Some(json!({
            "type": "balloon",
            "id": icon.id,
            "title": n.info_title,
            "text": n.info,
            "kind": balloon_icon(n.info_flags),
        }))
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

    /// Where icon `id` was last clicked or hovered.
    pub fn last_point(&self, id: u32) -> Option<(i32, i32)> {
        self.icons.iter().find(|i| i.id == id).and_then(|i| i.point)
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

/// Mouse buttons and keys a page can send. They map to browser events one to
/// one: `click` → `left`, `contextmenu` → `right`, `dblclick` → `double` (the
/// second half of a double click: the browser already sent the first click);
/// Enter / Space on a focused icon → `key`, the menu key / Shift+F10 → `menu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Left,
    Right,
    Double,
    Middle,
    Key,
    Menu,
}

impl Button {
    /// From the keyboard: the app is told where the icon is, not the cursor.
    pub fn keyboard(self) -> bool {
        matches!(self, Button::Key | Button::Menu)
    }
}

/// The callback messages Explorer sends for a click or key, in order.
/// Version 3+ icons also get `NIN_SELECT` / `WM_CONTEXTMENU` (documented for
/// version 4; Explorer sends them to 3 as well), and from the keyboard
/// `NIN_KEYSELECT` / `WM_CONTEXTMENU` alone (the "version 5.0 Shell"
/// behaviour, which `NOTIFYICON_VERSION` 3 opts into); older icons get a
/// left or right click instead.
pub fn click_messages(button: Button, version: u32) -> Vec<u32> {
    let v3 = version >= 3;
    let mut out = match button {
        Button::Left => vec![WM_LBUTTONDOWN, WM_LBUTTONUP],
        Button::Right => vec![WM_RBUTTONDOWN, WM_RBUTTONUP],
        Button::Double => vec![WM_LBUTTONDBLCLK, WM_LBUTTONUP],
        Button::Middle => vec![WM_MBUTTONDOWN, WM_MBUTTONUP],
        Button::Key if v3 => vec![NIN_KEYSELECT],
        Button::Menu if v3 => vec![WM_CONTEXTMENU],
        Button::Key => vec![WM_LBUTTONDOWN, WM_LBUTTONUP],
        Button::Menu => vec![WM_RBUTTONDOWN, WM_RBUTTONUP],
    };
    match button {
        Button::Left if v3 => out.push(NIN_SELECT),
        Button::Right if v3 => out.push(WM_CONTEXTMENU),
        _ => {}
    }
    out
}

/// The callback messages for the pointer entering or leaving an icon, as
/// ManagedShell sends them: a move and a hover on entry, a leave on exit,
/// and for version 4 icons `NIN_POPUPOPEN` / `NIN_POPUPCLOSE` (their own
/// pop-up instead of a tooltip).
pub fn hover_messages(over: bool, version: u32) -> Vec<u32> {
    let v4 = version >= 4;
    let mut out = if over { vec![WM_MOUSEMOVE, WM_MOUSEHOVER] } else { vec![WM_MOUSELEAVE] };
    if v4 {
        out.push(if over { NIN_POPUPOPEN } else { NIN_POPUPCLOSE });
    }
    out
}

/// The point a callback message carries: where the pointer is, or for the
/// keyboard the icon's top-left corner (the square we report around `at`).
pub fn message_point(button: Button, at: (i32, i32)) -> (i32, i32) {
    if button.keyboard() {
        (at.0 - HALF, at.1 - HALF)
    } else {
        at
    }
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

/// The pointer entered (`over`) or left icon `id` at `at` (physical screen
/// pixels; the cursor by default).
pub fn hover(id: u32, over: bool, at: Option<(i32, i32)>) -> AppResult<()> {
    imp::hover(id, over, at)
}

/// Whether some piece on the page declares `tray`, or `windows` (taskbar
/// progress and badges come through the tray window, see `taskband`).
fn declared(app: &AppHandle) -> bool {
    let state = app.state::<crate::state::AppState>();
    crate::desktop::page_catalog(&state)
        .pieces
        .iter()
        .any(|p| p.hooks.iter().any(|h| h == "tray" || h == "windows"))
}

/// Background watcher: run the tray while this Versailles hides the Windows
/// taskbar and some piece declares `tray` or `windows`; otherwise Explorer
/// keeps it.
pub fn start_watcher(app: AppHandle) {
    let _ = std::thread::Builder::new().name("versailles-tray-watch".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        // Not on the way out: the exit stops the tray before the taskbar,
        // and starting it again in between kept the exit waiting.
        let want = !crate::crash::exiting() && crate::taskbar::hiding() && declared(&app);
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
    use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::UI::Shell::{SHAllocShared, SHFreeShared, SHLockShared, SHUnlockShared};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
        GetCursorPos, GetPropW, GetWindowLongW, GetWindowThreadProcessId, IsWindow, MsgWaitForMultipleObjects,
        PeekMessageW, PostMessageW, RegisterClassW, RegisterWindowMessageW, RemovePropW, SendMessageTimeoutW,
        SendNotifyMessageW, SetPropW, SetWindowLongW, SetWindowPos, TranslateMessage, CS_DBLCLKS, GWL_STYLE,
        HWND_BROADCAST, HWND_TOPMOST, MSG,
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
    /// A second `TaskbarCreated` this long after the first, for apps that
    /// were too busy to answer it.
    const REBROADCAST: Duration = Duration::from_secs(3);
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
        // Called on the main thread when Versailles exits or updates: never
        // wait forever, whatever the tray thread is stuck on.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !r.thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if r.thread.is_finished() {
            let _ = r.thread.join();
        } else {
            tracing::warn!("tray: the tray thread did not stop within 5 s; going on without it");
        }
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

    /// AppBar messages that answer through shared memory (QUERYPOS, SETPOS,
    /// GETTASKBARPOS). shell32 shares that memory with the process owning the
    /// first `Shell_TrayWnd`, which is us, so Explorer could not read it:
    /// copy it into memory shared with Explorer, forward, copy the answer
    /// back (as ManagedShell's AppBarManager does). Without this, docked
    /// bars that registered while the tray ran were accepted but never got
    /// their space.
    unsafe fn forward_appbar(data: &[u8]) -> Option<LRESULT> {
        let (_, handle, source) = appbar_shared(data)?;
        let explorer = explorer()?;
        let mut pid = 0u32;
        GetWindowThreadProcessId(explorer, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let old = SHLockShared(HANDLE(handle as isize as *mut _), source);
        if old.is_null() {
            return None;
        }
        let new = SHAllocShared(Some(old as *const _), APPBAR_DATA_LEN as u32, pid);
        if new.is_invalid() {
            let _ = SHUnlockShared(old);
            return None;
        }
        let mut msg = with_shared(data, new.0 as isize as i64, pid);
        let cds = COPYDATASTRUCT {
            dwData: 0,
            cbData: msg.len() as u32,
            lpData: msg.as_mut_ptr() as *mut _,
        };
        // wParam: the bar's window, as shell32 sends it.
        let bar = WPARAM(u32_at(data, 4).unwrap_or(0) as i32 as isize as usize);
        let mut result = 0usize;
        SendMessageTimeoutW(
            explorer,
            WM_COPYDATA,
            bar,
            LPARAM(&cds as *const COPYDATASTRUCT as isize),
            SMTO_ABORTIFHUNG,
            4000,
            Some(&mut result),
        );
        let answer = SHLockShared(new, pid);
        if !answer.is_null() {
            std::ptr::copy_nonoverlapping(answer as *const u8, old as *mut u8, APPBAR_DATA_LEN);
            let _ = SHUnlockShared(answer);
        }
        let _ = SHUnlockShared(old);
        let _ = SHFreeShared(new, pid);
        Some(LRESULT(result as isize))
    }

    fn on_notify(data: &[u8]) -> bool {
        let Some(n) = parse(data) else {
            return false;
        };
        let (ok, changes, balloon) = {
            let mut t = table().lock().unwrap();
            let (ok, changes) = t.apply(&n, &mut Win);
            (ok, changes, t.balloon(&n))
        };
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
        // Never answered with NIN_BALLOON*: Explorer shows it and does that.
        if let Some(b) = balloon {
            emit(b);
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
    /// around where the icon was last clicked or hovered (or the cursor).
    fn on_rect_query(data: &[u8]) -> Option<isize> {
        let q = parse_rect_query(data)?;
        let point = table().lock().unwrap().point_of(&q)?;
        let (x, y) = point.or_else(cursor)?;
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
                    // AppBar messages (docked bars): Explorer answers.
                    0 => {
                        if let Some(r) = forward_appbar(data) {
                            return r;
                        }
                    }
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

    /// Explorer's taskband (`MSTaskSwWClass`), named by the `TaskbandHWND`
    /// property of Explorer's own `Shell_TrayWnd`.
    fn explorer_band(ours: HWND) -> Option<HWND> {
        let tray = explorer()?;
        let band = HWND(unsafe { GetPropW(tray, w!("TaskbandHWND")) }.0);
        (!band.is_invalid() && band != ours && unsafe { IsWindow(Some(band)) }.as_bool()).then_some(band)
    }

    /// The overlay description: a NUL-terminated string in memory the app
    /// shared (`SHAllocShared`) with the taskbar.
    unsafe fn shared_text(app_window: HWND, handle: isize) -> Option<String> {
        if handle == 0 {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(app_window, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let p = SHLockShared(HANDLE(handle as *mut _), pid) as *const u16;
        if p.is_null() {
            return None;
        }
        // At most 1024 characters: well inside the block's first page.
        let mut units = Vec::new();
        for i in 0..1024 {
            let c = *p.add(i);
            if c == 0 {
                break;
            }
            units.push(c);
        }
        let _ = SHUnlockShared(p as *const _);
        Some(String::from_utf16_lossy(&units))
    }

    /// The window apps' TaskbarList objects talk to while ours is the first
    /// `Shell_TrayWnd` (see `taskband`): note progress and badges, then pass
    /// every message on to Explorer's taskband and answer what it answers.
    unsafe extern "system" fn band_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        use crate::taskband::{self, Message};
        if msg < WM_USER {
            return DefWindowProcW(h, msg, w, l);
        }
        let id = w.0 as isize as i64;
        let message = match msg {
            taskband::SET_PROGRESS_VALUE => Some(Message::Value { id, value: l.0 as u32 }),
            taskband::SET_PROGRESS_STATE => Some(Message::State { id, flags: l.0 as u32 }),
            // The app may destroy the icon once the call returns: draw it now.
            taskband::SET_OVERLAY_ICON => Some(Message::Overlay {
                id,
                icon: if l.0 == 0 { None } else { crate::app_windows::icon_handle_url(l.0) },
            }),
            taskband::SET_OVERLAY_LABEL => Some(Message::Label { id, label: shared_text(hwnd(id), l.0) }),
            _ => None,
        };
        if let Some(m) = message {
            taskband::apply(m);
        }
        let Some(band) = explorer_band(h) else {
            return LRESULT(0);
        };
        let mut result = 0usize;
        SendMessageTimeoutW(band, msg, w, l, SMTO_ABORTIFHUNG, 4000, Some(&mut result));
        LRESULT(result as isize)
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
                (w!("VersaillesTaskband"), band_proc),
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

    /// The window named by our tray's `TaskbandHWND` property (see
    /// `taskband`). Never visible.
    fn create_band(tray: HWND) -> Option<HWND> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
            let band = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("VersaillesTaskband"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|err| tracing::warn!("tray: taskband window failed: {err}"))
            .ok()?;
            if let Err(err) = SetPropW(tray, w!("TaskbandHWND"), Some(HANDLE(band.0))) {
                tracing::warn!("tray: TaskbandHWND not set: {err}");
                let _ = DestroyWindow(band);
                return None;
            }
            Some(band)
        }
    }

    /// Tell the apps behind the listed windows their taskbar button is new,
    /// as Explorer does after a restart (and ManagedShell on start): they set
    /// their progress and overlay badges again, now through us.
    fn announce_buttons() {
        let msg = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
        if msg == 0 {
            return;
        }
        for w in crate::app_windows::list(false).unwrap_or_default() {
            unsafe {
                let _ = SendNotifyMessageW(hwnd(w.id), msg, WPARAM(0), LPARAM(0));
            }
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
        let band = create_band(tray);
        ACTIVE.store(true, Ordering::SeqCst);
        emit(json!({ "type": "active", "active": true }));
        broadcast_taskbar_created();
        if band.is_some() {
            announce_buttons();
        }
        tracing::info!(
            "tray: holding the notification icons{}",
            if band.is_some() { " and taskbar progress" } else { "" }
        );

        let started = Instant::now();
        let mut rebroadcast = true;
        let mut swept = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            if rebroadcast && started.elapsed() >= REBROADCAST {
                rebroadcast = false;
                broadcast_taskbar_created();
            }
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
            if let Some(band) = band {
                let _ = RemovePropW(tray, w!("TaskbandHWND"));
                let _ = DestroyWindow(band);
            }
            if let Some(child) = child {
                let _ = DestroyWindow(child);
            }
            let _ = DestroyWindow(tray);
        }
        // Explorer got every message too and draws them again; apps that
        // kept our window find Explorer's when they set up their button anew.
        crate::taskband::clear();
        if band.is_some() {
            announce_buttons();
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

    fn ensure_active() -> AppResult<()> {
        if active() {
            return Ok(());
        }
        Err(AppError::msg(
            "tray: Explorer holds the icons (Versailles lists them only while it hides the taskbar)",
        ))
    }

    /// Send callback messages to the icon's app, all with the same point.
    fn send(id: u32, t: &Target, messages: &[u32], (x, y): (i32, i32)) -> AppResult<()> {
        let owner = hwnd(t.hwnd);
        unsafe {
            if !IsWindow(Some(owner)).as_bool() {
                return Err(AppError::msg(format!("tray: the app of icon {id} is gone")));
            }
            for &m in messages {
                let (wp, lp) = callback_params(t.version, t.uid, m, x, y);
                SendNotifyMessageW(owner, t.callback, WPARAM(wp), LPARAM(lp))
                    .map_err(|e| AppError::msg(format!("tray: message to icon {id} failed: {e}")))?;
            }
        }
        Ok(())
    }

    pub fn click(id: u32, button: Button, at: Option<(i32, i32)>) -> AppResult<()> {
        ensure_active()?;
        // The keyboard has no pointer: use where the icon was last clicked or
        // hovered, as the page knows nothing better.
        let remembered = if button.keyboard() && at.is_none() {
            table().lock().unwrap().last_point(id)
        } else {
            None
        };
        let point = at.or(remembered).or_else(cursor).unwrap_or((0, 0));
        let t = table().lock().unwrap().target(id, point)?;
        // Lets the app bring its menu or window to the front.
        unsafe {
            let _ = AllowSetForegroundWindow(t.pid);
        }
        send(id, &t, &click_messages(button, t.version), message_point(button, point))
    }

    pub fn hover(id: u32, over: bool, at: Option<(i32, i32)>) -> AppResult<()> {
        ensure_active()?;
        let point = at.or_else(cursor).unwrap_or((0, 0));
        let t = {
            let mut table = table().lock().unwrap();
            if over {
                table.target(id, point)?
            } else {
                // Leaving keeps the point the pop-up was placed by.
                let kept = table.last_point(id).unwrap_or(point);
                table.target(id, kept)?
            }
        };
        send(id, &t, &hover_messages(over, t.version), point)
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
    pub fn hover(_id: u32, _over: bool, _at: Option<(i32, i32)>) -> AppResult<()> {
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
        let text = |b: &mut Vec<u8>, at: usize, s: &str| {
            for (i, u) in s.encode_utf16().enumerate() {
                b[8 + at + i * 2..8 + at + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
        };
        text(&mut b, 288, &n.info);
        text(&mut b, 804, &n.info_title);
        put(&mut b, 8 + 932, n.info_flags);
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

        // A balloon: szInfo at 288, szInfoTitle at 804, dwInfoFlags at 932,
        // read only with NIF_INFO.
        let balloon = Notify {
            info_title: "Tailscale".into(),
            info: "Connected to office".into(),
            info_flags: 2 | 0x10, // NIIF_WARNING | NIIF_NOSOUND
            version: 10_000,      // the timeout, with NIF_INFO
            ..note(NIM_MODIFY, 0x1067a, 0, NIF_INFO)
        };
        assert_eq!(parse(&bytes(&balloon)), Some(balloon.clone()));
        let plain = parse(&bytes(&Notify { flags: NIF_TIP, ..balloon.clone() })).unwrap();
        assert_eq!((plain.info.as_str(), plain.info_title.as_str(), plain.info_flags), ("", "", 0));
    }

    #[test]
    fn pushes_balloons_for_known_icons() {
        let mut t = Table::default();
        t.apply(&add(0x20, "Tailscale"), &mut Fake);
        let info = |title: &str, text: &str, flags: u32| Notify {
            info_title: title.into(),
            info: text.into(),
            info_flags: flags,
            ..note(NIM_MODIFY, 0x20, 0, NIF_INFO)
        };
        let n = info("Tailscale", "Connected", 1);
        // The balloon is not a change of the icon.
        assert_eq!(t.apply(&n, &mut Fake), (true, vec![]));
        assert_eq!(
            t.balloon(&n),
            Some(json!({ "type": "balloon", "id": 1, "title": "Tailscale", "text": "Connected", "kind": "info" }))
        );
        // A title or a text is enough; icons by NIIF_ICON_MASK.
        assert_eq!(t.balloon(&info("", "Text only", 3 | 0x20)).unwrap()["kind"], "error");
        assert_eq!(t.balloon(&info("Title only", "", 4)).unwrap()["kind"], "user");
        assert_eq!(t.balloon(&info("x", "", 0)).unwrap()["kind"], "none");
        assert_eq!(t.balloon(&info("x", "", 2)).unwrap()["kind"], "warning");
        // Empty title and text take a balloon down: nothing to push.
        assert_eq!(t.balloon(&info("", "", 1)), None);
        // Without NIF_INFO, for an unknown or hidden icon, or on a delete: none.
        assert_eq!(t.balloon(&Notify { flags: NIF_TIP, ..n.clone() }), None);
        assert_eq!(t.balloon(&Notify { hwnd: 0x99, ..n.clone() }), None);
        assert_eq!(t.balloon(&Notify { message: NIM_DELETE, ..n.clone() }), None);
        t.apply(&Notify { state: 1, state_mask: 1, ..note(NIM_MODIFY, 0x20, 0, NIF_STATE) }, &mut Fake);
        assert_eq!(t.balloon(&n), None);
        // An add that carries a balloon pushes it too.
        let mut t = Table::default();
        let first = Notify { flags: add(0, "").flags | NIF_INFO, info: "Hi".into(), ..add(0x30, "App") };
        t.apply(&first, &mut Fake);
        assert_eq!(t.balloon(&first).unwrap()["text"], "Hi");
    }

    #[test]
    fn show_tip_follows_version_and_flag() {
        let mut t = Table::default();
        // Below version 4 Windows always shows the tooltip.
        let (_, ch) = t.apply(&add(0x20, "Tailscale"), &mut Fake);
        assert!(ch[0].icon.show_tip);
        assert_eq!(change_json(&ch[0])["icon"]["showTip"], true);
        // Version 4 without NIF_SHOWTIP: the app draws its own pop-up.
        let v4 = Notify { version: 4, ..note(NIM_SETVERSION, 0x20, 0, 0) };
        let (ok, ch) = t.apply(&v4, &mut Fake);
        assert!(ok);
        assert_eq!(kinds(&ch), vec![(ChangeKind::Changed, 1)]);
        assert!(!ch[0].icon.show_tip);
        // Asking for it with the tooltip shows it; an image-only modify keeps it.
        let tip = Notify { tip: "Tailscale: Connected".into(), ..note(NIM_MODIFY, 0x20, 0, NIF_TIP | NIF_SHOWTIP) };
        assert!(t.apply(&tip, &mut Fake).1[0].icon.show_tip);
        let image = Notify { hicon: 9, ..note(NIM_MODIFY, 0x20, 0, NIF_ICON) };
        assert!(t.apply(&image, &mut Fake).1[0].icon.show_tip);
        // A tooltip without the flag drops it again.
        let plain = Notify { tip: "Tailscale".into(), ..note(NIM_MODIFY, 0x20, 0, NIF_TIP) };
        assert!(!t.apply(&plain, &mut Fake).1[0].icon.show_tip);
        // The same version again: nothing to push.
        assert_eq!(t.apply(&v4, &mut Fake), (true, vec![]));
    }

    #[test]
    fn finds_and_moves_appbar_shared_memory() {
        let mut m = vec![0u8; APPBAR_MSG_LEN];
        m[40..44].copy_from_slice(&ABM_SETPOS.to_le_bytes());
        m[48..56].copy_from_slice(&0x1234i64.to_le_bytes());
        m[56..60].copy_from_slice(&77u32.to_le_bytes());
        assert_eq!(appbar_shared(&m), Some((ABM_SETPOS, 0x1234, 77)));
        let moved = with_shared(&m, 0x99, 8944);
        assert_eq!(appbar_shared(&moved), Some((ABM_SETPOS, 0x99, 8944)));
        assert_eq!(&moved[..48], &m[..48]);
        // Messages without shared memory pass through as they are.
        let mut new = m.clone();
        new[40..44].copy_from_slice(&0u32.to_le_bytes()); // ABM_NEW
        assert_eq!(appbar_shared(&new), None);
        assert_eq!(appbar_shared(&m[..40]), None);
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
        // Keyboard: Enter / Space and the menu key / Shift+F10.
        assert_eq!(click_messages(Button::Key, 4), vec![NIN_KEYSELECT]);
        assert_eq!(click_messages(Button::Key, 3), vec![NIN_KEYSELECT]);
        assert_eq!(click_messages(Button::Menu, 4), vec![WM_CONTEXTMENU]);
        assert_eq!(click_messages(Button::Key, 0), vec![WM_LBUTTONDOWN, WM_LBUTTONUP]);
        assert_eq!(click_messages(Button::Menu, 0), vec![WM_RBUTTONDOWN, WM_RBUTTONUP]);
        let b: Button = serde_json::from_str(r#""double""#).unwrap();
        assert_eq!(b, Button::Double);
        assert_eq!(serde_json::from_str::<Button>(r#""menu""#).unwrap(), Button::Menu);
        assert!(serde_json::from_str::<Button>(r#""hover""#).is_err());
        // The keyboard gives the icon's top-left corner, the mouse its point.
        assert_eq!(message_point(Button::Key, (1500, 1180)), (1500 - HALF, 1180 - HALF));
        assert_eq!(message_point(Button::Menu, (-1200, 10)), (-1200 - HALF, 10 - HALF));
        assert_eq!(message_point(Button::Right, (1500, 1180)), (1500, 1180));

        // Version 4: point in wParam (16-bit each, negative on a left monitor),
        // message and icon number in lParam.
        let (wp, lp) = callback_params(4, 3, WM_CONTEXTMENU, -1200, 1180);
        assert_eq!(wp & 0xFFFF, (-1200i32 as u16) as usize);
        assert_eq!(wp >> 16, 1180);
        assert_eq!(lp, (WM_CONTEXTMENU | (3 << 16)) as isize);
        assert_eq!(callback_params(0, 3, WM_LBUTTONUP, 10, 20), (3, WM_LBUTTONUP as isize));
    }

    #[test]
    fn hover_messages_follow_the_icon_version() {
        assert_eq!(hover_messages(true, 0), vec![WM_MOUSEMOVE, WM_MOUSEHOVER]);
        assert_eq!(hover_messages(true, 3), vec![WM_MOUSEMOVE, WM_MOUSEHOVER]);
        assert_eq!(hover_messages(true, 4), vec![WM_MOUSEMOVE, WM_MOUSEHOVER, NIN_POPUPOPEN]);
        assert_eq!(hover_messages(false, 3), vec![WM_MOUSELEAVE]);
        assert_eq!(hover_messages(false, 4), vec![WM_MOUSELEAVE, NIN_POPUPCLOSE]);
        // Hovering remembers where, for Shell_NotifyIconGetRect.
        let mut t = Table::default();
        t.apply(&add(0x20, "Tailscale"), &mut Fake);
        assert_eq!(t.last_point(1), None);
        t.target(1, (40, 50)).unwrap();
        assert_eq!(t.last_point(1), Some((40, 50)));
        assert_eq!(t.last_point(2), None);
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
