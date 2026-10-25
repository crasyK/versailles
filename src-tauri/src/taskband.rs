//! Taskbar progress and overlay badges (#67): what apps set with
//! `ITaskbarList3::SetProgressValue` / `SetProgressState` / `SetOverlayIcon`
//! (a download's progress, a mail app's unread count), shown on the
//! `windows` hook as `progress` and `badge`.
//!
//! The TaskbarList object runs inside each app (`explorerframe.dll`). It
//! finds the taskbar with `FindWindow("Shell_TrayWnd")` and sends
//! `WM_USER + n` messages to the window named by that window's
//! `TaskbandHWND` property (Explorer's `MSTaskSwWClass`). While Versailles
//! holds the tray (`tray`), its `Shell_TrayWnd` comes first, so it names a
//! window of its own there, reads the messages and passes every one on to
//! Explorer's taskband unchanged, as it does for tray icons: Explorer keeps
//! the same state, and nothing is lost when Versailles stops. This is how
//! ManagedShell (Cairo, RetroBar) does it (`TasksService.setTaskbarListHwnd`),
//! with nothing injected. The message numbers are ManagedShell's:
//! `+64` progress value (0–65534 in `lParam`), `+65` progress state
//! (`TBPFLAG`), `+79` overlay icon (an `HICON`), `+85` its description (a
//! `SHAllocShared` block of the app's process). `wParam` is the window.
//!
//! Without a hidden taskbar, Explorer's taskband gets the messages directly
//! and windows read no `progress` / `badge`.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const WM_USER: u32 = 0x0400;
/// `ITaskbarList3::SetProgressValue`: `lParam` 0–[`PROGRESS_MAX`].
pub const SET_PROGRESS_VALUE: u32 = WM_USER + 64;
/// `ITaskbarList3::SetProgressState`: `lParam` a `TBPFLAG`.
pub const SET_PROGRESS_STATE: u32 = WM_USER + 65;
/// `ITaskbarList3::SetOverlayIcon`: `lParam` the icon (0 removes it).
pub const SET_OVERLAY_ICON: u32 = WM_USER + 79;
/// The overlay's accessible description: shared memory of the app's process.
pub const SET_OVERLAY_LABEL: u32 = WM_USER + 85;
/// The range TaskbarList scales progress to (RetroBar divides by it too).
pub const PROGRESS_MAX: u32 = 65534;


const TBPF_INDETERMINATE: u32 = 0x1;
const TBPF_NORMAL: u32 = 0x2;
const TBPF_ERROR: u32 = 0x4;
const TBPF_PAUSED: u32 = 0x8;

/// A window's progress as pages see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// `"normal"`, `"indeterminate"` (busy, no value), `"paused"` (yellow) or
    /// `"error"` (red).
    pub state: &'static str,
    /// 0–1; `null` while indeterminate.
    pub value: Option<f64>,
}

/// A window's overlay badge as pages see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Badge {
    /// `data:image/png;base64,…` at the icon's own size (16 px at 100 %).
    pub icon: String,
    /// What the app says it means ("3 unread messages"), when it says.
    pub label: Option<String>,
}

/// One message from an app, decoded (`id` is its window).
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Value { id: i64, value: u32 },
    State { id: i64, flags: u32 },
    /// The icon already drawn as a data URL (`None`: removed).
    Overlay { id: i64, icon: Option<String> },
    Label { id: i64, label: Option<String> },
}

#[derive(Debug, Default, Clone)]
struct Entry {
    flags: u32,
    value: u32,
    icon: Option<String>,
    label: Option<String>,
}

/// Progress and badges by window.
#[derive(Debug, Default)]
pub struct Band {
    windows: HashMap<i64, Entry>,
}

impl Band {
    /// Apply one message; whether anything a page sees changed.
    pub fn apply(&mut self, m: Message) -> bool {
        let id = match &m {
            Message::Value { id, .. } | Message::State { id, .. } | Message::Overlay { id, .. } | Message::Label { id, .. } => *id,
        };
        let before = (self.progress(id), self.badge(id));
        let e = self.windows.entry(id).or_default();
        match m {
            Message::Value { value, .. } => {
                e.value = value.min(PROGRESS_MAX);
                // "Unless SetProgressState has set a blocking state (TBPF_ERROR
                // or TBPF_PAUSED), SetProgressValue assumes TBPF_NORMAL."
                if e.flags & (TBPF_ERROR | TBPF_PAUSED) == 0 {
                    e.flags = TBPF_NORMAL;
                }
            }
            Message::State { flags, .. } => {
                e.flags = flags;
                if flags == 0 {
                    e.value = 0;
                }
            }
            Message::Overlay { icon, .. } => {
                if icon.is_none() {
                    e.label = None;
                }
                e.icon = icon;
            }
            Message::Label { label, .. } => e.label = label.filter(|l| !l.trim().is_empty()),
        }
        if e.flags == 0 && e.icon.is_none() && e.label.is_none() {
            self.windows.remove(&id);
        }
        before != (self.progress(id), self.badge(id))
    }

