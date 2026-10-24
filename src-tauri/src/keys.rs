//! Keys Windows keeps for itself (#31): `data-hotkey="Win"`, `"Copilot"`,
//! `"Win+A"`, and `"shell": { "suppress": ["Win+X", ...] }` in `#versailles`.
//!
//! RegisterHotKey (the global-shortcut plugin) cannot bind a bare Win key and
//! fails on Win+ combos the shell already owns. Anything with Win goes through
//! a low-level keyboard hook instead, the way PowerToys and AutoHotkey do it:
//!
//! - Win's down is held back, so the shell only sees a Win press it needs:
//!   a bound tap or combo is swallowed whole (Start has nothing to open); an
//!   unbound tap is replayed whole (Start opens as usual); any other key
//!   pressed with Win replays the Win down and then that key, so Windows'
//!   own Win+E, Win+D... keep working;
//! - mask keys sent before the Win key-up (AutoHotkey's `vkE8`, ChefKeys'
//!   Left Alt) did not keep Start closed on Windows 11 25H2, hence the above;
//! - replayed keys are tagged and go out from a small sender thread; until
//!   it has sent them, later keys queue behind them (a key-up must not
//!   overtake its replayed key-down, or the key stays down);
//! - the Copilot key sends LShift+LWin+F23, so `"Copilot"` is Win+Shift+F23.
//!
//! The hook runs on its own thread with its own message loop. Windows passes
//! a key on when the hook answers later than `LowLevelHooksTimeout` (and may
//! drop the hook), so the callback does no I/O, takes no lock, and does not
//! allocate: it reads a table the thread keeps for itself, hands toggles and
//! replays to the sender thread, and notes each key in a fixed-size queue.
//! The thread runs at time-critical priority and is exempt from power
//! throttling (#37).
//!
//! A watch thread drains that queue: it logs keys that reached the hook late
//! (and, with `VERSAILLES_KEYS_DEBUG=1`, every key), and witnesses Win and
//! Copilot presses through raw input: one the hook did not see within 150 ms
//! is logged as "missed by hook" and the hook is installed again. The hook
//! is also installed again after session unlock, resume, display on, and 30 s
//! without key traffic.
//!
//! All of this runs in a helper process, `versailles --keys-hook`, that owns
//! no windows: while one of Versailles' own windows had the focus (the
//! desktop, an overlay, a bar), Windows did not call a low-level hook first
//! in line in this process, nor any hook behind it, and the Win press went
//! to the shell (Start opened instead of the bound piece, #37). A hook in
//! another process is called as usual. The app sends the bindings to the
//! helper's stdin as JSON lines; the helper answers on stdout with toggles
//! and log lines, and exits when its stdin closes (the app quit or crashed).
//!
//! It exists only while a Win binding or a suppress entry exists, and dies
//! with the app, so the keyboard is never left changed. Win+L and
//! Ctrl+Alt+Del cannot be intercepted, and keys typed into an elevated window
//! are not seen (UIPI).

#![cfg_attr(not(windows), allow(dead_code))]

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::AppHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    /// The Win key pressed and released alone.
    WinTap,
    Vk(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    combo: Combo,
    piece: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Table {
    binds: Vec<Entry>,
    suppress: Vec<Entry>,
}

impl Table {
    /// Index of the entry for `c`: bindings first, then suppress entries.
    fn find(&self, c: &Combo) -> Option<usize> {
        self.binds.iter().chain(self.suppress.iter()).position(|e| e.combo == *c)
    }
    fn entry(&self, index: usize) -> Option<&Entry> {
        self.binds.iter().chain(self.suppress.iter()).nth(index)
    }
    fn is_empty(&self) -> bool {
        self.binds.is_empty() && self.suppress.is_empty()
    }
}

/// The bindings and their generation. The hook thread keeps a copy of its
/// own (refreshed by a message when this changes), so a key never waits for
/// this lock.
fn table() -> &'static Mutex<(u64, Arc<Table>)> {
    static TABLE: OnceLock<Mutex<(u64, Arc<Table>)>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new((0, Arc::new(Table::default()))))
}

fn app_slot() -> &'static OnceLock<AppHandle> {
    static APP: OnceLock<AppHandle> = OnceLock::new();
    &APP
}

/// Set the hook's bindings (`None` keeps the current ones) and suppress list,
/// installing or removing the hook as needed.
pub fn apply(app: &AppHandle, binds: Option<Vec<(String, String)>>, suppress: &[String]) {
    let _ = app_slot().set(app.clone());
    let (generation, current) = {
        let mut slot = table().lock().unwrap();
        let binds = match binds {
            Some(binds) => binds
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
                .collect(),
            None => slot.1.binds.clone(),
        };
        let suppress = suppress
            .iter()
            .filter_map(|accel| match parse(accel) {
                Some(combo) => Some(Entry { combo, piece: None }),
                None => {
                    tracing::warn!("shell.suppress: unknown key '{accel}'");
                    None
                }
            })
            .collect();
        *slot = (slot.0 + 1, Arc::new(Table { binds, suppress }));
        (slot.0, slot.1.clone())
    };
    client::run(!current.is_empty(), generation, &current);
}

/// Toggle the piece on the main thread.
fn fire(piece: &str) {
    if let Some(app) = app_slot().get() {
        let handle = app.clone();
        let id = piece.to_string();
        let _ = app.run_on_main_thread(move || crate::window_manager::toggle_hotkey_piece(&handle, &id));
    }
}

/// Toggle the piece of entry `index` of table `generation` (the one the hook
/// matched against); nothing when the bindings changed since.
fn fire_entry(generation: u64, index: usize) {
    if HELPER.load(Ordering::Relaxed) {
        // In the helper: the app toggles it.
        #[cfg(windows)]
        helper::note(&Note::Fire { generation, index });
        return;
    }
    let piece = {
        let slot = table().lock().unwrap();
        if slot.0 != generation {
            return;
        }
        slot.1.entry(index).and_then(|e| e.piece.clone())
    };
    if let Some(piece) = piece {
        fire(&piece);
    }
}

/// This process is the `--keys-hook` helper.
static HELPER: AtomicBool = AtomicBool::new(false);

/// App → helper: the bindings of one generation.
#[derive(Serialize, Deserialize)]
struct Bindings {
    generation: u64,
    table: Table,
}

/// Helper → app.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Note {
    /// Toggle the piece of this table entry.
    Fire { generation: u64, index: usize },
    /// A log line for the app's log.
    Log { level: String, text: String },
}

/// `versailles --keys-hook`: run as the helper until stdin closes.
pub fn helper_main() -> i32 {
    #[cfg(windows)]
    {
        helper::main()
    }
    #[cfg(not(windows))]
    {
        2
    }
}

