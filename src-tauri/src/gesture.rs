//! Whether a page window is inside a click or key press (SECURITY.md L5).
//!
//! The SDK checks `navigator.userActivation` before `power`, `windows-control`
//! and `tray.click`, but a page script can call the host directly. So the host
//! asks the page itself: every page window gets, before any page script runs,
//! capture listeners on `window` that note the time of each trusted click, tap
//! or key press (`isTrusted`: a script cannot dispatch one), and a global
//! function that says whether the last one was under 5 s ago (Chromium's own
//! transient activation lasts about as long). The function is non-writable and
//! non-configurable, and uses only references captured before the page ran, so
//! a page can call it (harmless) but cannot replace or fool it. The host calls
//! it with WebView2's `ExecuteScript`, whose answer comes back to the host only.
//!
//! Not `navigator.userActivation` itself: `ExecuteScript` runs as a user
//! gesture, so the page always looks activated while the host asks.

use std::sync::OnceLock;
use std::time::Duration;

use tauri::Webview;

use crate::error::{AppError, AppResult};

/// The function's global name, random per run so a page cannot predefine it.
fn name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| format!("__versailles_activation_{}", uuid::Uuid::new_v4().simple()))
}

/// Initialization script for page windows (the wallpaper, bars, overlays,
/// folder widgets). Runs before the page's own scripts on every document.
pub fn init_script() -> String {
    format!(
        r#"(function () {{
  try {{
    var now = performance.now.bind(performance);
    var last = -Infinity;
    var seen = function (e) {{ if (e.isTrusted) last = now(); }};
    ["keydown", "mousedown", "pointerdown", "pointerup", "touchend"].forEach(function (t) {{
      addEventListener(t, seen, true);
    }});
    Object.defineProperty(window, "{name}", {{ value: function () {{ return now() - last < 5000; }} }});
  }} catch (_) {{}}
}})();"#,
        name = name()
    )
}

/// The page in `webview` had a click, tap or key press in the last 5 s.
/// False when it cannot tell.
pub async fn active(webview: &Webview) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    // `window.` explicitly: a page cannot shadow `window`, and the property
    // itself cannot be redefined.
    let script = format!("window[\"{}\"]() === true", name());
    let sent = webview.with_webview(move |pw| {
        let run = move || -> windows::core::Result<()> {
            let core = unsafe { pw.controller().CoreWebView2() }?;
            let handler = webview2_com::ExecuteScriptCompletedHandler::create(Box::new(move |result, json: String| {
                let _ = tx.send(result.is_ok() && json == "true");
                Ok(())
            }));
            unsafe { core.ExecuteScript(&windows::core::HSTRING::from(script), &handler) }
        };
        if let Err(err) = run() {
            tracing::warn!("gesture: could not ask the page: {err}");
        }
    });
    if let Err(err) = sent {
        tracing::warn!("gesture: no webview: {err}");
        return false;
    }
    matches!(tokio::time::timeout(Duration::from_secs(2), rx).await, Ok(Ok(true)))
}

/// Refuse `what` unless the calling page is inside a click or key press. The
/// message ends like the SDK's own, so it gets the `needs-gesture` code.
pub async fn require(webview: &Webview, what: &str) -> AppResult<()> {
    if active(webview).await {
        Ok(())
    } else {
        tracing::warn!("{}: {what} refused: no click or key press", webview.label());
        Err(AppError::msg(format!("{what} needs a click or key press")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_script_defines_one_frozen_function() {
        let s = init_script();
        assert!(s.contains(name()));
        assert!(name().starts_with("__versailles_activation_") && name().len() > 30);
        // No writable / configurable / enumerable flags: the defaults are false.
        assert!(!s.contains("writable") && !s.contains("configurable"));
    }
}