    pub fn progress(&self, id: i64) -> Option<Progress> {
        let e = self.windows.get(&id)?;
        let value = Some((e.value as f64 / PROGRESS_MAX as f64).clamp(0.0, 1.0));
        // Error and paused win over the rest, as Explorer draws them.
        let (state, value) = if e.flags & TBPF_ERROR != 0 {
            ("error", value)
        } else if e.flags & TBPF_PAUSED != 0 {
            ("paused", value)
        } else if e.flags & TBPF_INDETERMINATE != 0 {
            ("indeterminate", None)
        } else if e.flags & TBPF_NORMAL != 0 {
            ("normal", value)
        } else {
            return None;
        };
        Some(Progress { state, value })
    }

    pub fn badge(&self, id: i64) -> Option<Badge> {
        let e = self.windows.get(&id)?;
        Some(Badge { icon: e.icon.clone()?, label: e.label.clone() })
    }

    /// Forget windows `alive` says are gone.
    pub fn retain(&mut self, alive: impl Fn(i64) -> bool) {
        self.windows.retain(|id, _| alive(*id));
    }

    pub fn clear(&mut self) {
        self.windows.clear();
    }
}

fn band() -> &'static Mutex<Band> {
    static BAND: OnceLock<Mutex<Band>> = OnceLock::new();
    BAND.get_or_init(Default::default)
}

/// Record a message (from the tray thread); whether a page would see a change.
pub fn apply(m: Message) -> bool {
    band().lock().unwrap().apply(m)
}

/// Window `id`'s progress, if its app set one while Versailles held the taskbar.
pub fn progress(id: i64) -> Option<Progress> {
    band().lock().unwrap().progress(id)
}

/// Window `id`'s overlay badge, if its app set one while Versailles held the taskbar.
pub fn badge(id: i64) -> Option<Badge> {
    band().lock().unwrap().badge(id)
}

/// Keep only windows still listed.
pub fn retain(alive: impl Fn(i64) -> bool) {
    band().lock().unwrap().retain(alive);
}

/// Versailles stopped holding the taskbar: Explorer has it all again.
pub fn clear() {
    band().lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_follows_taskbar_list_rules() {
        let mut b = Band::default();
        assert_eq!(b.progress(1), None);
        // A value alone assumes the normal state.
        assert!(b.apply(Message::Value { id: 1, value: 32767 }));
        let p = b.progress(1).unwrap();
        assert_eq!(p.state, "normal");
        assert!((p.value.unwrap() - 0.5).abs() < 0.001);
        // Indeterminate has no value; a new value makes it normal again.
        assert!(b.apply(Message::State { id: 1, flags: TBPF_INDETERMINATE }));
        assert_eq!(b.progress(1), Some(Progress { state: "indeterminate", value: None }));
        b.apply(Message::Value { id: 1, value: PROGRESS_MAX });
        assert_eq!(b.progress(1), Some(Progress { state: "normal", value: Some(1.0) }));
        // Paused and error stay through value updates.
        b.apply(Message::State { id: 1, flags: TBPF_PAUSED });
        b.apply(Message::Value { id: 1, value: 0 });
        assert_eq!(b.progress(1), Some(Progress { state: "paused", value: Some(0.0) }));
        b.apply(Message::State { id: 1, flags: TBPF_ERROR });
        assert_eq!(b.progress(1).unwrap().state, "error");
        // The same value again is no change; no progress removes the entry.
        assert!(!b.apply(Message::State { id: 1, flags: TBPF_ERROR }));
        assert!(b.apply(Message::State { id: 1, flags: 0 }));
        assert_eq!(b.progress(1), None);
        assert!(b.windows.is_empty());
        // Out of range values are clamped.
        b.apply(Message::Value { id: 2, value: 99_999 });
        assert_eq!(b.progress(2).unwrap().value, Some(1.0));
    }

    #[test]
    fn badges_with_and_without_labels() {
        let mut b = Band::default();
        // A description before the icon shows nothing yet.
        assert!(!b.apply(Message::Label { id: 5, label: Some("3 unread".into()) }));
        assert!(b.apply(Message::Overlay { id: 5, icon: Some("data:a".into()) }));
        assert_eq!(b.badge(5), Some(Badge { icon: "data:a".into(), label: Some("3 unread".into()) }));
        assert!(b.apply(Message::Label { id: 5, label: Some("  ".into()) }));
        assert_eq!(b.badge(5).unwrap().label, None);
        // Removing the icon drops the badge; progress is independent.
        b.apply(Message::Value { id: 5, value: 10 });
        assert!(b.apply(Message::Overlay { id: 5, icon: None }));
        assert_eq!(b.badge(5), None);
        assert!(b.progress(5).is_some());
        b.retain(|id| id != 5);
        assert_eq!(b.progress(5), None);
    }


    #[test]
    fn payload_shape() {
        let p = serde_json::to_value(Progress { state: "indeterminate", value: None }).unwrap();
        assert_eq!(p, serde_json::json!({ "state": "indeterminate", "value": null }));
        let b = serde_json::to_value(Badge { icon: "data:x".into(), label: None }).unwrap();
        assert_eq!(b, serde_json::json!({ "icon": "data:x", "label": null }));
    }
}