/// A fixed-size queue for one producer thread and one consumer thread.
/// `push` and `pop` never allocate, lock or wait, so the hook callback can
/// use it. A push to a full queue is dropped and counted.
struct Ring<T: Copy, const N: usize> {
    slots: [UnsafeCell<MaybeUninit<T>>; N],
    /// Next slot to read; only the consumer moves it.
    head: AtomicUsize,
    /// Next slot to write; only the producer moves it.
    tail: AtomicUsize,
    dropped: AtomicU32,
}

// Slots are only written by the producer before it publishes them (`tail`)
// and only read by the consumer before it frees them (`head`).
unsafe impl<T: Copy + Send, const N: usize> Sync for Ring<T, N> {}

impl<T: Copy, const N: usize> Ring<T, N> {
    const fn new() -> Self {
        Self {
            slots: [const { UnsafeCell::new(MaybeUninit::uninit()) }; N],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            dropped: AtomicU32::new(0),
        }
    }

    /// Producer only. `false`: full, `value` was dropped.
    fn push(&self, value: T) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail.wrapping_sub(self.head.load(Ordering::Acquire)) >= N {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        unsafe { (*self.slots[tail % N].get()).write(value) };
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer only.
    fn pop(&self) -> Option<T> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let value = unsafe { (*self.slots[head % N].get()).assume_init() };
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    /// Pushes dropped since the last call.
    fn take_dropped(&self) -> u32 {
        self.dropped.swap(0, Ordering::Relaxed)
    }
}

/// What the hook did with a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Decision {
    #[default]
    Passed,
    Swallowed,
    /// One of our own replays: passed on untouched.
    Ours,
}

/// One key event as the hook saw it, for the watch thread to log.
#[derive(Debug, Clone, Copy, Default)]
struct Event {
    /// Tick count (`GetTickCount`) when the callback started.
    at: u32,
    /// How long after the key the callback started (ms).
    late: u32,
    /// How long the callback took to decide (µs).
    took: u32,
    vk: u16,
    scan: u16,
    /// `KBDLLHOOKSTRUCT::flags` (injected, extended...).
    flags: u32,
    down: bool,
    decision: Decision,
}

/// Key events from the hook callback to the watch thread.
static EVENTS: Ring<Event, 256> = Ring::new();

/// A key that reached the hook this late (ms) is logged as slow.
const SLOW_MS: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lateness {
    OnTime,
    Slow,
    /// Past `LowLevelHooksTimeout`: Windows has probably passed the key on.
    TooLate,
}

/// Milliseconds from a key's stamp to `now`, both tick counts. A stamp
/// after `now` (an injected key with a time of its own) counts as 0.
fn late_ms(now: u32, stamp: u32) -> u32 {
    let d = now.wrapping_sub(stamp);
    if d > i32::MAX as u32 {
        0
    } else {
        d
    }
}

fn lateness(late: u32, timeout: u32) -> Lateness {
    if late >= timeout {
        Lateness::TooLate
    } else if late >= SLOW_MS {
        Lateness::Slow
    } else {
        Lateness::OnTime
    }
}

/// The `LowLevelHooksTimeout` Windows applies: the registry value when set,
/// at most 1000 ms (Windows 10 1709 and later), else about 300 ms.
fn hooks_timeout(set: Option<u32>) -> u32 {
    match set {
        Some(ms) if ms > 0 => ms.min(1000),
        _ => 300,
    }
}

/// Keys the raw-input witness checks: LWin, RWin, F23 (the Copilot key).
const WATCHED: [u16; 3] = [0x5B, 0x5C, VK_F23];

fn watched(vk: u16) -> Option<usize> {
    WATCHED.iter().position(|&w| w == vk)
}

/// Stamp (`KBDLLHOOKSTRUCT::time | 1`, 0 = never) of the last down of each
/// `WATCHED` key the hook saw.
static SEEN: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];

/// Raw input and the hook stamp the same press at most this far apart (ms).
const SAME_PRESS_MS: u32 = 50;

/// Whether the hook's last down of a key (`hook_stamp`, 0 = never) is the
/// press raw input saw at `raw_stamp` (or a later one).
fn hook_saw(hook_stamp: u32, raw_stamp: u32) -> bool {
    hook_stamp != 0 && (raw_stamp.wrapping_sub(hook_stamp) as i32) <= SAME_PRESS_MS as i32
}

/// Keys a slow-key warning may name: Win, Shift / Ctrl / Alt and F1-F24.
/// Others are only counted, so the main log never spells typed text.
fn nameable(vk: u16) -> bool {
    matches!(vk, 0x5B | 0x5C | 0x10..=0x12 | 0xA0..=0xA5 | 0x70..=0x87)
}

/// One warning for keys that reached the hook late (`(vk, ms)` each).
fn slow_summary(slow: &[(u16, u32)], timeout: u32) -> Option<String> {
    let max = slow.iter().map(|&(_, ms)| ms).max()?;
    let mut keys: Vec<String> = Vec::new();
    for &(vk, _) in slow.iter().filter(|&&(vk, _)| nameable(vk)) {
        let name = format!("{vk:#04x}");
        if !keys.contains(&name) {
            keys.push(name);
        }
    }
    let others = slow.iter().filter(|&&(vk, _)| !nameable(vk)).count();
    if others > 0 {
        keys.push(format!("{others} other"));
    }
    let mut text = format!(
        "keys: {} key event{} reached the hook late (max {max} ms; {})",
        slow.len(),
        if slow.len() == 1 { "" } else { "s" },
        keys.join(", ")
    );
    if max >= timeout {
        text.push_str(&format!(
            "; Windows passes a key on after {timeout} ms (LowLevelHooksTimeout), so the shell probably got it"
        ));
    }
    Some(text)
}

/// `VERSAILLES_KEYS_DEBUG=1`: log every key event the hook sees to
/// `keys.log` (its own file: the main log may rotate under another
/// Versailles writing to it).
fn debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("VERSAILLES_KEYS_DEBUG").is_ok_and(|v| v == "1"))
}

/// Send key events `(virtual key, key up)` tagged as Versailles' own, so the
/// keyboard hook passes them on untouched (a Win combo it would otherwise
/// hold back or swallow). For Windows' own shortcuts, e.g. Win+Esc.
#[cfg(windows)]
pub fn send_tagged(keys: &[(u16, bool)]) {
    hook::send_tagged(keys)
}

#[cfg(not(windows))]
pub fn send_tagged(_keys: &[(u16, bool)]) {}

