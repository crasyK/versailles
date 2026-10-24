//! Hide the Windows taskbar while the page asks for it (#30):
//! `"shell": { "hideTaskbar": true }` in `#versailles` / `versailles.json`.
//!
//! Hiding = auto-hide (frees the strip the taskbar reserves) plus hidden
//! `Shell_TrayWnd` / `Shell_SecondaryTrayWnd` windows, hidden again the moment
//! Explorer shows them. Explorer stays the shell: Start, Quick Settings, the
//! notification center and toasts keep working. Nothing is injected.
//!
//! Getting the taskbar back never depends on this process staying healthy:
//! - a clean stop (config off, Quit) restores in place;
//! - while hiding, the hide thread owns the named mutex [`HIDDEN_MUTEX`]. A
//!   guard process (`--taskbar-guard`, outside the app's job and process
//!   tree) waits on it and restores when it is abandoned: a crash, the hang
//!   watchdog, a killed supervisor, Task Manager;
//! - `HKCU\...\RunOnce` runs `--restore-taskbar` at the next logon (logoff or
//!   power loss while hidden), and a start with hiding off restores too;
//! - `versailles --restore-taskbar` by hand.
//!
//! The taskbar's own auto-hide setting is saved before hiding
//! (`taskbar-restore.json` in the app's local data folder, not the synced
//! desktop folder: it describes this machine) and put back as it was.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

struct Hider {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

/// Set while this process holds the taskbar hidden (not when another
/// Versailles does): the tray (`tray`) only runs then.
static HIDING: AtomicBool = AtomicBool::new(false);

pub fn hiding() -> bool {
    HIDING.load(Ordering::SeqCst)
}

fn hider() -> &'static Mutex<Option<Hider>> {
    static HIDER: OnceLock<Mutex<Option<Hider>>> = OnceLock::new();
    HIDER.get_or_init(|| Mutex::new(None))
}

/// Follow the config: start or stop hiding. Called at boot and on every
/// config reload; does nothing when the state already matches.
pub fn apply(hide: bool) {
    let mut slot = hider().lock().unwrap();
    // The hide thread ends on its own when another Versailles already hides.
    if slot.as_ref().is_some_and(|h| h.thread.is_finished()) {
        *slot = None;
    }
    match (hide, slot.is_some()) {
        (true, false) => {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            match std::thread::Builder::new()
                .name("versailles-taskbar".into())
                .spawn(move || win::hide_while(&flag))
            {
                Ok(thread) => *slot = Some(Hider { stop, thread }),
                Err(err) => tracing::warn!("taskbar: hide thread failed to start: {err}"),
            }
        }
        (false, true) => stop(slot.take().unwrap()),
        (false, false) => {
            // Hidden by an earlier run that could not restore (guard gone too).
            if win::saved_state().is_some() && win::restore_if_orphaned() {
                tracing::info!("taskbar: restored after an earlier run");
            }
        }
        (true, true) => {}
    }
}

/// Restore before the app exits (next to `docks::release_all`).
pub fn release() {
    if let Some(h) = hider().lock().unwrap().take() {
        stop(h);
    }
}

fn stop(h: Hider) {
    // Icons go back to Explorer before its taskbar does (and the tray
    // watcher does not start it again meanwhile).
    HIDING.store(false, Ordering::SeqCst);
    crate::tray::release();
    h.stop.store(true, Ordering::SeqCst);
    // The thread wakes within one poll (300 ms), restores, then ends.
    let _ = h.thread.join();
}

/// Explorer's own taskbar window (never the tray window of ours).
#[cfg(windows)]
pub fn explorer_tray() -> Option<windows::Win32::Foundation::HWND> {
    win::primary()
}

/// `--restore-taskbar` and the guard run here, before the supervisor and
/// Tauri. `Some(exit code)` when the arguments asked for one of them.
pub fn run_cli() -> Option<i32> {
    let arg = std::env::args().nth(1)?;
    match arg.as_str() {
        "--restore-taskbar" => Some(if win::restore_if_orphaned() { 0 } else { 2 }),
        "--taskbar-guard" => {
            // Re-launch once more: the guard's parent then is a process that
            // has exited, so a tree kill of the app (`taskkill /T`, Task
            // Manager) does not reach it.
            win::spawn_self("--taskbar-guard-run");
            Some(0)
        }
        "--taskbar-guard-run" => {
            win::guard();
            Some(0)
        }
        _ => None,
    }
}

