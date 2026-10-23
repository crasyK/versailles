//! `notify` hook: the user's recent toast notifications, read-only, through
//! Windows' `UserNotificationListener`. `notify-control` (#33) dismisses and
//! clears them, opens the app that raised one, and turns an app's own banners
//! on or off; it goes through `quick_set` (control class, see `quick`).
//!
//! An unpackaged process (no package identity) can call `GetAccessStatus`
//! and `GetNotificationsAsync`, but registering for `NotificationChanged`
//! fails with `0x80070490` (element not found). So new toasts are found by
//! polling the list and diffing ids (see `sensors::start_watcher`).
//! `RequestAccessAsync` is never called: the user grants or denies access
//! under Windows Settings > Privacy & security > Notifications.

use crate::error::{AppError, AppResult};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;

/// Most recent toasts `now()` returns.
pub const RECENT_LIMIT: usize = 50;

/// One toast as a page sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toast {
    /// Windows' id for this notification (unique while it is in the list).
    pub id: u32,
    /// Display name of the app that raised it, when Windows knows it.
    pub app: Option<String>,
    /// App user model id of that app.
    pub aumid: Option<String>,
    pub title: String,
    pub body: String,
    /// When it arrived, ms since the Unix epoch.
    pub time: i64,
}

/// `UserNotificationListenerAccessStatus` → page string.
pub fn access_name(status: i32) -> &'static str {
    match status {
        1 => "allowed",
        2 => "denied",
        _ => "unspecified",
    }
}

/// WinRT `DateTime` (100 ns ticks since 1601-01-01 UTC) → Unix ms.
pub fn ticks_to_unix_ms(ticks: i64) -> i64 {
    const EPOCH_DIFF_TICKS: i64 = 116_444_736_000_000_000;
    (ticks - EPOCH_DIFF_TICKS).div_euclid(10_000)
}

/// A toast's text elements → (title, body): the first line is the title,
/// the rest (non-empty) the body, one per line.
pub fn title_and_body(texts: &[String]) -> (String, String) {
    let mut lines = texts.iter().map(|s| s.trim()).filter(|s| !s.is_empty());
    let title = lines.next().unwrap_or_default().to_string();
    let body = lines.collect::<Vec<_>>().join("\n");
    (title, body)
}

/// Newest first, at most `limit`.
pub fn recent(mut items: Vec<Toast>, limit: usize) -> Vec<Toast> {
    items.sort_by(|a, b| b.time.cmp(&a.time).then(b.id.cmp(&a.id)));
    items.truncate(limit);
    items
}

/// Ids in `current` that `seen` does not hold, oldest first (push order).
pub fn unseen(seen: &HashSet<u32>, current: &[(u32, i64)]) -> Vec<u32> {
    let mut fresh: Vec<(u32, i64)> = current
        .iter()
        .copied()
        .filter(|(id, _)| !seen.contains(id))
        .collect();
    fresh.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    fresh.into_iter().map(|(id, _)| id).collect()
}

/// `notify.now()`: `{ access, items }`, items newest first. Without access
/// `items` is empty; on a system with no listener `access` is `unavailable`.
/// Remove one notification from Action Center.
pub fn dismiss(id: u32) -> AppResult<()> {
    imp::dismiss(id)
}

/// Remove all of them.
pub fn clear() -> AppResult<()> {
    imp::clear()
}

/// App user model ids name apps, never paths or arguments: letters, digits,
/// `.`, `_`, `-`, `!`, `{`, `}`, and `\` (desktop apps use a path-like id).
pub fn valid_aumid(aumid: &str) -> bool {
    !aumid.is_empty()
        && aumid.len() <= 512
        && !aumid.contains("..")
        && aumid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '!' | '{' | '}' | '\\' | ' '))
}

/// Open the app that raised a notification (its own click action is not
/// reachable from outside: Windows does not hand out its arguments).
pub fn open_app(aumid: &str) -> AppResult<()> {
    if !valid_aumid(aumid) {
        return Err(AppError::msg("notify-control: not an app id"));
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{aumid}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| AppError::msg(format!("notify-control: could not open the app ({e})")))
}

/// Turn an app's own pop-ups (banners) on or off. Its notifications still
/// reach Action Center, so `notify` keeps seeing them.
pub fn banners(aumid: &str, on: bool) -> AppResult<()> {
    if !valid_aumid(aumid) {
        return Err(AppError::msg("notify-control: not an app id"));
    }
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD};
        let key = HSTRING::from(format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings\\{aumid}"
        ));
        let value: u32 = on.into();
        let r = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &key,
                windows::core::w!("ShowBanner"),
                REG_DWORD.0,
                Some(&value as *const u32 as *const _),
                4,
            )
        };
        if r.is_err() {
            return Err(AppError::msg(format!("notify-control: could not change banners ({r:?})")));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        Err(AppError::msg("notifications are only available on Windows"))
    }
}

