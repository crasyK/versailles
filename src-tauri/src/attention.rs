//! Attention (#58): app windows that flashed their taskbar button
//! (`FlashWindowEx` with `FLASHW_TRAY`, or Windows refusing an app the
//! foreground). The `windows` hook shows them as `attention: true` until the
//! window is activated.
//!
//! Shell hook messages (`RegisterShellHookWindow`) come from win32k to every
//! registered window, whoever holds `Shell_TrayWnd` and whether or not
//! Explorer's taskbar is shown; caption-only flashes send nothing. Learn
//! calls `RegisterShellHookWindow` "not intended for general use", but every
//! replacement shell relies on it (ManagedShell, Cairo, RetroBar).
//!
//! Checked live on Windows 11 24H2: a tray flash sends `HSHELL_FLASH` each
//! time the button lights and `HSHELL_REDRAW` each time it goes dark, and a
//! flash with a count ends lit (a last `HSHELL_FLASH`, no redraw).
//! `FLASHW_STOP` sends `HSHELL_REDRAW` while the button blinks, and nothing
//! once a counted flash has ended lit. Every title change sends
//! `HSHELL_REDRAW` too.
//!
//! So attention is set by the first `HSHELL_FLASH` and stays through the
//! blinks; it clears on activation (`HSHELL_WINDOWACTIVATED` /
//! `RUDEAPPACTIVATED`, or the window poll finding it in front), when the
//! window closes, and when a redraw with an unchanged title (not a title
//! update) is not followed by another flash within `SETTLE`: the app
//! stopped flashing. A counted flash that ended lit keeps attention until
//! activation, as Explorer keeps the button lit. ManagedShell clears on any
//! redraw, which would drop attention on every blink and on a chat app's
//! title update; Explorer keeps the button lit until activation.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// `HSHELL_WINDOWACTIVATED | HSHELL_HIGHBIT`: missing from the `windows` crate.
pub const HSHELL_RUDEAPPACTIVATED: u32 = 0x8004;
/// `HSHELL_REDRAW | HSHELL_HIGHBIT`: missing from the `windows` crate.
pub const HSHELL_FLASH: u32 = 0x8006;
const HSHELL_WINDOWDESTROYED: u32 = 2;
const HSHELL_WINDOWACTIVATED: u32 = 4;
const HSHELL_REDRAW: u32 = 6;

/// A dark button with no flash for this long has stopped flashing. Blinks
/// follow the caret blink time (about 0.5 s) or the app's own timeout.
pub const SETTLE: Duration = Duration::from_millis(2000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellEvent {
    Flash,
    Activated,
    Redraw,
    Destroyed,
    Other,
}

/// A shell hook message's `wParam`.
pub fn classify(code: u32) -> ShellEvent {
    match code {
        HSHELL_FLASH => ShellEvent::Flash,
        HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED => ShellEvent::Activated,
        HSHELL_REDRAW => ShellEvent::Redraw,
        HSHELL_WINDOWDESTROYED => ShellEvent::Destroyed,
        _ => ShellEvent::Other,
    }
}

#[derive(Debug)]
struct Entry {
    /// The title last seen, to tell a title update from a dark blink.
    title: String,
    /// Dark since a redraw: clears at this time unless it flashes again.
    clear_at: Option<Instant>,
}

/// Windows asking for attention.
#[derive(Debug, Default)]
pub struct Attention {
    flagged: HashMap<i64, Entry>,
}

impl Attention {
    /// Apply one shell event for window `id` (its title now; `foreground`:
    /// it is the window in front). Returns whether its attention changed.
    pub fn on_event(&mut self, event: ShellEvent, id: i64, title: &str, foreground: bool, now: Instant) -> bool {
        match event {
            // A window in front needs no attention (Explorer does not light it either).
            ShellEvent::Flash if foreground => false,
            ShellEvent::Flash => match self.flagged.get_mut(&id) {
                Some(entry) => {
                    entry.clear_at = None;
                    false
                }
                None => {
                    self.flagged.insert(id, Entry { title: title.to_string(), clear_at: None });
                    true
                }
            },
            ShellEvent::Activated | ShellEvent::Destroyed => self.clear(id),
            ShellEvent::Redraw => {
                if let Some(entry) = self.flagged.get_mut(&id) {
                    if entry.title == title {
                        // Dark: a blink, or the app stopped. The next flash tells.
                        entry.clear_at.get_or_insert(now + SETTLE);
                    } else {
                        // A title update: keep the attention.
                        entry.title = title.to_string();
                    }
                }
                false
            }
            ShellEvent::Other => false,
        }
    }

    /// Clear windows that stopped flashing while dark; whether any did.
    pub fn tick(&mut self, now: Instant) -> bool {
        let before = self.flagged.len();
        self.flagged.retain(|_, e| e.clear_at.is_none_or(|t| now < t));
        self.flagged.len() != before
    }

    pub fn contains(&self, id: i64) -> bool {
        self.flagged.contains_key(&id)
    }

    /// Drop `id`; whether it had attention.
    pub fn clear(&mut self, id: i64) -> bool {
        self.flagged.remove(&id).is_some()
    }

    /// Keep only windows `alive` says still exist.
    pub fn retain(&mut self, alive: impl Fn(i64) -> bool) {
        self.flagged.retain(|id, _| alive(*id));
    }

    pub fn reset(&mut self) {
        self.flagged.clear();
    }
}

fn state() -> &'static std::sync::Mutex<Attention> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<Attention>> = std::sync::OnceLock::new();
    STATE.get_or_init(Default::default)
}

