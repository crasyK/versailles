//! Embedded ConPTY session for the Versailles Action Bar terminal mode.
//!
//! Events (shared contract with the frontend):
//! - `pty://data` → `{ "session": u64, "data": "<base64>" }`
//! - `pty://exit` → `{ "session": u64 }`
//!
//! Every session gets a fresh id from a counter starting at 1; reader threads
//! only ever tag events with their own session's id, so output from a killed
//! session can never be mistaken for the new one.
//!
//! Both events go only to the windows of the piece that opened the session
//! (its spawn window and any monitor copy), never to every window: terminal
//! output can hold secrets, and other pieces could `listen` for it.

use crate::cli;
use crate::page::HookClass;
use crate::state::AppState;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State, Webview};

/// `pty_*` only from a spawn window whose piece declares `pty` (never the
/// wallpaper). Returns the piece the call acts as.
fn gate_pty(app_state: &AppState, webview: &Webview, caller: Option<&str>) -> Result<String, String> {
    crate::commands::require_hook(app_state, webview, caller, "pty", HookClass::Exec)
        .map_err(|e| e.to_string())
}

/// Send a pty event to the open windows of piece `owner` only.
fn emit_to_owner<S: Serialize + Clone>(app: &AppHandle, owner: &str, event: &str, payload: S) {
    for label in crate::window_manager::piece_window_labels(app, owner) {
        let _ = app.emit_to(label.as_str(), event, payload.clone());
    }
}

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
struct PtyData {
    session: u64,
    data: String,
}

#[derive(Clone, Serialize)]
struct PtyExit {
    session: u64,
}

pub struct PtySession {
    id: u64,
    master: Box<dyn MasterPty + Send>,
    /// Keystrokes go to a dedicated writer thread in arrival order; `pty_write`
    /// never blocks the main thread on a full ConPTY input pipe.
    input: Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    alive: Arc<AtomicBool>,
}

pub struct PtyState {
    session: Mutex<Option<PtySession>>,
}

impl Default for PtyState {
    fn default() -> Self {
        Self {
            session: Mutex::new(None),
        }
    }
}

fn close_inner(session: &mut Option<PtySession>) {
    if let Some(mut s) = session.take() {
        s.alive.store(false, Ordering::SeqCst);
        // taskkill waits for a child process: run it off the calling thread.
        // Kill the tree first while the shell PID still exists; TerminateProcess
        // on pwsh alone leaves ssh / TUI children orphaned on Windows.
        let pid = s.pid;
        std::thread::spawn(move || {
            if let Some(pid) = pid {
                kill_process_tree(pid);
            }
            let _ = s.killer.kill();
            tracing::debug!("pty session {} closed", s.id);
        });
    }
}

#[cfg(windows)]
fn kill_process_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .status();
}

#[cfg(not(windows))]
fn kill_process_tree(_pid: u32) {}

/// Tear down any live PTY (hide launcher / leave terminal mode).
pub fn close_pty_session(state: &PtyState) {
    if let Ok(mut guard) = state.session.lock() {
        close_inner(&mut guard);
    }
}

const PTY_BATCH_MAX: usize = 16 * 1024;
const PTY_BATCH_MS: Duration = Duration::from_millis(8);

/// Returns the new session id (JSON number).
#[tauri::command]
pub async fn pty_open(
    app: AppHandle,
    webview: Webview,
    app_state: State<'_, AppState>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    caller: Option<String>,
) -> Result<u64, String> {
    let owner = gate_pty(&app_state, &webview, caller.as_deref())?;
    // Shell probe + ConPTY spawn can take hundreds of ms: keep the UI thread free.
    cli::blocking(move || open_session(&app, owner, cwd, cols, rows)).await
}