pub fn read() -> AppResult<Value> {
    match imp::access() {
        Ok(access) if access == "allowed" => {
            let items = recent(imp::all()?, RECENT_LIMIT);
            Ok(json!({ "access": access, "items": items }))
        }
        Ok(access) => Ok(json!({ "access": access, "items": [] })),
        Err(err) => {
            tracing::debug!("notify: listener unavailable: {err}");
            Ok(json!({ "access": "unavailable", "items": [] }))
        }
    }
}

/// Poll state for pushing new toasts to pieces that declare `notify`.
#[derive(Debug, Default)]
pub struct Watch {
    /// `None` until the first poll, which only records what is already there.
    seen: Option<HashSet<u32>>,
}

impl Watch {
    /// New toasts since the last poll, oldest first. Empty on the first poll
    /// and whenever access is missing.
    pub fn poll(&mut self) -> Vec<Toast> {
        if !matches!(imp::access(), Ok("allowed")) {
            self.seen = None;
            return Vec::new();
        }
        let Ok(current) = imp::ids() else {
            return Vec::new();
        };
        let ids: HashSet<u32> = current.iter().map(|(id, _)| *id).collect();
        let fresh = match &self.seen {
            Some(seen) => unseen(seen, &current),
            None => Vec::new(),
        };
        self.seen = Some(ids);
        fresh.into_iter().filter_map(imp::one).collect()
    }

    /// Forget the baseline (nobody declares `notify` any more).
    pub fn reset(&mut self) {
        self.seen = None;
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::UI::Notifications::Management::UserNotificationListener;
    use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};

    fn err(e: windows::core::Error) -> AppError {
        AppError::msg(e.to_string())
    }

