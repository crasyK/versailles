//! Crash recovery: a tiny supervisor process, a panic log, and a hang watchdog.
//!
//! Release builds keep `panic = "abort"` (mutexes are `.lock().unwrap()`
//! everywhere; unwinding would cascade poisoned locks) and recover by restart:
//!
//! - `versailles.exe` normally starts as a **supervisor** that never touches
//!   Tauri. It re-launches itself with `VERSAILLES_SUPERVISED=1`, puts that child
//!   in a kill-on-close Job Object (killing the supervisor kills the app), and
//!   restarts it with backoff when it exits non-zero. Exit code 0 (Quit, or a
//!   second instance that forwarded to the first) ends the supervisor too.
//! - The **panic hook** writes message, location and thread to the log and to
//!   `last-crash.txt` before the abort.
//! - The **hang watchdog** (supervised child only) pings the main thread every
//!   5 s and terminates the process with code 3 after 45 s without an answer,
//!   so a WebView2 deadlock turns into a restart.
//!
//! Opt-outs: `VERSAILLES_NO_SUPERVISOR=1`, `VERSAILLES_NO_WATCHDOG=1`. Debug and
//! bench (`VERSAILLES_BENCH=1` / `--bench-boot`) builds never supervise.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Must match `identifier` in tauri.conf.json (Tauri's `app_log_dir`).
const IDENTIFIER: &str = "com.versailles.widgets";
const SUPERVISED_ENV: &str = "VERSAILLES_SUPERVISED";

static SUPERVISED: AtomicBool = AtomicBool::new(false);
static EXITING: AtomicBool = AtomicBool::new(false);

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// `%LOCALAPPDATA%\com.versailles.widgets`: per-machine state.
pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(IDENTIFIER))
}

/// Same folder tauri-plugin-log writes `Versailles.log` to.
pub fn log_dir() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join("logs"))
}

/// A line in a log file of its own, for processes without the logger
/// (supervisor, taskbar guard).
pub fn append_log(name: &str, text: &str) {
    write_log_file(name, text, true);
}

fn write_log_file(name: &str, text: &str, append: bool) {
    let Some(dir) = log_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(dir.join(name));
    if let Ok(mut file) = file {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{stamp}] {text}");
    }
}

/// Note a fatal condition in `last-crash.txt` (overwritten) and the log.
pub fn record_fatal(text: &str) {
    // File first: if the logger itself is what broke, the file still lands.
    write_log_file("last-crash.txt", text, false);
    log::error!("{text}");
}

pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>");
        let payload = info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        let backtrace = std::backtrace::Backtrace::force_capture();
        record_fatal(&format!(
            "panic in thread '{name}' at {location}: {msg}\n{backtrace}"
        ));
        default(info);
    }));
}

/// Mark a requested shutdown so the watchdog never turns a slow exit into a restart.
pub fn note_exiting() {
    EXITING.store(true, Ordering::SeqCst);
}

/// A shutdown was requested (and not cancelled).
pub fn exiting() -> bool {
    EXITING.load(Ordering::SeqCst)
}

fn terminate_now(code: u32) -> ! {
    // TerminateProcess, not exit(): a hung main thread may hold the loader lock,
    // and ExitProcess would then hang too.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        let _ = TerminateProcess(GetCurrentProcess(), code);
    }
    std::process::exit(code as i32)
}

const PING_EVERY: Duration = Duration::from_secs(5);
const HANG_LIMIT: Duration = Duration::from_secs(45);
/// A watchdog sleep that overshoots by this much means the machine slept.
const SLEEP_SLACK: Duration = Duration::from_secs(10);

