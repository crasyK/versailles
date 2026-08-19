//! Embedded ConPTY session for the Versailles Action Bar terminal mode.

use crate::cli;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    /// Separate from session lock so write_all cannot stall resize / is_alive.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
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
        let _ = s.killer.kill();
    }
}

/// Tear down any live PTY (hide launcher / leave terminal mode).
pub fn close_pty_session(state: &PtyState) {
    if let Ok(mut guard) = state.session.lock() {
        close_inner(&mut guard);
    }
}

const PTY_BATCH_MAX: usize = 16 * 1024;
const PTY_BATCH_MS: Duration = Duration::from_millis(8);

#[tauri::command]
pub fn pty_open(
    app: AppHandle,
    state: State<'_, PtyState>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    caller: Option<String>,
) -> Result<(), String> {
    crate::page::enforce_hook_from_disk(caller.as_deref(), "pty")?;
    let mut guard = state.session.lock().map_err(|e| e.to_string())?;
    close_inner(&mut guard);

    let shell = cli::shell_exe()?;
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
    // Keep profile so user aliases/path work; interactive session.
    if let Some(dir) = cwd.as_ref().filter(|d| std::path::Path::new(d).is_dir()) {
        cmd.cwd(dir);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn {shell}: {e}"))?;
    // Drop slave so only master remains attached.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone reader: {e}"))?;
    let writer = Arc::new(Mutex::new(
        pair.master
            .take_writer()
            .map_err(|e| format!("take writer: {e}"))?,
    ));
    let killer = child.clone_killer();
    let alive = Arc::new(AtomicBool::new(true));
    let alive_r = alive.clone();
    let app_r = app.clone();

    // Coalesce ConPTY output (~16KB / 8ms) so floods don't emit per-4KB IPC events.
    let pending = Arc::new(Mutex::new(Vec::<u8>::new()));
    let pending_r = pending.clone();
    let pending_f = pending.clone();
    let alive_f = alive.clone();
    let app_f = app.clone();

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
                let out = STANDARD.encode(&*p);
                p.clear();
                out
            };
            let _ = app_f.emit("pty://data", chunk);
        }
    });

    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while alive_r.load(Ordering::SeqCst) {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut emit_now: Option<String> = None;
                    if let Ok(mut p) = pending_r.lock() {
                        p.extend_from_slice(&buf[..n]);
                        if p.len() >= PTY_BATCH_MAX {
                            emit_now = Some(STANDARD.encode(&*p));
                            p.clear();
                        }
                    }
                    if let Some(chunk) = emit_now {
                        let _ = app_r.emit("pty://data", chunk);
                    }
                }
                Err(_) => break,
            }
        }
        // Flush remainder before exit signal.
        if let Ok(mut p) = pending_r.lock() {
            if !p.is_empty() {
                let chunk = STANDARD.encode(&*p);
                p.clear();
                let _ = app_r.emit("pty://data", chunk);
            }
        }
        let _ = app_r.emit("pty://exit", true);
        alive_r.store(false, Ordering::SeqCst);
    });

    *guard = Some(PtySession {
        master: pair.master,
        writer,
        killer,
        alive,
    });
    Ok(())
}

#[tauri::command]
pub fn pty_write(
    state: State<'_, PtyState>,
    data: String,
    caller: Option<String>,
) -> Result<(), String> {
    crate::page::enforce_hook_from_disk(caller.as_deref(), "pty")?;
    // Clone writer Arc under a short session lock, then write outside it.
    let writer = {
        let guard = state.session.lock().map_err(|e| e.to_string())?;
        let session = guard.as_ref().ok_or_else(|| "no pty session".to_string())?;
        session.writer.clone()
    };
    let mut w = writer.lock().map_err(|e| e.to_string())?;
    w.write_all(data.as_bytes())
        .map_err(|e| format!("pty write: {e}"))?;
    let _ = w.flush();
    Ok(())
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, PtyState>,
    cols: u16,
    rows: u16,
    caller: Option<String>,
) -> Result<(), String> {
    crate::page::enforce_hook_from_disk(caller.as_deref(), "pty")?;
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
    state: State<'_, PtyState>,
    caller: Option<String>,
) -> Result<bool, String> {
    crate::page::enforce_hook_from_disk(caller.as_deref(), "pty")?;
    let guard = state.session.lock().map_err(|e| e.to_string())?;
    Ok(guard
        .as_ref()
        .map(|s| s.alive.load(Ordering::SeqCst))
        .unwrap_or(false))
}

#[tauri::command]
pub fn pty_close(state: State<'_, PtyState>, caller: Option<String>) -> Result<(), String> {
    crate::page::enforce_hook_from_disk(caller.as_deref(), "pty")?;
    close_pty_session(&state);
    Ok(())
}