#[cfg(windows)]
mod hook {
    use super::{
        debug, fire_entry, hook_saw, hooks_timeout, late_ms, lateness, slow_summary, table, watched, Combo, Decision,
        Event, Key, Lateness, Ring, Table, EVENTS, SEEN,
    };
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex, OnceLock};
    use std::thread::{JoinHandle, Thread};
    use windows::core::{w, BOOL, GUID};
    use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WAIT_TIMEOUT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Power::{
        RegisterPowerSettingNotification, RegisterSuspendResumeNotification, UnregisterPowerSettingNotification,
        UnregisterSuspendResumeNotification, POWERBROADCAST_SETTING,
    };
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, GetCurrentThreadId, ProcessPowerThrottling, SetProcessInformation,
        SetThreadInformation, SetThreadPriority, ThreadPowerThrottling, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE, THREAD_POWER_THROTTLING_CURRENT_VERSION,
        THREAD_POWER_THROTTLING_EXECUTION_SPEED, THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_TIME_CRITICAL,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, VIRTUAL_KEY,
        VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows::Win32::UI::Input::{
        GetRawInputData, GetRegisteredRawInputDevices, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
        RAWINPUTHEADER, RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEKEYBOARD,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageTime,
        MsgWaitForMultipleObjects, PeekMessageW, PostThreadMessageW, RegisterClassW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, DEVICE_NOTIFY_WINDOW_HANDLE, HC_ACTION, HHOOK, HWND_MESSAGE,
        KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, MSG, PBT_APMRESUMEAUTOMATIC, PBT_POWERSETTINGCHANGE,
        PM_NOREMOVE, PM_REMOVE, QS_ALLINPUT, RI_KEY_BREAK, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
        WM_INPUT, WM_KEYDOWN, WM_POWERBROADCAST, WM_QUIT, WM_SYSKEYDOWN, WM_WTSSESSION_CHANGE, WNDCLASSW,
        WTS_SESSION_UNLOCK,
    };

    /// `dwExtraInfo` of the keys we send ourselves ("VRSL").
    const TAG: usize = 0x5652_534C;
    /// Re-install after this long without a key reaching the hook: Windows
    /// silently drops a hook whose thread once answered late (a UI Automation
    /// client reading our windows is enough). A 1 s interval did not help
    /// with missed presses (#37).
    const REINSTALL_MS: u32 = 30_000;
    /// Raw input saw a Win / Copilot press: the hook should have seen it
    /// within this many ms.
    const WITNESS_MS: u32 = 150;
    /// At most one re-install per this many ms for missed presses (two
    /// hooking apps could otherwise take turns pushing each other back).
    const MISSED_REINSTALL_MS: u32 = 5_000;
    /// The watch thread drains the key queue this often (ms).
    const DRAIN_MS: u32 = 250;
    /// And checks that raw input still comes to it this often.
    const RECLAIM_MS: u32 = 10_000;

    /// Thread messages to the hook thread.
    const WM_REINSTALL: u32 = WM_APP + 1; // wParam: Reason
    const WM_TABLE: u32 = WM_APP + 2;

    #[derive(Clone, Copy)]
    enum Reason {
        Missed = 1,
        Unlock,
        Resume,
        DisplayOn,
    }

    fn reason_text(code: usize) -> &'static str {
        match code {
            1 => "a press missed the hook",
            2 => "session unlocked",
            3 => "resumed",
            4 => "display on",
            _ => "requested",
        }
    }

    /// The monitor's on/off/dimmed state (`GUID_CONSOLE_DISPLAY_STATE`).
    const GUID_CONSOLE_DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);

    // wtsapi32 directly: not worth the large Win32_System_RemoteDesktop feature.
    #[link(name = "wtsapi32", kind = "raw-dylib")]
    extern "system" {
        fn WTSRegisterSessionNotification(hwnd: HWND, flags: u32) -> BOOL;
        fn WTSUnRegisterSessionNotification(hwnd: HWND) -> BOOL;
    }
    const NOTIFY_FOR_THIS_SESSION: u32 = 0;

    struct Running {
        thread_id: u32,
        thread: JoinHandle<()>,
        watch_id: u32,
        watch: Option<JoinHandle<()>>,
    }

    fn running() -> &'static Mutex<Option<Running>> {
        static R: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
        R.get_or_init(|| Mutex::new(None))
    }

    /// Start the hook when `need`, stop it when not; a running hook picks up
    /// the new table.
    pub fn run(need: bool) {
        let mut slot = running().lock().unwrap();
        if slot.as_ref().is_some_and(|r| r.thread.is_finished()) {
            stop(slot.take().unwrap());
        }
        match (need, slot.as_ref()) {
            (true, None) => *slot = start(),
            (true, Some(r)) => post(r.thread_id, WM_TABLE, 0),
            (false, Some(_)) => {
                stop(slot.take().unwrap());
                tracing::info!("keyboard hook removed");
            }
            (false, None) => {}
        }
    }

    fn start() -> Option<Running> {
        // The sender must exist before the first key needs it.
        sender();
        process_throttling(false);
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("versailles-keys".into()).spawn(move || {
            create_queue();
            let _ = tx.send(unsafe { GetCurrentThreadId() });
            hook_thread();
        });
        let thread = match spawned {
            Ok(thread) => thread,
            Err(err) => {
                tracing::warn!("keyboard hook thread failed to start: {err}");
                process_throttling(true);
                return None;
            }
        };
        let Ok(thread_id) = rx.recv() else {
            let _ = thread.join();
            process_throttling(true);
            return None;
        };
        let (tx, rx) = mpsc::channel();
        let watch = std::thread::Builder::new()
            .name("versailles-keys-watch".into())
            .spawn(move || {
                create_queue();
                let _ = tx.send(unsafe { GetCurrentThreadId() });
                watch_thread(thread_id);
            })
            .map_err(|err| tracing::warn!("keyboard watch thread failed to start: {err}"))
            .ok();
        let watch_id = if watch.is_some() { rx.recv().unwrap_or(0) } else { 0 };
        Some(Running { thread_id, thread, watch_id, watch })
    }

    fn stop(r: Running) {
        post(r.thread_id, WM_QUIT, 0);
        post(r.watch_id, WM_QUIT, 0);
        let _ = r.thread.join();
        if let Some(watch) = r.watch {
            let _ = watch.join();
        }
        process_throttling(true);
    }

    fn post(thread: u32, msg: u32, wparam: usize) {
        if thread != 0 {
            unsafe {
                let _ = PostThreadMessageW(thread, msg, WPARAM(wparam), LPARAM(0));
            }
        }
    }

    /// Give this thread its message queue now, so messages posted to it
    /// right after it reports its id are not lost.
    fn create_queue() {
        let mut msg = MSG::default();
        unsafe {
            let _ = PeekMessageW(&mut msg, None, WM_APP, WM_APP, PM_NOREMOVE);
        }
    }

    /// While the hook runs, Windows must not run the process slowly
    /// (EcoQoS): its windows are mostly hidden, which on battery is reason
    /// enough. `allow`: hand the decision back to Windows.
    fn process_throttling(allow: bool) {
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: if allow { 0 } else { PROCESS_POWER_THROTTLING_EXECUTION_SPEED },
            StateMask: 0,
        };
        unsafe {
            if let Err(err) = SetProcessInformation(
                GetCurrentProcess(),
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of_val(&state) as u32,
            ) {
                tracing::warn!("keys: power throttling setting failed: {err}");
            }
        }
    }

    /// The hook thread answers first: time-critical priority (as AutoHotkey's
    /// hook thread) and never throttled.
    fn harden() {
        let state = THREAD_POWER_THROTTLING_STATE {
            Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: 0,
        };
        unsafe {
            if let Err(err) = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) {
                tracing::warn!("keys: hook thread priority failed: {err}");
            }
            if let Err(err) = SetThreadInformation(
                GetCurrentThread(),
                ThreadPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of_val(&state) as u32,
            ) {
                tracing::warn!("keys: hook thread throttling failed: {err}");
            }
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

    /// New hook first, then drop the old one: no moment without a hook, and
    /// calls queued meanwhile reach only the new one.
    fn reinstall(hook: &mut Option<HHOOK>) -> bool {
        let Some(fresh) = install() else {
            return false;
        };
        if let Some(h) = hook.replace(fresh) {
            unsafe {
                let _ = UnhookWindowsHookEx(h);
            }
        }
        reset_if_released();
        true
    }

    fn hook_thread() {
        harden();
        load_table();
        let mut hook = install();
        if hook.is_some() {
            tracing::info!("keyboard hook installed");
        }
        unsafe {
            'outer: loop {
                if MsgWaitForMultipleObjects(None, false, REINSTALL_MS, QS_ALLINPUT) == WAIT_TIMEOUT {
                    reinstall(&mut hook);
                    continue;
                }
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    match msg.message {
                        WM_QUIT => break 'outer,
                        WM_TABLE => load_table(),
                        WM_REINSTALL => {
                            if reinstall(&mut hook) {
                                tracing::info!("keyboard hook installed again: {}", reason_text(msg.wParam.0));
                            }
                        }
                        _ => {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }
            }
            if let Some(h) = hook {
                let _ = UnhookWindowsHookEx(h);
            }
        }
    }

    /// One thing the sender thread does for the hook.
    #[derive(Clone, Copy)]
    enum Cmd {
        /// Toggle the piece of this table entry.
        Fire { generation: u64, index: usize },
        /// Send these two key events.
        Replay([Replay; 2]),
        /// Send this key event (one that came in behind a replay).
        Key(Replay),
    }

    impl Cmd {
        fn sends_keys(&self) -> bool {
            !matches!(self, Cmd::Fire { .. })
        }
    }

    static CMDS: Ring<Cmd, 64> = Ring::new();
    /// Key-sending commands handed to the sender and not sent yet.
    static PENDING: AtomicUsize = AtomicUsize::new(0);

    /// The sender thread: replays keys and toggles pieces for the hook, in
    /// order. `None` when it could not start.
    fn sender() -> Option<&'static Thread> {
        static SENDER: OnceLock<Option<Thread>> = OnceLock::new();
        SENDER
            .get_or_init(|| {
                std::thread::Builder::new()
                    .name("versailles-keys-send".into())
                    .spawn(|| loop {
                        while let Some(cmd) = CMDS.pop() {
                            run_cmd(cmd);
                            if cmd.sends_keys() {
                                PENDING.fetch_sub(1, Ordering::SeqCst);
                            }
                        }
                        std::thread::park();
                    })
                    .map_err(|err| tracing::warn!("keyboard sender thread failed to start: {err}"))
                    .ok()
                    .map(|h| h.thread().clone())
            })
            .as_ref()
    }

    /// From the hook: hand `cmd` to the sender (run it here only if there is
    /// none or its queue is full).
    fn command(cmd: Cmd) {
        if let Some(thread) = sender() {
            let keys = cmd.sends_keys();
            if keys {
                PENDING.fetch_add(1, Ordering::SeqCst);
            }
            if CMDS.push(cmd) {
                thread.unpark();
                return;
            }
            if keys {
                PENDING.fetch_sub(1, Ordering::SeqCst);
            }
        }
        run_cmd(cmd);
    }

    fn run_cmd(cmd: Cmd) {
        match cmd {
            Cmd::Fire { generation, index } => fire_entry(generation, index),
            Cmd::Replay(keys) => send(&keys),
            Cmd::Key(key) => send(&[key]),
        }
    }

    struct State {
        /// Win is physically down (set on its first down, not on auto-repeat).
        win_held: Cell<bool>,
        /// Which Win key (LWin / RWin) is down.
        win_vk: Cell<u16>,
        /// We replayed the Win key-down to the shell for this press (an
        /// unbound Win combo such as Win+E): the rest of it passes through.
        win_sent: Cell<bool>,
        /// Another key went down while Win was held: not a tap.
        win_combo: Cell<bool>,
        /// Keys whose down we swallowed; their up (and repeats) go too.
        swallowed: RefCell<[bool; 256]>,
        /// Shift / Ctrl / Alt as seen by this hook (bits 1 / 2 / 4). The
        /// async key state can lag behind an event that just went past.
        mods: Cell<u8>,
        /// This thread's copy of the bindings and their generation.
        table: RefCell<(u64, Arc<Table>)>,
    }

    thread_local! {
        static STATE: State = State {
            win_held: Cell::new(false),
            win_vk: Cell::new(VK_LWIN.0),
            win_sent: Cell::new(false),
            win_combo: Cell::new(false),
            swallowed: RefCell::new([false; 256]),
            mods: Cell::new(0),
            table: RefCell::new((0, Arc::new(Table::default()))),
        };
    }

    /// Copy the current bindings for the callback (outside it: this locks).
    fn load_table() {
        let current = table().lock().unwrap().clone();
        STATE.with(|s| *s.table.borrow_mut() = current);
    }

    /// A dropped hook may have missed a key-up; forget keys no longer down.
    fn reset_if_released() {
        STATE.with(|s| {
            // While we hold Win back, its async state reads "up"; only a
            // replayed Win can be checked that way.
            if s.win_held.get() && s.win_sent.get() && !held(VK_LWIN) && !held(VK_RWIN) {
                s.win_held.set(false);
            }
            let mut swallowed = s.swallowed.borrow_mut();
            for (vk, on) in swallowed.iter_mut().enumerate() {
                if *on && !held(VIRTUAL_KEY(vk as u16)) {
                    *on = false;
                }
            }
            if !held(VK_SHIFT) && !held(VK_CONTROL) && !held(VK_MENU) {
                s.mods.set(0);
            }
        });
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

    /// One key event to replay: virtual key, scan code, `KEYEVENTF_*` flags.
    type Replay = (u16, u16, u32);

    fn send(keys: &[Replay]) {
        let inputs: Vec<INPUT> = keys
            .iter()
            .map(|&(vk, scan, flags)| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: scan,
                        dwFlags: KEYBD_EVENT_FLAGS(flags),
                        dwExtraInfo: TAG,
                        ..Default::default()
                    },
                },
            })
            .collect();
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    const EXTENDED: u32 = 0x1; // KEYEVENTF_EXTENDEDKEY
    const KEYUP: u32 = 0x2; // KEYEVENTF_KEYUP

    /// See `keys::send_tagged`. The Win keys are extended keys.
    pub fn send_tagged(keys: &[(u16, bool)]) {
        let events: Vec<Replay> = keys
            .iter()
            .map(|&(vk, up)| {
                let ext = if vk == VK_LWIN.0 || vk == VK_RWIN.0 { EXTENDED } else { 0 };
                (vk, 0, ext | if up { KEYUP } else { 0 })
            })
            .collect();
        send(&events);
    }

    /// `true`: swallow this event. `scan` / `extended` describe it for a replay.
    ///
    /// The shell never sees a Win press unless it needs it: Win's down is held
    /// back. A tap runs its binding (nothing reached the shell, so Start has
    /// nothing to open) or, unbound, is replayed whole so Start opens as
    /// usual. A bound combo is swallowed with Win. Any other key pressed with
    /// Win replays the Win down first, then that key, so Windows' own Win+E,
    /// Win+D... keep working. Shift / Ctrl / Alt pass through untouched.
    fn handle(vk: u16, down: bool, scan: u16, extended: bool) -> bool {
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
                        s.win_vk.set(vk);
                        s.win_sent.set(false);
                        s.win_combo.set(false);
                    }
                    // Hold it back (auto-repeat too) until we know what it is.
                    return !s.win_sent.get();
                }
                if !s.win_held.get() {
                    return false; // pressed before the hook saw it
                }
                s.win_held.set(false);
                if s.win_sent.get() {
                    return false; // the shell has its Win down: let it go up
                }
                if !s.win_combo.get() {
                    let tap = Combo { ctrl, alt, shift, win: true, key: Key::WinTap };
                    let table = s.table.borrow();
                    match table.1.find(&tap) {
                        Some(index) => {
                            if table.1.entry(index).is_some_and(|e| e.piece.is_some()) {
                                command(Cmd::Fire { generation: table.0, index });
                            } // else suppressed
                        }
                        // Unbound tap: give the shell its Win press (Start).
                        None => command(Cmd::Replay([(vk, 0, EXTENDED), (vk, 0, EXTENDED | KEYUP)])),
                    }
                }
                return true;
            }
            let i = vk as usize & 0xFF;
            if !down {
                return std::mem::replace(&mut s.swallowed.borrow_mut()[i], false);
            }
            if s.swallowed.borrow()[i] {
                return true; // auto-repeat of a swallowed key
            }
            let win = s.win_held.get();
            if win {
                s.win_combo.set(true);
            }
            let combo = Combo { ctrl, alt, shift, win, key: Key::Vk(vk) };
            let table = s.table.borrow();
            if let Some(index) = table.1.find(&combo) {
                s.swallowed.borrow_mut()[i] = true;
                if table.1.entry(index).is_some_and(|e| e.piece.is_some()) {
                    command(Cmd::Fire { generation: table.0, index });
                }
                return true;
            }
            if win && !s.win_sent.get() {
                // An unbound Win combo: replay Win down, then this key.
                s.win_sent.set(true);
                let flags = if extended { EXTENDED } else { 0 };
                command(Cmd::Replay([(s.win_vk.get(), 0, EXTENDED), (vk, scan, flags)]));
                return true;
            }
            false
        })
    }

    /// The hook callback. No I/O, locks or allocation here: past
    /// `LowLevelHooksTimeout` Windows passes the key on without us.
    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let at = GetTickCount();
            let t0 = std::time::Instant::now();
            let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let vk = k.vkCode as u16;
            let ours = k.flags.0 & LLKHF_INJECTED.0 != 0 && k.dwExtraInfo == TAG;
            let msg = wparam.0 as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let extended = k.flags.0 & LLKHF_EXTENDED.0 != 0;
            if down {
                if let Some(i) = watched(vk) {
                    SEEN[i].store(k.time | 1, Ordering::Relaxed);
                }
            }
            let mut swallow = !ours && handle(vk, down, k.scanCode as u16, extended);
            if !ours && !swallow && PENDING.load(Ordering::SeqCst) > 0 {
                // Our replays have not gone out yet: this key goes after them.
                let flags = if extended { EXTENDED } else { 0 } | if down { 0 } else { KEYUP };
                command(Cmd::Key((vk, k.scanCode as u16, flags)));
                swallow = true;
            }
            EVENTS.push(Event {
                at,
                late: late_ms(at, k.time),
                took: t0.elapsed().as_micros().min(u32::MAX as u128) as u32,
                vk,
                scan: k.scanCode as u16,
                flags: k.flags.0,
                down,
                decision: if ours {
                    Decision::Ours
                } else if swallow {
                    Decision::Swallowed
                } else {
                    Decision::Passed
                },
            });
            if swallow {
                return LRESULT(1);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// `LowLevelHooksTimeout` from the registry (a DWORD, or the string some
    /// guides set).
    pub(super) fn timeout_setting() -> Option<u32> {
        unsafe {
            let mut value = 0u32;
            let mut size = std::mem::size_of::<u32>() as u32;
            let status = RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Control Panel\\Desktop"),
                w!("LowLevelHooksTimeout"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as *mut _),
                Some(&mut size),
            );
            if status.is_ok() {
                return Some(value);
            }
            let mut text = [0u16; 16];
            let mut size = std::mem::size_of_val(&text) as u32;
            let status = RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Control Panel\\Desktop"),
                w!("LowLevelHooksTimeout"),
                RRF_RT_REG_SZ,
                None,
                Some(text.as_mut_ptr() as *mut _),
                Some(&mut size),
            );
            if !status.is_ok() {
                return None;
            }
            let len = text.iter().position(|&c| c == 0).unwrap_or(text.len());
            String::from_utf16_lossy(&text[..len]).trim().parse().ok()
        }
    }

    /// A raw-input press of a watched key, to check against the hook.
    #[derive(Clone, Copy)]
    struct Pending {
        stamp: u32,
        due: u32,
        injected: bool,
    }

    #[derive(Default)]
    struct Watch {
        hook_thread: u32,
        /// Watched keys raw input has down (to skip auto-repeat).
        down: [bool; 3],
        pending: [Option<Pending>; 3],
        /// Tick of the last re-install for a missed press (0 = none).
        missed_reinstall: u32,
        /// Last display state (0 off, 1 on, 2 dimmed).
        display: Option<u32>,
    }

    thread_local! {
        static WATCH: RefCell<Watch> = RefCell::new(Watch::default());
    }

    /// The watch thread's message-only window and what it registered.
    struct Watcher {
        hwnd: HWND,
        /// Who had keyboard raw input before us (the event loop's window):
        /// given back when we stop.
        previous: Option<RAWINPUTDEVICE>,
        display: Option<windows::Win32::System::Power::HPOWERNOTIFY>,
        resume: Option<windows::Win32::System::Power::HPOWERNOTIFY>,
        session: bool,
    }

    fn watch_thread(hook_thread: u32) {
        let timeout = hooks_timeout(timeout_setting());
        tracing::info!("keys: hook watch on (LowLevelHooksTimeout {timeout} ms)");
        WATCH.with(|w| w.borrow_mut().hook_thread = hook_thread);
        let watcher = watcher();
        let mut reclaimed = unsafe { GetTickCount() };
        unsafe {
            'outer: loop {
                MsgWaitForMultipleObjects(None, false, next_wait(), QS_ALLINPUT);
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'outer;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                check_pending();
                drain(timeout);
                if let Some(w) = &watcher {
                    if GetTickCount().wrapping_sub(reclaimed) >= RECLAIM_MS {
                        reclaimed = GetTickCount();
                        reclaim_raw_input(w.hwnd);
                    }
                }
            }
        }
        drain(timeout);
        if let Some(w) = watcher {
            close_watcher(w);
        }
    }

    /// How long to wait for messages: until the next drain or witness check.
    fn next_wait() -> u32 {
        let now = unsafe { GetTickCount() };
        WATCH.with(|w| {
            w.borrow()
                .pending
                .iter()
                .flatten()
                .map(|p| (p.due.wrapping_sub(now) as i32).max(0) as u32)
                .fold(DRAIN_MS, u32::min)
        })
    }

    fn watcher() -> Option<Watcher> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
            // Registering again (a second start) fails harmlessly.
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(watch_proc),
                hInstance: instance,
                lpszClassName: w!("VersaillesKeysWatch"),
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("VersaillesKeysWatch"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
            .map_err(|err| tracing::warn!("keys: watch window failed: {err}"))
            .ok()?;
            let previous = registered_keyboard();
            let sink = RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
            if let Err(err) = RegisterRawInputDevices(&[sink], std::mem::size_of::<RAWINPUTDEVICE>() as u32) {
                tracing::warn!("keys: raw input failed, missed presses go unnoticed: {err}");
            }
            let session = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).as_bool();
            if !session {
                tracing::warn!("keys: session notifications failed");
            }
            let display =
                RegisterPowerSettingNotification(HANDLE(hwnd.0), &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE)
                    .map_err(|err| tracing::warn!("keys: display notifications failed: {err}"))
                    .ok();
            let resume = RegisterSuspendResumeNotification(HANDLE(hwnd.0), DEVICE_NOTIFY_WINDOW_HANDLE)
                .map_err(|err| tracing::warn!("keys: resume notifications failed: {err}"))
                .ok();
            Some(Watcher { hwnd, previous, display, resume, session })
        }
    }

    fn close_watcher(w: Watcher) {
        unsafe {
            if let Some(n) = w.display {
                let _ = UnregisterPowerSettingNotification(n);
            }
            if let Some(n) = w.resume {
                let _ = UnregisterSuspendResumeNotification(n);
            }
            if w.session {
                let _ = WTSUnRegisterSessionNotification(w.hwnd);
            }
            // Give keyboard raw input back to whoever had it, or end ours.
            let back = match w.previous {
                Some(p) if p.hwndTarget != w.hwnd => p,
                _ => RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: RIDEV_REMOVE, hwndTarget: HWND::default() },
            };
            let _ = RegisterRawInputDevices(&[back], std::mem::size_of::<RAWINPUTDEVICE>() as u32);
            let _ = DestroyWindow(w.hwnd);
        }
    }

    /// The process' keyboard raw-input registration (one window per process).
    fn registered_keyboard() -> Option<RAWINPUTDEVICE> {
        let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
        let mut count = 0u32;
        unsafe {
            GetRegisteredRawInputDevices(None, &mut count, size);
            if count == 0 {
                return None;
            }
            let mut list = vec![RAWINPUTDEVICE::default(); count as usize];
            let got = GetRegisteredRawInputDevices(Some(list.as_mut_ptr()), &mut count, size);
            if got == u32::MAX {
                return None;
            }
            list.truncate(got as usize);
            list.into_iter().find(|d| d.usUsagePage == 1 && d.usUsage == 6)
        }
    }

    /// The event loop may register keyboard raw input again: take it back.
    fn reclaim_raw_input(hwnd: HWND) {
        if registered_keyboard().is_some_and(|d| d.hwndTarget == hwnd) {
            return;
        }
        let sink = RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
        if unsafe { RegisterRawInputDevices(&[sink], std::mem::size_of::<RAWINPUTDEVICE>() as u32) }.is_ok() {
            tracing::info!("keys: raw input taken back");
        }
    }

    unsafe extern "system" fn watch_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_INPUT => {
                raw_input(HRAWINPUT(l.0 as *mut c_void));
                DefWindowProcW(h, msg, w, l)
            }
            WM_WTSSESSION_CHANGE => {
                if w.0 as u32 == WTS_SESSION_UNLOCK {
                    request(Reason::Unlock);
                }
                LRESULT(0)
            }
            WM_POWERBROADCAST => {
                match w.0 as u32 {
                    PBT_APMRESUMEAUTOMATIC => request(Reason::Resume),
                    PBT_POWERSETTINGCHANGE if l.0 != 0 => {
                        let setting = &*(l.0 as *const POWERBROADCAST_SETTING);
                        if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && setting.DataLength >= 4 {
                            let state = std::ptr::read_unaligned(setting.Data.as_ptr() as *const u32);
                            // The first notice (right after registering) is the current state.
                            let was = WATCH.with(|w| w.borrow_mut().display.replace(state));
                            if state == 1 && was.is_some_and(|was| was != 1) {
                                request(Reason::DisplayOn);
                            }
                        }
                    }
                    _ => {}
                }
                LRESULT(1)
            }
            _ => DefWindowProcW(h, msg, w, l),
        }
    }

    fn request(reason: Reason) {
        let thread = WATCH.with(|w| w.borrow().hook_thread);
        post(thread, WM_REINSTALL, reason as usize);
    }

    /// A Win or Copilot key-down from raw input: check the hook saw it too.
    fn raw_input(handle: HRAWINPUT) {
        let mut raw = RAWINPUT::default();
        let mut size = std::mem::size_of::<RAWINPUT>() as u32;
        let got = unsafe {
            GetRawInputData(
                handle,
                RID_INPUT,
                Some(&mut raw as *mut RAWINPUT as *mut c_void),
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if got == 0 || got == u32::MAX || raw.header.dwType != RIM_TYPEKEYBOARD.0 {
            return;
        }
        let kb = unsafe { raw.data.keyboard };
        let Some(i) = watched(kb.VKey) else {
            return;
        };
        let up = kb.Flags as u32 & RI_KEY_BREAK != 0;
        let stamp = unsafe { GetMessageTime() } as u32;
        let now = unsafe { GetTickCount() };
        WATCH.with(|w| {
            let mut w = w.borrow_mut();
            if up {
                w.down[i] = false;
            } else if !std::mem::replace(&mut w.down[i], true) {
                w.pending[i] = Some(Pending {
                    stamp,
                    due: now.wrapping_add(WITNESS_MS),
                    // Injected keys come from no device.
                    injected: raw.header.hDevice.is_invalid(),
                });
            }
        });
    }

    fn check_pending() {
        let now = unsafe { GetTickCount() };
        let mut missed = [None; 3];
        WATCH.with(|w| {
            let mut w = w.borrow_mut();
            for (i, slot) in w.pending.iter_mut().enumerate() {
                if let Some(p) = *slot {
                    if (now.wrapping_sub(p.due) as i32) >= 0 {
                        *slot = None;
                        if !hook_saw(SEEN[i].load(Ordering::Relaxed), p.stamp) {
                            missed[i] = Some(p.injected);
                        }
                    }
                }
            }
        });
        for (i, injected) in missed.iter().enumerate() {
            let Some(injected) = *injected else {
                continue;
            };
            let app = crate::sensors::read("focus")
                .ok()
                .and_then(|v| v.get("app").and_then(|a| a.as_str()).map(str::to_string))
                .unwrap_or_else(|| "?".into());
            tracing::warn!(
                "keys: missed by hook: {}{} (foreground: {app})",
                ["LWin", "RWin", "Copilot (F23)"][i],
                if injected { ", injected" } else { "" }
            );
            let again = WATCH.with(|w| {
                let mut w = w.borrow_mut();
                let ok = w.missed_reinstall == 0 || now.wrapping_sub(w.missed_reinstall) >= MISSED_REINSTALL_MS;
                if ok {
                    w.missed_reinstall = now | 1;
                }
                ok
            });
            if again {
                request(Reason::Missed);
            }
        }
    }

    /// Log what the hook noted: slow keys always, every key when debugging.
    fn drain(timeout: u32) {
        let debug = debug();
        let mut slow = Vec::new();
        while let Some(e) = EVENTS.pop() {
            if debug {
                log_event(&e);
            }
            if lateness(e.late, timeout) != Lateness::OnTime {
                slow.push((e.vk, e.late));
            }
        }
        let dropped = EVENTS.take_dropped();
        if debug && dropped > 0 {
            crate::crash::append_log("keys.log", &format!("{dropped} key events not recorded (queue full)"));
        }
        if let Some(text) = slow_summary(&slow, timeout) {
            tracing::warn!("{text}");
        }
    }

    fn log_event(e: &Event) {
        let ago = unsafe { GetTickCount() }.wrapping_sub(e.at);
        let when = chrono::Local::now() - chrono::Duration::milliseconds(ago as i64);
        crate::crash::append_log(
            "keys.log",
            &format!(
                "{} key {:#04x} {} scan {:#x} flags {:#x}{} late {} ms in {} us",
                when.format("%H:%M:%S%.3f"),
                e.vk,
                if e.down { "down" } else { "up" },
                e.scan,
                e.flags,
                match e.decision {
                    Decision::Passed => "",
                    Decision::Swallowed => " swallowed",
                    Decision::Ours => " ours",
                },
                e.late,
                e.took
            ),
        );
    }

    #[cfg(test)]
    mod tests {
        /// Read-only: what this machine sets for `LowLevelHooksTimeout`.
        #[test]
        #[ignore]
        fn live_hooks_timeout() {
            let set = super::timeout_setting();
            println!("LowLevelHooksTimeout: {set:?} → {} ms", super::hooks_timeout(set));
        }
    }
}

/// The helper side: bindings in on stdin, notes out on stdout.
#[cfg(windows)]
mod helper {
    use super::{hook, table, Bindings, Note, HELPER};
    use std::io::{BufRead, Write};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    /// One note for the app (stdout is the pipe it reads).
    pub fn note(n: &Note) {
        if let Ok(line) = serde_json::to_string(n) {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        }
    }

    /// Log lines (`tracing` ones too) go to the app's log.
    struct Forward;

    impl log::Log for Forward {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Info
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                note(&Note::Log { level: record.level().to_string(), text: record.args().to_string() });
            }
        }
        fn flush(&self) {}
    }

    pub fn main() -> i32 {
        HELPER.store(true, Ordering::Relaxed);
        if log::set_boxed_logger(Box::new(Forward)).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            match serde_json::from_str::<Bindings>(&line) {
                Ok(b) => {
                    let need = !b.table.is_empty();
                    *table().lock().unwrap() = (b.generation, Arc::new(b.table));
                    hook::run(need);
                }
                Err(err) => tracing::warn!("keys helper: unreadable bindings: {err}"),
            }
        }
        hook::run(false);
        0
    }
}

