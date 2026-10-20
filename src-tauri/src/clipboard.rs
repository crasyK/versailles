//! `clipboard` hook: read / write plain text and a change notice. The host
//! keeps no history; a page that wants one keeps it itself.

#[cfg(not(windows))]
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Longest text handed to a page (UTF-16 units).
const MAX_READ: usize = 4 * 1024 * 1024;

#[cfg(windows)]
mod win {
    use super::MAX_READ;
    use crate::error::{AppError, AppResult};
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
        IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    /// Open clipboard, closed on drop. Another app may hold it briefly: retry.
    struct Open;

    impl Open {
        fn new() -> AppResult<Self> {
            for _ in 0..10 {
                if unsafe { OpenClipboard(None) }.is_ok() {
                    return Ok(Open);
                }
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Err(AppError::msg("clipboard is busy"))
        }
    }

    impl Drop for Open {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    pub fn read_text() -> AppResult<Option<String>> {
        let format = CF_UNICODETEXT.0 as u32;
        if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
            return Ok(None);
        }
        let _open = Open::new()?;
        let handle = unsafe { GetClipboardData(format) }.map_err(|e| AppError::msg(e.to_string()))?;
        let mem = HGLOBAL(handle.0);
        unsafe {
            let ptr = GlobalLock(mem) as *const u16;
            if ptr.is_null() {
                return Ok(None);
            }
            let cap = (GlobalSize(mem) / 2).min(MAX_READ);
            let units = std::slice::from_raw_parts(ptr, cap);
            let len = units.iter().position(|&c| c == 0).unwrap_or(cap);
            let text = String::from_utf16_lossy(&units[..len]);
            let _ = GlobalUnlock(mem);
            Ok(Some(text))
        }
    }

    pub fn write_text(text: &str) -> AppResult<()> {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        units.push(0);
        let _open = Open::new()?;
        unsafe {
            EmptyClipboard().map_err(|e| AppError::msg(e.to_string()))?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2).map_err(|e| AppError::msg(e.to_string()))?;
            let ptr = GlobalLock(mem) as *mut u16;
            if ptr.is_null() {
                return Err(AppError::msg("clipboard: could not lock memory"));
            }
            std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
            let _ = GlobalUnlock(mem);
            // On success the clipboard owns `mem`.
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0)))
                .map_err(|e| AppError::msg(e.to_string()))?;
        }
        Ok(())
    }

    pub fn sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }
}

#[cfg(windows)]
pub use win::{read_text, write_text};

#[cfg(not(windows))]
pub fn read_text() -> AppResult<Option<String>> {
    Err(AppError::msg("clipboard is only available on Windows"))
}

#[cfg(not(windows))]
pub fn write_text(_text: &str) -> AppResult<()> {
    Err(AppError::msg("clipboard is only available on Windows"))
}

/// Window labels of pieces that declared `clipboard` (the wallpaper hosts
/// every `article.widget`, so it counts once).
fn listener_labels(app: &AppHandle) -> Vec<String> {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    crate::bus::hook_window_labels(app, &cat, "clipboard")
}

/// Send `clipboard://changed` (no content) to pieces with the hook whenever
/// the system clipboard changes. Polls the sequence number, which is cheap.
pub fn start_change_watcher(app: AppHandle) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        let mut last = win::sequence();
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let now = win::sequence();
            if now == last {
                continue;
            }
            last = now;
            for label in listener_labels(&app) {
                let _ = app.emit_to(label.as_str(), "clipboard://changed", ());
            }
            // Tauri events do not reach the wallpaper's page iframe; the push
            // channel does (see `bus::frame_script`).
            crate::push::to_hook(&app, "clipboard", &serde_json::Value::Null);
        }
    });
    #[cfg(not(windows))]
    let _ = (app, Duration::ZERO);
}