fn open_session(app: &AppHandle, owner: String, cwd: Option<String>, cols: u16, rows: u16) -> Result<u64, String> {
    let shell = cli::shell_exe()?;
    let state = app.state::<PtyState>();
    let mut guard = state.session.lock().map_err(|e| e.to_string())?;
    close_inner(&mut guard);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.max(8),
            cols: cols.max(20),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let mut cmd = CommandBuilder::new(shell);
    cmd.arg("-NoLogo");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // Keep profile so user aliases/path work; interactive session.
    if let Some(dir) = cwd.as_ref().filter(|d| std::path::Path::new(d).is_dir()) {
        cmd.cwd(dir);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn {shell}: {e}"))?;
    let pid = child.process_id();
    // Drop slave so only master remains attached.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone reader: {e}"))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take writer: {e}"))?;
    let killer = child.clone_killer();
    let session = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
    let alive = Arc::new(AtomicBool::new(true));

    // Writer thread: drains keystrokes in order; ends when the session (and
    // with it the Sender) is dropped or the pipe breaks.
    let (input, input_rx) = channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for chunk in input_rx {
            if writer.write_all(&chunk).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    // Coalesce ConPTY output (~16KB / 8ms) so floods don't emit per-4KB IPC events.
    let pending = Arc::new(Mutex::new(Vec::<u8>::new()));
    let pending_r = pending.clone();
    let pending_f = pending.clone();
    let alive_r = alive.clone();
    let alive_f = alive.clone();
    let app_r = app.clone();
    let app_f = app.clone();
    let owner_r = owner.clone();
    let emit_data = move |app: &AppHandle, bytes: &[u8]| {
        emit_to_owner(
            app,
            &owner,
            "pty://data",
            PtyData {
                session,
                data: STANDARD.encode(bytes),
            },
        );
    };
    let emit_data_f = emit_data.clone();

    std::thread::spawn(move || {
        while alive_f.load(Ordering::SeqCst) {
            std::thread::sleep(PTY_BATCH_MS);
            let chunk = {
                let mut p = match pending_f.lock() {
                    Ok(g) => g,
                    Err(_) => break,
                };
                if p.is_empty() {
                    continue;
                }
                std::mem::take(&mut *p)
            };
            emit_data_f(&app_f, &chunk);
        }
    });

    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while alive_r.load(Ordering::SeqCst) {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut emit_now: Option<Vec<u8>> = None;
                    if let Ok(mut p) = pending_r.lock() {
                        p.extend_from_slice(&buf[..n]);
                        if p.len() >= PTY_BATCH_MAX {
                            emit_now = Some(std::mem::take(&mut *p));
                        }
                    }
                    if let Some(chunk) = emit_now {
                        emit_data(&app_r, &chunk);
                    }
                }
                Err(_) => break,
            }
        }
        // Flush remainder before exit signal.
        if let Ok(mut p) = pending_r.lock() {
            if !p.is_empty() {
                let chunk = std::mem::take(&mut *p);
                emit_data(&app_r, &chunk);
            }
        }
        emit_to_owner(&app_r, &owner_r, "pty://exit", PtyExit { session });
        alive_r.store(false, Ordering::SeqCst);
    });

    *guard = Some(PtySession {
        id: session,
        master: pair.master,
        input,
        killer,
        pid,
        alive,
    });
    Ok(session)
}

/// Stays a sync command on purpose: IPC messages are handled in order on the
/// main thread, so keystrokes reach the writer channel in order. The send never
/// blocks — the per-session writer thread does the actual pipe write.
#[tauri::command]
pub fn pty_write(
    webview: Webview,
    state: State<'_, PtyState>,
    app_state: State<'_, AppState>,
    data: String,
    caller: Option<String>,
) -> Result<(), String> {
    gate_pty(&app_state, &webview, caller.as_deref())?;
    let guard = state.session.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or_else(|| "no pty session".to_string())?;
    session
        .input
        .send(data.into_bytes())
        .map_err(|_| "pty write: session closed".to_string())
}

#[tauri::command]
pub fn pty_resize(
    webview: Webview,
    state: State<'_, PtyState>,
    app_state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    caller: Option<String>,
) -> Result<(), String> {
    gate_pty(&app_state, &webview, caller.as_deref())?;
    let guard = state.session.lock().map_err(|e| e.to_string())?;
    let session = guard.as_ref().ok_or_else(|| "no pty session".to_string())?;
    session
        .master
        .resize(PtySize {
            rows: rows.max(8),
            cols: cols.max(20),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("pty resize: {e}"))
}

#[tauri::command]
pub fn pty_is_alive(
    webview: Webview,
    state: State<'_, PtyState>,
    app_state: State<'_, AppState>,
    caller: Option<String>,
) -> Result<bool, String> {
    gate_pty(&app_state, &webview, caller.as_deref())?;
    let guard = state.session.lock().map_err(|e| e.to_string())?;
    Ok(guard
        .as_ref()
        .map(|s| s.alive.load(Ordering::SeqCst))
        .unwrap_or(false))
}

#[tauri::command]
pub fn pty_close(
    webview: Webview,
    state: State<'_, PtyState>,
    app_state: State<'_, AppState>,
    caller: Option<String>,
) -> Result<(), String> {
    gate_pty(&app_state, &webview, caller.as_deref())?;
    close_pty_session(&state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_payloads_match_frontend_contract() {
        let data = serde_json::to_value(PtyData {
            session: 7,
            data: STANDARD.encode(b"hi"),
        })
        .unwrap();
        assert_eq!(data, serde_json::json!({ "session": 7, "data": "aGk=" }));
        let exit = serde_json::to_value(PtyExit { session: 7 }).unwrap();
        assert_eq!(exit, serde_json::json!({ "session": 7 }));
    }
}