/// Only in the supervised child (a restart needs someone to restart us).
pub fn start_hang_watchdog(app: &tauri::AppHandle) {
    if !SUPERVISED.load(Ordering::SeqCst) || env_flag("VERSAILLES_NO_WATCHDOG") {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("versailles-watchdog".into())
        .spawn(move || {
            let answered = Arc::new(Mutex::new(Instant::now()));
            let mut tick = Instant::now();
            loop {
                std::thread::sleep(PING_EVERY);
                let now = Instant::now();
                if now.duration_since(tick) > PING_EVERY + SLEEP_SLACK {
                    // System sleep / hibernate: the main thread did not hang.
                    *answered.lock().unwrap_or_else(|e| e.into_inner()) = now;
                }
                tick = now;
                let silent = now.duration_since(*answered.lock().unwrap_or_else(|e| e.into_inner()));
                if silent > HANG_LIMIT {
                    if EXITING.load(Ordering::SeqCst) {
                        record_fatal("shutdown stalled; terminating");
                        terminate_now(0);
                    }
                    record_fatal(&format!(
                        "main thread unresponsive for {}s (likely a WebView2 deadlock); terminating for restart",
                        silent.as_secs()
                    ));
                    terminate_now(3);
                }
                let slot = answered.clone();
                if app
                    .run_on_main_thread(move || {
                        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                    })
                    .is_err()
                {
                    // Event loop is gone (shutting down).
                    return;
                }
            }
        });
    if let Err(err) = spawned {
        tracing::warn!("hang watchdog failed to start: {err}");
    }
}

const BACKOFF_SECS: [u64; 5] = [1, 2, 5, 10, 30];
const CRASH_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Restart backoff: 1s, 2s, 5s, 10s, 30s for the 1st..5th crash inside a
/// sliding 10-minute window; a 6th crash in that window gives up.
#[derive(Debug, Default)]
pub struct RestartPolicy {
    crashes: VecDeque<Instant>,
}

impl RestartPolicy {
    pub fn on_crash(&mut self, now: Instant) -> Option<Duration> {
        while self
            .crashes
            .front()
            .is_some_and(|t| now.duration_since(*t) >= CRASH_WINDOW)
        {
            self.crashes.pop_front();
        }
        self.crashes.push_back(now);
        BACKOFF_SECS
            .get(self.crashes.len() - 1)
            .map(|s| Duration::from_secs(*s))
    }
}

/// Call first in `main()`. Returns `Some(exit code)` when this process acted as
/// the supervisor (the app ran in a child); `None` means "run the app here".
pub fn supervise_if_needed() -> Option<i32> {
    if env_flag(SUPERVISED_ENV) {
        SUPERVISED.store(true, Ordering::SeqCst);
        // Do not leak into shells / apps this process launches.
        std::env::remove_var(SUPERVISED_ENV);
        return None;
    }
    if cfg!(debug_assertions)
        || !cfg!(windows)
        || env_flag("VERSAILLES_NO_SUPERVISOR")
        || env_flag("VERSAILLES_BENCH")
        || std::env::args().any(|a| a == "--bench-boot")
    {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    Some(run_supervisor(exe))
}

fn supervisor_log(text: &str) {
    append_log("supervisor.log", text);
}

fn run_supervisor(exe: PathBuf) -> i32 {
    let job = job::KillOnCloseJob::create();
    if job.is_none() {
        supervisor_log("job object unavailable; the app will outlive a killed supervisor");
    }
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut policy = RestartPolicy::default();
    loop {
        let started = Instant::now();
        let mut child = match std::process::Command::new(&exe)
            .args(&args)
            .env(SUPERVISED_ENV, "1")
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                supervisor_log(&format!("cannot start {}: {err}", exe.display()));
                return 1;
            }
        };
        if let Some(job) = &job {
            if !job.assign(&child) {
                supervisor_log("could not assign the app to the job object");
            }
        }
        let code = match child.wait() {
            Ok(status) => status.code().unwrap_or(1),
            Err(err) => {
                supervisor_log(&format!("wait failed: {err}"));
                1
            }
        };
        if code == 0 {
            return 0;
        }
        let uptime = started.elapsed().as_secs();
        match policy.on_crash(Instant::now()) {
            Some(delay) => {
                supervisor_log(&format!(
                    "app exited with code {code} (0x{:08X}) after {uptime}s; restarting in {}s",
                    code as u32,
                    delay.as_secs()
                ));
                std::thread::sleep(delay);
            }
            None => {
                supervisor_log(&format!(
                    "app exited with code {code} (0x{:08X}) after {uptime}s; too many crashes in 10 minutes, giving up",
                    code as u32
                ));
                return code;
            }
        }
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
    };

    /// Closing the last handle (supervisor exit or kill) terminates the app.
    /// Silent breakaway: processes the app launches (apps from the action bar,
    /// terminals, WebView2) are NOT in the job and survive a Versailles restart.
    pub struct KillOnCloseJob(HANDLE);

    impl KillOnCloseJob {
        pub fn create() -> Option<Self> {
            unsafe {
                let handle = CreateJobObjectW(None, PCWSTR::null()).ok()?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .ok()?;
                // The handle lives for the rest of the supervisor process on purpose.
                Some(Self(handle))
            }
        }

        pub fn assign(&self, child: &std::process::Child) -> bool {
            let process = HANDLE(child.as_raw_handle());
            unsafe { AssignProcessToJobObject(self.0, process).is_ok() }
        }
    }
}

#[cfg(not(windows))]
mod job {
    pub struct KillOnCloseJob;

    impl KillOnCloseJob {
        pub fn create() -> Option<Self> {
            None
        }

        pub fn assign(&self, _child: &std::process::Child) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_then_give_up_within_window() {
        let mut policy = RestartPolicy::default();
        let t0 = Instant::now();
        let secs: Vec<Option<u64>> = (0..6)
            .map(|i| {
                policy
                    .on_crash(t0 + Duration::from_secs(i * 30))
                    .map(|d| d.as_secs())
            })
            .collect();
        assert_eq!(
            secs,
            vec![Some(1), Some(2), Some(5), Some(10), Some(30), None]
        );
    }

    #[test]
    fn old_crashes_age_out_of_the_window() {
        let mut policy = RestartPolicy::default();
        let t0 = Instant::now();
        for i in 0..4 {
            assert!(policy.on_crash(t0 + Duration::from_secs(i)).is_some());
        }
        // 11 minutes later the earlier crashes no longer count.
        let later = t0 + Duration::from_secs(11 * 60);
        assert_eq!(policy.on_crash(later), Some(Duration::from_secs(1)));
    }

    #[test]
    fn identifier_matches_tauri_conf() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"].as_str(), Some(IDENTIFIER));
    }
}