#[cfg(windows)]
mod win {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, HWND, LPARAM, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    use windows::Win32::System::Threading::{
        CreateMutexW, OpenMutexW, ReleaseMutex, WaitForSingleObject, INFINITE, SYNCHRONIZATION_ACCESS_RIGHTS,
    };
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, APPBARDATA};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, FindWindowExW, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible, MsgWaitForMultipleObjects,
        PeekMessageW, SetWindowPos, ShowWindow, TranslateMessage, EVENT_OBJECT_SHOW, HWND_BOTTOM, MSG, PM_REMOVE,
        QS_ALLINPUT, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNA, WINEVENT_OUTOFCONTEXT,
        WINEVENT_SKIPOWNPROCESS,
    };

    /// Owned by the hide thread for as long as the taskbar is hidden.
    const HIDDEN_MUTEX: PCWSTR = w!("Local\\Versailles.TaskbarHidden");
    const RUN_ONCE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce");
    const RUN_ONCE_VALUE: PCWSTR = w!("VersaillesRestoreTaskbar");
    const ABS_AUTOHIDE: usize = 0x1;
    /// SYNCHRONIZE | MUTEX_MODIFY_STATE: wait on it and release it.
    const MUTEX_WAIT_RELEASE: SYNCHRONIZATION_ACCESS_RIGHTS = SYNCHRONIZATION_ACCESS_RIGHTS(0x0010_0001);
    const POLL: Duration = Duration::from_millis(300);
    const STATE_CHECK: Duration = Duration::from_secs(2);

    fn note(text: &str) {
        crate::crash::append_log("taskbar.log", text);
    }

    fn restore_file() -> Option<PathBuf> {
        crate::crash::app_data_dir().map(|d| d.join("taskbar-restore.json"))
    }

    /// The taskbar's auto-hide / always-on-top state from before we hid it.
    pub fn saved_state() -> Option<usize> {
        let text = std::fs::read_to_string(restore_file()?).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        value["state"].as_u64().map(|s| s as usize)
    }

    fn save_state(state: usize) {
        let Some(path) = restore_file() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serde_json::json!({ "state": state }).to_string());
    }

    fn set_run_once(on: bool) {
        unsafe {
            if on {
                let Ok(exe) = std::env::current_exe() else {
                    return;
                };
                let cmd: Vec<u16> = format!("\"{}\" --restore-taskbar", exe.display())
                    .encode_utf16()
                    .chain([0])
                    .collect();
                let _ = RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    RUN_ONCE,
                    RUN_ONCE_VALUE,
                    REG_SZ.0,
                    Some(cmd.as_ptr() as *const _),
                    (cmd.len() * 2) as u32,
                );
            } else {
                let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_ONCE, RUN_ONCE_VALUE);
            }
        }
    }

    pub fn spawn_self(arg: &str) {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        if let Err(err) = std::process::Command::new(exe)
            .arg(arg)
            .creation_flags(DETACHED_PROCESS)
            .spawn()
        {
            note(&format!("could not start {arg}: {err}"));
        }
    }

    /// Explorer's taskbar. While the tray runs (`tray`), a Versailles has a
    /// `Shell_TrayWnd` of its own in front of it (ours, or another
    /// Versailles' such as the e2e's): take the one Explorer owns, else the
    /// first that is not ours.
    pub fn primary() -> Option<HWND> {
        let mut after: Option<HWND> = None;
        let mut fallback = None;
        while let Ok(h) = unsafe { FindWindowExW(None, after, w!("Shell_TrayWnd"), None) } {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
            if pid != std::process::id() {
                let explorer = crate::app_windows::process_path(pid).is_some_and(|p| {
                    std::path::Path::new(&p)
                        .file_name()
                        .is_some_and(|f| f.eq_ignore_ascii_case("explorer.exe"))
                });
                if explorer {
                    return Some(h);
                }
                fallback.get_or_insert(h);
            }
            after = Some(h);
        }
        fallback
    }

    /// The primary taskbar and one per other monitor.
    fn taskbars() -> Vec<HWND> {
        let mut out: Vec<HWND> = primary().into_iter().collect();
        let mut after: Option<HWND> = None;
        while let Ok(h) = unsafe { FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), None) } {
            out.push(h);
            after = Some(h);
        }
        out
    }

    fn is_taskbar(h: HWND) -> bool {
        let mut buf = [0u16; 32];
        let n = unsafe { GetClassNameW(h, &mut buf) }.max(0) as usize;
        let class = String::from_utf16_lossy(&buf[..n]);
        class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd"
    }

    fn hide(h: HWND) {
        unsafe {
            let _ = SetWindowPos(
                h,
                Some(HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    fn appbar(tray: HWND) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: tray,
            ..Default::default()
        }
    }

    fn get_state(tray: HWND) -> usize {
        let mut abd = appbar(tray);
        unsafe { SHAppBarMessage(ABM_GETSTATE, &mut abd) }
    }

    fn set_state(tray: HWND, state: usize) {
        let mut abd = appbar(tray);
        abd.lParam = LPARAM(state as isize);
        unsafe {
            SHAppBarMessage(ABM_SETSTATE, &mut abd);
        }
    }

    /// Explorer shows its windows again for Start, the auto-hide trigger and
    /// after a restart; hide them the moment it does.
    unsafe extern "system" fn on_show(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        id_object: i32,
        _id_child: i32,
        _thread: u32,
        _time: u32,
    ) {
        // OBJID_WINDOW: the window itself, not a part of it.
        if id_object == 0 && !hwnd.is_invalid() && is_taskbar(hwnd) {
            hide(hwnd);
        }
    }

    /// Mutex free (or abandoned by a crashed run) within 3 s, else another
    /// Versailles is hiding the taskbar and this one leaves it alone.
    fn acquire() -> Option<HANDLE> {
        unsafe {
            let m = CreateMutexW(None, false, HIDDEN_MUTEX).ok()?;
            let r = WaitForSingleObject(m, 3000);
            if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
                Some(m)
            } else {
                let _ = CloseHandle(m);
                None
            }
        }
    }

    /// The hide thread: hide, keep hidden until `stop`, restore.
    pub fn hide_while(stop: &AtomicBool) {
        let Some(mutex) = acquire() else {
            tracing::warn!("taskbar: another Versailles is hiding it; leaving it alone");
            return;
        };
        let Some(tray) = primary() else {
            tracing::warn!("taskbar: Explorer's taskbar not found; nothing to hide");
            unsafe {
                let _ = ReleaseMutex(mutex);
                let _ = CloseHandle(mutex);
            }
            return;
        };
        // Keep the first saved state: after a crash the taskbar may still be
        // on our auto-hide, which is not what the user had.
        let original = saved_state().unwrap_or_else(|| {
            let state = get_state(tray);
            save_state(state);
            state
        });
        set_run_once(true);
        spawn_self("--taskbar-guard");
        tracing::info!("taskbar: hiding (saved state {original:#x})");
        super::HIDING.store(true, Ordering::SeqCst);

        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_SHOW,
                None,
                Some(on_show),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        let mut checked = Instant::now() - STATE_CHECK;
        while !stop.load(Ordering::SeqCst) {
            if checked.elapsed() >= STATE_CHECK {
                // Also catches a restarted Explorer (new window, same setting)
                // and the user switching auto-hide off in Settings.
                if let Some(tray) = primary() {
                    let state = get_state(tray);
                    if state & ABS_AUTOHIDE == 0 {
                        set_state(tray, state | ABS_AUTOHIDE);
                    }
                }
                checked = Instant::now();
            }
            for h in taskbars() {
                if unsafe { IsWindowVisible(h) }.as_bool() {
                    hide(h);
                }
            }
            pump(POLL);
        }
        unsafe {
            if !hook.is_invalid() {
                let _ = UnhookWinEvent(hook);
            }
        }
        super::HIDING.store(false, Ordering::SeqCst);
        restore();
        tracing::info!("taskbar: restored");
        unsafe {
            let _ = ReleaseMutex(mutex);
            let _ = CloseHandle(mutex);
        }
    }

    /// Wait up to `timeout`, delivering WinEvent callbacks meanwhile.
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

    /// Put the saved auto-hide state back (left alone when nothing was
    /// saved), show every taskbar, and drop the restore file and RunOnce.
    fn restore() {
        if let (Some(tray), Some(state)) = (primary(), saved_state()) {
            set_state(tray, state);
        }
        for h in taskbars() {
            unsafe {
                let _ = ShowWindow(h, SW_SHOWNA);
            }
        }
        if let Some(path) = restore_file() {
            let _ = std::fs::remove_file(path);
        }
        set_run_once(false);
    }

    /// Restore unless a live Versailles holds the mutex (it hides on purpose).
    pub fn restore_if_orphaned() -> bool {
        unsafe {
            match OpenMutexW(MUTEX_WAIT_RELEASE, false, HIDDEN_MUTEX) {
                Ok(m) => {
                    let r = WaitForSingleObject(m, 0);
                    if r == WAIT_TIMEOUT {
                        let _ = CloseHandle(m);
                        note("--restore-taskbar: Versailles is hiding the taskbar; turn off shell.hideTaskbar or quit it");
                        return false;
                    }
                    restore();
                    let _ = ReleaseMutex(m);
                    let _ = CloseHandle(m);
                }
                Err(_) => restore(),
            }
        }
        true
    }

    /// Wait for the hiding run to end. A clean stop already restored (no
    /// restore file left); anything else restores here.
    pub fn guard() {
        unsafe {
            match OpenMutexW(MUTEX_WAIT_RELEASE, false, HIDDEN_MUTEX) {
                Ok(m) => {
                    let r = WaitForSingleObject(m, INFINITE);
                    if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
                        if saved_state().is_some() {
                            note(if r == WAIT_ABANDONED {
                                "Versailles ended while hiding the taskbar; restored it"
                            } else {
                                "hiding ended without a restore; restored the taskbar"
                            });
                            restore();
                        }
                        let _ = ReleaseMutex(m);
                    }
                    let _ = CloseHandle(m);
                }
                // The run ended before the guard got here.
                Err(_) => {
                    if saved_state().is_some() {
                        note("Versailles ended before the taskbar guard started; restored the taskbar");
                        restore();
                    }
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod win {
    use std::sync::atomic::AtomicBool;

    pub fn saved_state() -> Option<usize> {
        None
    }
    pub fn hide_while(_stop: &AtomicBool) {}
    pub fn restore_if_orphaned() -> bool {
        true
    }
    pub fn spawn_self(_arg: &str) {}
    pub fn guard() {}
}
