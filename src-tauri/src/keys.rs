//! Keys Windows keeps for itself (#31): `data-hotkey="Win"`, `"Copilot"`,
//! `"Win+A"`, and `"shell": { "suppress": ["Win+X", ...] }` in `#versailles`.
//!
//! RegisterHotKey (the global-shortcut plugin) cannot bind a bare Win key and
//! fails on Win+ combos the shell already owns. Anything with Win goes through
//! a low-level keyboard hook instead, the way PowerToys and AutoHotkey do it:
//!
//! - a matched key's down and up are swallowed; the shell never sees them;
//! - before a Win key-up that ends a swallowed combo (or a bound Win tap), an
//!   unassigned key (`vkE8`, tagged) goes out so Start does not open;
//! - the Copilot key sends LShift+LWin+F23, so `"Copilot"` is Win+Shift+F23.
//!
//! The hook runs on its own thread with its own message loop and does no I/O;
//! the action is posted to the main thread. Windows silently drops a hook that
//! takes too long, so it is installed again every 30 s. It exists only while
//! a Win binding or a suppress entry exists, and dies with the process, so
//! the keyboard is never left changed. Win+L and Ctrl+Alt+Del cannot be
//! intercepted, and keys typed into an elevated window are not seen (UIPI).

use std::sync::{OnceLock, RwLock};
use tauri::AppHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// The Win key pressed and released alone.
    WinTap,
    Vk(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: Key,
}

const VK_F23: u16 = 0x86;

/// `"Win"`, `"Copilot"`, `"Win+A"`, `"Ctrl+Win+F1"`... Case-insensitive;
/// `Super` / `Meta` / `Cmd` mean Win. `None` when it is not a key we know.
pub fn parse(accel: &str) -> Option<Combo> {
    let mut c = Combo { ctrl: false, alt: false, shift: false, win: false, key: Key::WinTap };
    let mut key = None;
    for token in accel.split('+').map(|t| t.trim().to_ascii_lowercase()) {
        match token.as_str() {
            "ctrl" | "control" => c.ctrl = true,
            "alt" | "option" => c.alt = true,
            "shift" => c.shift = true,
            "win" | "super" | "meta" | "cmd" | "command" => c.win = true,
            "copilot" => {
                c.win = true;
                c.shift = true;
                key = Some(Key::Vk(VK_F23));
            }
            other => {
                if key.is_some() {
                    return None;
                }
                key = Some(Key::Vk(vk(other)?));
            }
        }
    }
    match key {
        Some(k) => c.key = k,
        // Modifiers only: the Win tap is the one we support.
        None if c.win => c.key = Key::WinTap,
        None => return None,
    }
    Some(c)
}

fn vk(name: &str) -> Option<u16> {
    let b = name.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
        return Some(b[0].to_ascii_uppercase() as u16);
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        return (1..=24).contains(&n).then_some(0x70 + n - 1);
    }
    Some(match name {
        "space" => 0x20,
        "tab" => 0x09,
        "enter" | "return" => 0x0D,
        "esc" | "escape" => 0x1B,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "insert" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "printscreen" => 0x2C,
        "pause" => 0x13,
        "," | "comma" => 0xBC,
        "." | "period" => 0xBE,
        "-" | "minus" => 0xBD,
        "=" | "equal" => 0xBB,
        ";" | "semicolon" => 0xBA,
        "/" | "slash" => 0xBF,
        "`" | "backquote" => 0xC0,
        "[" | "bracketleft" => 0xDB,
        "]" | "bracketright" => 0xDD,
        "\\" | "backslash" => 0xDC,
        "'" | "quote" => 0xDE,
        _ => return None,
    })
}

/// Accelerators the hook handles instead of RegisterHotKey.
pub fn wants_hook(accel: &str) -> bool {
    parse(accel).is_some_and(|c| c.win)
}

/// What the hook matches: a combo and the piece it toggles (`None`: only
/// swallow it).
struct Entry {
    combo: Combo,
    piece: Option<String>,
}

#[derive(Default)]
struct Table {
    binds: Vec<Entry>,
    suppress: Vec<Entry>,
}

impl Table {
    fn lookup(&self, c: &Combo) -> Option<&Entry> {
        self.binds.iter().chain(self.suppress.iter()).find(|e| e.combo == *c)
    }
    fn is_empty(&self) -> bool {
        self.binds.is_empty() && self.suppress.is_empty()
    }
}

fn table() -> &'static RwLock<Table> {
    static TABLE: OnceLock<RwLock<Table>> = OnceLock::new();
    TABLE.get_or_init(|| RwLock::new(Table::default()))
}

fn app_slot() -> &'static OnceLock<AppHandle> {
    static APP: OnceLock<AppHandle> = OnceLock::new();
    &APP
}