/// Whether window `id` asks for attention.
pub fn flagged(id: i64) -> bool {
    state().lock().unwrap().contains(id)
}

/// The window came to the front: its attention is answered.
pub fn clear(id: i64) {
    state().lock().unwrap().clear(id);
}

/// Listen for flashes (idempotent). The window poll calls this while some
/// piece declares `windows`, and `stop` when none does.
pub fn start() {
    #[cfg(windows)]
    imp::start();
}

pub fn stop() {
    #[cfg(windows)]
    imp::stop();
    state().lock().unwrap().reset();
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread::JoinHandle;
    use windows::core::w;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DestroyWindow, DispatchMessageW,
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, IsWindow, MsgWaitForMultipleObjects,
        PeekMessageW, RegisterClassW, RegisterShellHookWindow, RegisterWindowMessageW, TranslateMessage, MSG,
        PM_REMOVE, QS_ALLINPUT, WINDOW_STYLE, WNDCLASSW, WS_EX_TOOLWINDOW,
    };

    struct Running {
        stop: Arc<AtomicBool>,
        thread: JoinHandle<()>,
    }

    fn slot() -> &'static Mutex<Option<Running>> {
        static SLOT: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
        SLOT.get_or_init(|| Mutex::new(None))
    }

    /// Shell hook messages seen (`wParam`, window), for the live test.
    #[cfg(test)]
    static SEEN: Mutex<Vec<(u32, i64)>> = Mutex::new(Vec::new());

    /// `RegisterWindowMessageW("SHELLHOOK")`.
    static SHELLHOOK: AtomicU32 = AtomicU32::new(0);

    pub fn start() {
        let mut slot = slot().lock().unwrap();
        if slot.as_ref().is_some_and(|r| !r.thread.is_finished()) {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        match std::thread::Builder::new()
            .name("versailles-shellhook".into())
            .spawn(move || run(&flag))
        {
            Ok(thread) => *slot = Some(Running { stop, thread }),
            Err(err) => tracing::warn!("attention: thread failed to start: {err}"),
        }
    }

    pub fn stop() {
        let Some(r) = slot().lock().unwrap().take() else {
            return;
        };
        r.stop.store(true, Ordering::SeqCst);
        let _ = r.thread.join();
    }

    fn title_of(h: HWND) -> String {
        // Another process's title is read from the window itself, without
        // sending it a message: a hung app cannot block this.
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

    unsafe extern "system" fn proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        let hook = SHELLHOOK.load(Ordering::SeqCst);
        if hook != 0 && msg == hook {
            #[cfg(test)]
            SEEN.lock().unwrap().push((w.0 as u32, l.0 as i64));
            let event = classify(w.0 as u32);
            if event != ShellEvent::Other {
                let target = HWND(l.0 as *mut _);
                let id = l.0 as i64;
                let title = if event == ShellEvent::Destroyed { String::new() } else { title_of(target) };
                let foreground = GetForegroundWindow() == target;
                let changed = state().lock().unwrap().on_event(event, id, &title, foreground, Instant::now());
                if changed {
                    tracing::debug!("attention: {event:?} {id}");
                }
            }
            return LRESULT(0);
        }
        DefWindowProcW(h, msg, w, l)
    }

    fn create() -> Option<HWND> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
            // Registering again (a second start) fails harmlessly.
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance,
                lpszClassName: w!("VersaillesShellHook"),
                ..Default::default()
            });
            // A hidden top-level window: shell hook messages do not reach
            // message-only windows.
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("VersaillesShellHook"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|err| tracing::warn!("attention: window failed: {err}"))
            .ok()
        }
    }

    fn run(stop: &AtomicBool) {
        SHELLHOOK.store(unsafe { RegisterWindowMessageW(w!("SHELLHOOK")) }, Ordering::SeqCst);
        let Some(window) = create() else {
            return;
        };
        if !unsafe { RegisterShellHookWindow(window) }.as_bool() {
            tracing::warn!("attention: RegisterShellHookWindow failed");
            unsafe {
                let _ = DestroyWindow(window);
            }
            return;
        }
        let mut swept = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            unsafe {
                MsgWaitForMultipleObjects(None, false, 250, QS_ALLINPUT);
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            let mut attention = state().lock().unwrap();
            attention.tick(Instant::now());
            if swept.elapsed() >= Duration::from_secs(2) {
                swept = Instant::now();
                attention.retain(|id| unsafe { IsWindow(Some(HWND(id as isize as *mut _))) }.as_bool());
            }
        }
        unsafe {
            let _ = DeregisterShellHookWindow(window);
            let _ = DestroyWindow(window);
        }
    }

    #[cfg(test)]
    unsafe extern "system" fn probe_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        DefWindowProcW(h, msg, w, l)
    }

    /// A window of this test process flashes its taskbar button; the shell
    /// hook must see it: `cargo test --lib live_flash -- --ignored --nocapture`.
    #[cfg(test)]
    pub fn live_flash() {
        use windows::Win32::UI::WindowsAndMessaging::{
            FlashWindowEx, ShowWindow, FLASHWINFO, FLASHW_STOP, FLASHW_TIMER, FLASHW_TRAY, SW_SHOWNOACTIVATE, WS_OVERLAPPEDWINDOW,
        };
        start();
        std::thread::sleep(Duration::from_millis(300));
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).unwrap().0);
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(probe_proc),
                hInstance: instance,
                lpszClassName: w!("VersaillesFlashProbe"),
                ..Default::default()
            });
            let probe = CreateWindowExW(
                Default::default(),
                w!("VersaillesFlashProbe"),
                w!("Versailles flash probe"),
                WS_OVERLAPPEDWINDOW,
                -2000,
                -2000,
                200,
                100,
                None,
                None,
                Some(instance),
                None,
            )
            .expect("probe window");
            let _ = ShowWindow(probe, SW_SHOWNOACTIVATE);
            let pump = |ms: u64| {
                let until = Instant::now() + Duration::from_millis(ms);
                while Instant::now() < until {
                    let mut msg = MSG::default();
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&msg);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            pump(300);
            let id = probe.0 as isize as i64;
            let foreground = GetForegroundWindow() == probe;
            // Flash until stopped: attention must hold through the blinks.
            let mut info = FLASHWINFO {
                cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                hwnd: probe,
                dwFlags: FLASHW_TRAY | FLASHW_TIMER,
                uCount: 0,
                dwTimeout: 0,
            };
            let _ = FlashWindowEx(&info);
            pump(300);
            let mut held = true;
            for _ in 0..25 {
                pump(100);
                held &= flagged(id);
            }
            eprintln!("probe {id}: foreground={foreground} flagged through 2.5 s of blinks={held}");
            info.dwFlags = FLASHW_STOP;
            let _ = FlashWindowEx(&info);
            pump(2600);
            let after_stop = flagged(id);
            eprintln!("flagged 2.6 s after FLASHW_STOP={after_stop}");
            let messages: Vec<String> =
                SEEN.lock().unwrap().iter().filter(|(_, h)| *h == id).map(|(c, _)| format!("{c:#x}")).collect();
            eprintln!("shell hook messages for the probe: {messages:?}");
            let _ = DestroyWindow(probe);
            pump(400);
            let after_close = flagged(id);
            stop();
            if !foreground {
                assert!(held, "attention did not hold through the blinks");
                assert!(!after_stop, "FLASHW_STOP did not clear");
                assert!(!after_close, "closing did not clear");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_shell_codes() {
        assert_eq!(classify(0x8006), ShellEvent::Flash);
        assert_eq!(classify(4), ShellEvent::Activated);
        assert_eq!(classify(0x8004), ShellEvent::Activated);
        assert_eq!(classify(6), ShellEvent::Redraw);
        assert_eq!(classify(2), ShellEvent::Destroyed);
        assert_eq!(classify(1), ShellEvent::Other); // created
        assert_eq!(classify(0x10), ShellEvent::Other); // monitor changed
    }

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn flash_then_activate() {
        let now = t0();
        let mut a = Attention::default();
        assert!(a.on_event(ShellEvent::Flash, 7, "Chat", false, now));
        assert!(a.contains(7));
        // The next blink changes nothing.
        assert!(!a.on_event(ShellEvent::Flash, 7, "Chat", false, now));
        // Another window activated: still flagged.
        assert!(!a.on_event(ShellEvent::Activated, 8, "Editor", true, now));
        assert!(a.contains(7));
        assert!(a.on_event(ShellEvent::Activated, 7, "Chat", true, now));
        assert!(!a.contains(7));
    }

    #[test]
    fn a_window_in_front_gets_no_attention() {
        let mut a = Attention::default();
        assert!(!a.on_event(ShellEvent::Flash, 7, "Chat", true, t0()));
        assert!(!a.contains(7));
    }

    #[test]
    fn blinks_keep_attention() {
        let now = t0();
        let ms = |n: u64| now + Duration::from_millis(n);
        let mut a = Attention::default();
        a.on_event(ShellEvent::Flash, 7, "Chat", false, now);
        // Dark, lit, dark, lit: every half second, as Windows blinks.
        for k in 0..6u64 {
            let event = if k % 2 == 0 { ShellEvent::Redraw } else { ShellEvent::Flash };
            assert!(!a.on_event(event, 7, "Chat", false, ms(500 * (k + 1))));
            assert!(!a.tick(ms(500 * (k + 1) + 250)));
            assert!(a.contains(7));
        }
        // Ends lit (a flash count ran out): kept until activation.
        assert!(!a.tick(ms(60_000)));
        assert!(a.contains(7));
    }

    #[test]
    fn stopping_while_dark_clears_after_settling() {
        let now = t0();
        let ms = |n: u64| now + Duration::from_millis(n);
        let mut a = Attention::default();
        a.on_event(ShellEvent::Flash, 7, "Chat", false, now);
        a.on_event(ShellEvent::Redraw, 7, "Chat", false, ms(500));
        assert!(!a.tick(ms(500) + SETTLE - Duration::from_millis(1)));
        assert!(a.contains(7));
        assert!(a.tick(ms(500) + SETTLE));
        assert!(!a.contains(7));
    }

    #[test]
    fn a_title_update_keeps_attention() {
        let now = t0();
        let ms = |n: u64| now + Duration::from_millis(n);
        let mut a = Attention::default();
        a.on_event(ShellEvent::Flash, 7, "Chat", false, now);
        // A new message changes the title: a redraw that is no blink.
        assert!(!a.on_event(ShellEvent::Redraw, 7, "Chat (1)", false, ms(100)));
        assert!(!a.tick(ms(100) + SETTLE * 2));
        assert!(a.contains(7));
        // Later blinks compare with the new title.
        a.on_event(ShellEvent::Redraw, 7, "Chat (1)", false, ms(10_000));
        assert!(a.tick(ms(10_000) + SETTLE));
        // A redraw of a window without attention does nothing.
        assert!(!a.on_event(ShellEvent::Redraw, 9, "x", false, now));
        assert!(!a.contains(9));
    }

    #[test]
    fn closing_and_sweeping_clear() {
        let now = t0();
        let mut a = Attention::default();
        a.on_event(ShellEvent::Flash, 1, "a", false, now);
        a.on_event(ShellEvent::Flash, 2, "b", false, now);
        a.on_event(ShellEvent::Flash, 3, "c", false, now);
        assert!(a.on_event(ShellEvent::Destroyed, 1, "", false, now));
        a.retain(|id| id != 2);
        assert!(!a.contains(1) && !a.contains(2) && a.contains(3));
        assert!(!a.on_event(ShellEvent::Other, 3, "c", false, now));
        a.reset();
        assert!(!a.contains(3));
    }

    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_flash() {
        imp::live_flash();
    }
}