/// The app side: starts the helper while bindings exist, sends it each new
/// table, toggles what it reports, and starts it again if it ends.
#[cfg(windows)]
mod client {
    use super::{fire_entry, Note, Table};
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// Start a helper that ended on its own again at most this many times.
    const RESTARTS: u32 = 5;

    struct Helper {
        id: u32,
        stdin: ChildStdin,
    }

    fn slot() -> &'static Mutex<Option<Helper>> {
        static S: OnceLock<Mutex<Option<Helper>>> = OnceLock::new();
        S.get_or_init(|| Mutex::new(None))
    }

    /// The bindings last sent, for a helper started again.
    fn last() -> &'static Mutex<Option<String>> {
        static L: OnceLock<Mutex<Option<String>>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(None))
    }

    pub fn run(need: bool, generation: u64, table: &Table) {
        let mut slot = slot().lock().unwrap();
        if !need {
            *last().lock().unwrap() = None;
            // Its stdin closes: it removes the hook and exits.
            slot.take();
            return;
        }
        let line = serde_json::json!({ "generation": generation, "table": table }).to_string();
        *last().lock().unwrap() = Some(line.clone());
        if slot.is_none() {
            *slot = start();
        }
        send(&mut slot, &line);
    }

    fn start() -> Option<Helper> {
        let exe = std::env::current_exe().ok()?;
        let mut child = Command::new(exe)
            .arg("--keys-hook")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|err| tracing::warn!("keyboard hook helper failed to start: {err}"))
            .ok()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let id = child.id();
        std::thread::Builder::new()
            .name("versailles-keys-helper".into())
            .spawn(move || read(child, stdout))
            .map_err(|err| tracing::warn!("keyboard hook helper reader failed to start: {err}"))
            .ok()?;
        Some(Helper { id, stdin })
    }

    fn send(slot: &mut Option<Helper>, line: &str) {
        if let Some(h) = slot.as_mut() {
            if writeln!(h.stdin, "{line}").and_then(|_| h.stdin.flush()).is_err() {
                tracing::warn!("keyboard hook helper did not take the bindings");
            }
        }
    }

    fn read(mut child: Child, stdout: ChildStdout) {
        let id = child.id();
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else {
                break;
            };
            match serde_json::from_str::<Note>(&line) {
                Ok(Note::Fire { generation, index }) => fire_entry(generation, index),
                Ok(Note::Log { level, text }) => {
                    let level = level.parse().unwrap_or(log::Level::Info);
                    log::log!(target: "versailles_lib::keys::hook", level, "{text}");
                }
                Err(_) => {}
            }
        }
        let _ = child.wait();
        ended(id);
    }

    /// The helper `id` exited: fine when we closed it, else start another.
    fn ended(id: u32) {
        static RESTARTED: AtomicU32 = AtomicU32::new(0);
        {
            let mut slot = slot().lock().unwrap();
            if !slot.as_ref().is_some_and(|h| h.id == id) {
                return;
            }
            slot.take();
        }
        if RESTARTED.fetch_add(1, Ordering::Relaxed) >= RESTARTS {
            tracing::warn!("keyboard hook helper ended; Win keys stay unbound until Versailles restarts");
            return;
        }
        tracing::warn!("keyboard hook helper ended; starting it again");
        std::thread::sleep(Duration::from_secs(1));
        let Some(line) = last().lock().unwrap().clone() else {
            return;
        };
        let mut slot = slot().lock().unwrap();
        if slot.is_none() {
            *slot = start();
            send(&mut slot, &line);
        }
    }
}