/// Set the hook's bindings (`None` keeps the current ones) and suppress list,
/// installing or removing the hook as needed.
pub fn apply(app: &AppHandle, binds: Option<Vec<(String, String)>>, suppress: &[String]) {
    let _ = app_slot().set(app.clone());
    let need = {
        let mut t = table().write().unwrap();
        if let Some(binds) = binds {
            t.binds = binds
                .into_iter()
                .filter_map(|(accel, id)| match parse(&accel) {
                    Some(combo) => {
                        tracing::info!("hotkey {accel} → {id} (keyboard hook)");
                        Some(Entry { combo, piece: Some(id) })
                    }
                    None => {
                        tracing::warn!("invalid data-hotkey '{accel}' on '{id}'");
                        None
                    }
                })
                .collect();
        }
        t.suppress = suppress
            .iter()
            .filter_map(|accel| match parse(accel) {
                Some(combo) => Some(Entry { combo, piece: None }),
                None => {
                    tracing::warn!("shell.suppress: unknown key '{accel}'");
                    None
                }
            })
            .collect();
        !t.is_empty()
    };
    hook::run(need);
}

/// Toggle the piece on the main thread (called from the hook).
fn fire(piece: &str) {
    if let Some(app) = app_slot().get() {
        let handle = app.clone();
        let id = piece.to_string();
        let _ = app.run_on_main_thread(move || crate::window_manager::toggle_hotkey_piece(&handle, &id));
    }
}