    pub fn access() -> AppResult<&'static str> {
        let listener = UserNotificationListener::Current().map_err(err)?;
        let status = listener.GetAccessStatus().map_err(err)?;
        Ok(access_name(status.0))
    }

    fn list() -> AppResult<Vec<UserNotification>> {
        let listener = UserNotificationListener::Current().map_err(err)?;
        let view = listener
            .GetNotificationsAsync(NotificationKinds::Toast)
            .map_err(err)?
            .get()
            .map_err(err)?;
        let n = view.Size().map_err(err)?;
        Ok((0..n).filter_map(|i| view.GetAt(i).ok()).collect())
    }

    fn convert(n: &UserNotification) -> Option<Toast> {
        let id = n.Id().ok()?;
        let time = n
            .CreationTime()
            .map(|t| ticks_to_unix_ms(t.UniversalTime))
            .unwrap_or(0);
        let info = n.AppInfo().ok();
        let app = info
            .as_ref()
            .and_then(|i| i.DisplayInfo().ok())
            .and_then(|d| d.DisplayName().ok())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        let aumid = info
            .as_ref()
            .and_then(|i| i.AppUserModelId().ok())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        let texts: Vec<String> = KnownNotificationBindings::ToastGeneric()
            .and_then(|name| n.Notification()?.Visual()?.GetBinding(&name))
            .and_then(|binding| binding.GetTextElements())
            .map(|els| {
                let count = els.Size().unwrap_or(0);
                (0..count)
                    .filter_map(|i| els.GetAt(i).ok())
                    .filter_map(|t| t.Text().ok())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let (title, body) = title_and_body(&texts);
        Some(Toast { id, app, aumid, title, body, time })
    }

    pub fn all() -> AppResult<Vec<Toast>> {
        Ok(list()?.iter().filter_map(convert).collect())
    }

    /// `(id, time)` of every toast: cheap enough to poll.
    pub fn ids() -> AppResult<Vec<(u32, i64)>> {
        Ok(list()?
            .iter()
            .filter_map(|n| {
                let id = n.Id().ok()?;
                let time = n.CreationTime().map(|t| ticks_to_unix_ms(t.UniversalTime)).unwrap_or(0);
                Some((id, time))
            })
            .collect())
    }

    pub fn one(id: u32) -> Option<Toast> {
        let listener = UserNotificationListener::Current().ok()?;
        convert(&listener.GetNotification(id).ok()?)
    }

    fn allowed() -> AppResult<UserNotificationListener> {
        let listener = UserNotificationListener::Current().map_err(err)?;
        let status = listener.GetAccessStatus().map_err(err)?;
        if access_name(status.0) != "allowed" {
            return Err(AppError::msg(
                "notify-control: Windows has not allowed access to notifications (Settings > Privacy & security > Notifications)",
            ));
        }
        Ok(listener)
    }

    pub fn dismiss(id: u32) -> AppResult<()> {
        let listener = allowed()?;
        if listener.GetNotification(id).is_err() {
            return Err(AppError::msg(format!("notify-control: no notification {id}")));
        }
        listener.RemoveNotification(id).map_err(err)
    }

    /// `ClearNotifications` leaves the list untouched for an unpackaged
    /// process (it answers OK), while removing one by one works: do both.
    pub fn clear() -> AppResult<()> {
        let listener = allowed()?;
        let _ = listener.ClearNotifications();
        let mut left = 0;
        for n in list()? {
            if let Ok(id) = n.Id() {
                if listener.RemoveNotification(id).is_err() {
                    left += 1;
                }
            }
        }
        if left > 0 {
            return Err(AppError::msg(format!("notify-control: {left} notification(s) could not be removed")));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    fn unavailable<T>() -> AppResult<T> {
        Err(AppError::msg("notifications are only available on Windows"))
    }
    pub fn access() -> AppResult<&'static str> {
        unavailable()
    }
    pub fn all() -> AppResult<Vec<Toast>> {
        unavailable()
    }
    pub fn ids() -> AppResult<Vec<(u32, i64)>> {
        unavailable()
    }
    pub fn one(_id: u32) -> Option<Toast> {
        None
    }
    pub fn dismiss(_id: u32) -> AppResult<()> {
        unavailable()
    }
    pub fn clear() -> AppResult<()> {
        unavailable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toast(id: u32, time: i64) -> Toast {
        Toast {
            id,
            app: Some("Mail".into()),
            aumid: None,
            title: format!("t{id}"),
            body: String::new(),
            time,
        }
    }

    /// `cargo test live_dismiss -- --ignored --nocapture`: dismisses the
    /// toasts titled "Versailles test" (raise one first).
    #[test]
    #[ignore]
    fn live_dismiss() {
        let items = imp::all().unwrap();
        let mine: Vec<_> = items.iter().filter(|t| t.title == "Versailles test").collect();
        println!("found {} test toast(s) of {}", mine.len(), items.len());
        for t in &mine {
            println!("dismiss {}: {:?}", t.id, dismiss(t.id));
        }
        let left = imp::all().unwrap().iter().filter(|t| t.title == "Versailles test").count();
        println!("test toasts left: {left}");
    }

    #[test]
    fn aumids_are_app_ids_only() {
        assert!(valid_aumid("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"));
        assert!(valid_aumid("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe"));
        assert!(!valid_aumid(""));
        assert!(!valid_aumid("a\"b"));
        assert!(!valid_aumid("x & calc"));
        assert!(!valid_aumid("..\\..\\x"));
    }

    #[test]
    fn access_names() {
        assert_eq!(access_name(0), "unspecified");
        assert_eq!(access_name(1), "allowed");
        assert_eq!(access_name(2), "denied");
        assert_eq!(access_name(9), "unspecified");
    }

    #[test]
    fn winrt_ticks_become_unix_ms() {
        assert_eq!(ticks_to_unix_ms(116_444_736_000_000_000), 0);
        // 2026-10-17T19:13:53Z
        let unix_ms = 1_792_264_433_000;
        assert_eq!(ticks_to_unix_ms(116_444_736_000_000_000 + unix_ms * 10_000), unix_ms);
    }

    #[test]
    fn first_text_is_the_title_the_rest_the_body() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(title_and_body(&s(&["Hi", "line 1", "", " line 2 "])), ("Hi".into(), "line 1\nline 2".into()));
        assert_eq!(title_and_body(&s(&["  ", "Only"])), ("Only".into(), String::new()));
        assert_eq!(title_and_body(&[]), (String::new(), String::new()));
    }

    #[test]
    fn recent_is_newest_first_and_capped() {
        let items = vec![toast(1, 10), toast(3, 30), toast(2, 20)];
        let ids: Vec<u32> = recent(items, 2).iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![3, 2]);
    }

    #[test]
    fn unseen_ids_come_oldest_first() {
        let seen: HashSet<u32> = [1, 2].into_iter().collect();
        assert_eq!(unseen(&seen, &[(1, 10), (5, 50), (4, 40), (2, 20)]), vec![4, 5]);
        assert!(unseen(&seen, &[(1, 10)]).is_empty());
    }

    /// Live probe of this machine's listener (prints shapes, never text):
    /// `cargo test --lib toasts::tests::live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_read_from_an_unpackaged_process() {
        let v = read().unwrap();
        let items = v["items"].as_array().cloned().unwrap_or_default();
        println!("access={} items={}", v["access"], items.len());
        for t in items.iter().take(3) {
            println!(
                "id={} app={} aumid={} titleLen={} bodyLen={} time={}",
                t["id"],
                t["app"].as_str().map_or(0, str::len),
                !t["aumid"].is_null(),
                t["title"].as_str().map_or(0, str::len),
                t["body"].as_str().map_or(0, str::len),
                t["time"],
            );
        }
        if let Some(id) = items.first().and_then(|t| t["id"].as_u64()) {
            // What a push uses for a new id.
            assert_eq!(imp::one(id as u32).map(|t| t.id), Some(id as u32));
        }
        let mut watch = Watch::default();
        assert!(watch.poll().is_empty(), "the first poll is a baseline");    }

    #[test]
    fn toast_json_shape() {
        let v = serde_json::to_value(toast(7, 1_000)).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["app"], "Mail");
        assert_eq!(v["aumid"], Value::Null);
        assert_eq!(v["title"], "t7");
        assert_eq!(v["time"], 1_000);
    }
}