#[cfg(not(windows))]
mod client {
    pub fn run(_need: bool, _generation: u64, _table: &super::Table) {}
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

    #[test]
    fn table_finds_binds_before_suppress() {
        let win = parse("Win").unwrap();
        let a = parse("Win+A").unwrap();
        let t = Table {
            binds: vec![Entry { combo: win, piece: Some("start".into()) }],
            suppress: vec![Entry { combo: win, piece: None }, Entry { combo: a, piece: None }],
        };
        assert_eq!(t.find(&win), Some(0));
        assert_eq!(t.entry(0).and_then(|e| e.piece.as_deref()), Some("start"));
        assert_eq!(t.find(&a), Some(2));
        assert!(t.entry(2).is_some_and(|e| e.piece.is_none()));
        assert_eq!(t.find(&parse("Win+B").unwrap()), None);
        assert!(t.entry(3).is_none());
    }

    #[test]
    fn ring_keeps_order_and_drops_when_full() {
        let ring: Ring<u32, 4> = Ring::new();
        assert_eq!(ring.pop(), None);
        for v in 0..4 {
            assert!(ring.push(v));
        }
        assert!(!ring.push(99));
        assert!(!ring.push(100));
        assert_eq!(ring.take_dropped(), 2);
        assert_eq!(ring.take_dropped(), 0);
        assert_eq!(ring.pop(), Some(0));
        assert!(ring.push(4));
        assert_eq!((1..=4).map(|_| ring.pop().unwrap()).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
        assert_eq!(ring.pop(), None);
        // Wrap around the slots many times.
        for v in 0..1000 {
            assert!(ring.push(v));
            assert!(ring.push(v + 1));
            assert_eq!(ring.pop(), Some(v));
            assert_eq!(ring.pop(), Some(v + 1));
        }
    }

    #[test]
    fn ring_across_threads() {
        static RING: Ring<u64, 8> = Ring::new();
        const COUNT: u64 = 100_000;
        let producer = std::thread::spawn(|| {
            for v in 0..COUNT {
                while !RING.push(v) {
                    std::thread::yield_now();
                }
            }
        });
        let mut next = 0;
        while next < COUNT {
            match RING.pop() {
                Some(v) => {
                    assert_eq!(v, next);
                    next += 1;
                }
                None => std::thread::yield_now(),
            }
        }
        producer.join().unwrap();
        assert_eq!(RING.pop(), None);
    }

    #[test]
    fn latency() {
        assert_eq!(late_ms(1_000, 990), 10);
        // The tick count wrapped between the key and the callback.
        assert_eq!(late_ms(5, u32::MAX - 4), 10);
        // A stamp from the future (an injected key's own time).
        assert_eq!(late_ms(1_000, 1_200), 0);
        assert_eq!(lateness(99, 300), Lateness::OnTime);
        assert_eq!(lateness(100, 300), Lateness::Slow);
        assert_eq!(lateness(299, 300), Lateness::Slow);
        assert_eq!(lateness(300, 300), Lateness::TooLate);
        assert_eq!(lateness(50, 40), Lateness::TooLate);
        assert_eq!(hooks_timeout(None), 300);
        assert_eq!(hooks_timeout(Some(0)), 300);
        assert_eq!(hooks_timeout(Some(200)), 200);
        assert_eq!(hooks_timeout(Some(5_000)), 1_000);
    }

    #[test]
    fn witness_matches_hook() {
        assert!(!hook_saw(0, 1_000), "never seen");
        assert!(hook_saw(1_001, 1_000), "same stamp (| 1)");
        assert!(hook_saw(960, 1_000), "close enough");
        assert!(!hook_saw(900, 1_000), "an earlier press");
        assert!(hook_saw(1_100, 1_000), "a later press");
        assert!(hook_saw(u32::MAX - 9, 20), "across the wrap");
        assert!(!hook_saw(u32::MAX - 500, 20), "earlier, across the wrap");
        assert_eq!(watched(0x5B), Some(0));
        assert_eq!(watched(0x5C), Some(1));
        assert_eq!(watched(VK_F23), Some(2));
        assert_eq!(watched(0x41), None);
    }

    #[test]
    fn helper_messages_round_trip() {
        let t = Table {
            binds: vec![Entry { combo: parse("Win").unwrap(), piece: Some("action-bar".into()) }],
            suppress: vec![Entry { combo: parse("Copilot").unwrap(), piece: None }],
        };
        let line = serde_json::json!({ "generation": 7, "table": &t }).to_string();
        let b: Bindings = serde_json::from_str(&line).unwrap();
        assert_eq!(b.generation, 7);
        assert_eq!(b.table.find(&parse("Win").unwrap()), Some(0));
        assert_eq!(b.table.find(&parse("Copilot").unwrap()), Some(1));
        assert_eq!(b.table.entry(0).and_then(|e| e.piece.as_deref()), Some("action-bar"));
        let fire = Note::Fire { generation: 7, index: 1 };
        let back: Note = serde_json::from_str(&serde_json::to_string(&fire).unwrap()).unwrap();
        assert_eq!(back, fire);
        assert!(serde_json::from_str::<Note>(r#"{"log":{"level":"WARN","text":"x"}}"#).is_ok());
    }

    #[test]
    fn slow_summary_names_only_safe_keys() {
        assert_eq!(slow_summary(&[], 300), None);
        let one = slow_summary(&[(0x5B, 120)], 300).unwrap();
        assert_eq!(one, "keys: 1 key event reached the hook late (max 120 ms; 0x5b)");
        let text = slow_summary(&[(0x5B, 150), (0x41, 410), (0x42, 380), (0x5B, 200), (VK_F23, 120)], 300).unwrap();
        assert!(text.starts_with("keys: 5 key events reached the hook late (max 410 ms; 0x5b, 0x86, 2 other)"), "{text}");
        assert!(text.contains("after 300 ms (LowLevelHooksTimeout)"), "{text}");
        assert!(!text.contains("0x41") && !text.contains("0x42"), "{text}");
    }
}