#[cfg(windows)]
mod hook {
    use super::{fire, table, Combo, Key};
    use std::cell::{Cell, RefCell};
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::thread::JoinHandle;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WAIT_TIMEOUT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, PostThreadMessageW,
        SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT,
        LLKHF_INJECTED, MSG, PM_REMOVE, QS_ALLINPUT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
    };

    /// `dwExtraInfo` of the keys we send ourselves ("VRSL").
    const TAG: usize = 0x5652_534C;
    /// Unassigned virtual key (AutoHotkey's suggestion for a menu mask).
    const MASK_VK: u16 = 0xE8;
    const REINSTALL_MS: u32 = 30_000;

    struct Running {
        thread_id: u32,
        thread: JoinHandle<()>,
    }

    fn running() -> &'static Mutex<Option<Running>> {
        static R: std::sync::OnceLock<Mutex<Option<Running>>> = std::sync::OnceLock::new();
        R.get_or_init(|| Mutex::new(None))
    }

    /// Start the hook thread when `need`, stop it when not.
    pub fn run(need: bool) {
        let mut slot = running().lock().unwrap();
        if slot.as_ref().is_some_and(|r| r.thread.is_finished()) {
            *slot = None;
        }
        match (need, slot.is_some()) {
            (true, false) => {
                let (tx, rx) = mpsc::channel();
                let spawned = std::thread::Builder::new().name("versailles-keys".into()).spawn(move || {
                    let _ = tx.send(unsafe { GetCurrentThreadId() });
                    hook_thread();
                });
                match spawned {
                    Ok(thread) => {
                        if let Ok(thread_id) = rx.recv() {
                            *slot = Some(Running { thread_id, thread });
                        }
                    }
                    Err(err) => tracing::warn!("keyboard hook thread failed to start: {err}"),
                }
            }
            (false, true) => {
                let r = slot.take().unwrap();
                unsafe {
                    let _ = PostThreadMessageW(r.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
                }
                let _ = r.thread.join();
                tracing::info!("keyboard hook removed");
            }
            _ => {}
        }
    }

    fn install() -> Option<HHOOK> {
        unsafe {
            let module = GetModuleHandleW(None).ok();
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), module.map(Into::into), 0) {
                Ok(h) => Some(h),
                Err(err) => {
                    tracing::warn!("keyboard hook failed: {err}");
                    None
                }
            }
        }
    }

    fn hook_thread() {
        let mut hook = install();
        if hook.is_some() {
            tracing::info!("keyboard hook installed");
        }
        unsafe {
            'outer: loop {
                if MsgWaitForMultipleObjects(None, false, REINSTALL_MS, QS_ALLINPUT) == WAIT_TIMEOUT {
                    // Windows removes a hook that once ran past its timeout
                    // without telling us; a fresh one costs nothing.
                    if let Some(h) = hook.take() {
                        let _ = UnhookWindowsHookEx(h);
                    }
                    hook = install();
                    continue;
                }
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'outer;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if let Some(h) = hook {
                let _ = UnhookWindowsHookEx(h);
            }
        }
    }

    struct State {
        /// Win is down (set on its first down, not on auto-repeat).
        win_held: Cell<bool>,
        /// Another key went down while Win was held: not a tap.
        win_combo: Cell<bool>,
        /// A swallowed Win combo: mask the Win key-up.
        mask: Cell<bool>,
        /// Keys whose down we swallowed; their up (and repeats) go too.
        swallowed: RefCell<[bool; 256]>,
        /// Shift / Ctrl / Alt as seen by this hook (bits 1 / 2 / 4). The
        /// async key state can lag behind an event that just went past.
        mods: Cell<u8>,
    }

    thread_local! {
        static STATE: State = State {
            win_held: Cell::new(false),
            win_combo: Cell::new(false),
            mask: Cell::new(false),
            swallowed: RefCell::new([false; 256]),
            mods: Cell::new(0),
        };
    }

    fn held(vk: VIRTUAL_KEY) -> bool {
        (unsafe { GetAsyncKeyState(vk.0 as i32) }) < 0
    }

    /// Shift / Ctrl / Alt (generic, left, right) as a `mods` bit.
    fn modifier_bit(vk: u16) -> u8 {
        match vk {
            0x10 | 0xA0 | 0xA1 => 1,
            0x11 | 0xA2 | 0xA3 => 2,
            0x12 | 0xA4 | 0xA5 => 4,
            _ => 0,
        }
    }

    fn send_mask() {
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: VIRTUAL_KEY(MASK_VK), dwFlags: flags, dwExtraInfo: TAG, ..Default::default() },
            },
        };
        let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// `true`: swallow this event.
    fn handle(vk: u16, down: bool) -> bool {
        STATE.with(|s| {
            let bit = modifier_bit(vk);
            if bit != 0 {
                s.mods.set(if down { s.mods.get() | bit } else { s.mods.get() & !bit });
                return false;
            }
            let shift = s.mods.get() & 1 != 0 || held(VK_SHIFT);
            let ctrl = s.mods.get() & 2 != 0 || held(VK_CONTROL);
            let alt = s.mods.get() & 4 != 0 || held(VK_MENU);
            let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;
            if is_win {
                if down {
                    if !s.win_held.get() {
                        s.win_held.set(true);
                        s.win_combo.set(false);
                        s.mask.set(false);
                    }
                    return false;
                }
                s.win_held.set(false);
                let mut mask = s.mask.get();
                if !s.win_combo.get() {
                    let tap = Combo {
                        ctrl,
                        alt,
                        shift,
                        win: true,
                        key: Key::WinTap,
                    };
                    if let Some(entry) = table().read().unwrap().lookup(&tap) {
                        if let Some(piece) = &entry.piece {
                            fire(piece);
                        }
                        mask = true;
                    }
                }
                if mask {
                    send_mask();
                }
                // Let the Win key-up through: swallowing it would leave Win stuck.
                return false;
            }
            let i = vk as usize & 0xFF;
            if !down {
                return std::mem::replace(&mut s.swallowed.borrow_mut()[i], false);
            }
            if s.swallowed.borrow()[i] {
                return true; // auto-repeat of a swallowed key
            }
            if s.win_held.get() {
                s.win_combo.set(true);
            }
            let combo = Combo {
                ctrl,
                alt,
                shift,
                win: s.win_held.get() || held(VK_LWIN) || held(VK_RWIN),
                key: Key::Vk(vk),
            };
            let t = table().read().unwrap();
            let Some(entry) = t.lookup(&combo) else {
                return false;
            };
            s.swallowed.borrow_mut()[i] = true;
            if combo.win {
                s.mask.set(true);
            }
            if let Some(piece) = &entry.piece {
                fire(piece);
            }
            true
        })
    }

    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let ours = k.flags.0 & LLKHF_INJECTED.0 != 0 && k.dwExtraInfo == TAG;
            let msg = wparam.0 as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            if !ours && handle(k.vkCode as u16, down) {
                return LRESULT(1);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

#[cfg(not(windows))]
mod hook {
    pub fn run(_need: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_win_keys() {
        let tap = parse("Win").unwrap();
        assert!(tap.win && tap.key == Key::WinTap && !tap.shift);
        let copilot = parse("Copilot").unwrap();
        assert!(copilot.win && copilot.shift && copilot.key == Key::Vk(VK_F23));
        assert_eq!(parse("super+a"), Some(Combo { ctrl: false, alt: false, shift: false, win: true, key: Key::Vk(0x41) }));
        assert_eq!(parse("Ctrl+Win+F1").unwrap().key, Key::Vk(0x70));
        assert_eq!(parse("Win+F24").unwrap().key, Key::Vk(0x87));
        assert!(parse("Win+Nope").is_none());
        assert!(parse("Ctrl").is_none());
        assert!(parse("A+B").is_none());
    }

    #[test]
    fn only_win_combos_use_the_hook() {
        assert!(wants_hook("Win"));
        assert!(wants_hook("Copilot"));
        assert!(wants_hook("Super+Space"));
        assert!(!wants_hook("Alt+Space"));
        assert!(!wants_hook("Ctrl+Shift+K"));
    }
}
