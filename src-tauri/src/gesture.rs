//! Whether a page window is inside a click or key press (SECURITY.md L5).
//!
//! The SDK checks `navigator.userActivation` before `power`, `windows-control`
//! and `tray.click`, but a page script can call the host directly. So the host
//! asks the page's own Chromium: every page window gets, before any page
//! script runs, a global function that reads the real activation through
//! getters captured at that moment. It is non-writable and non-configurable,
//! so a page can call it (harmless) but cannot replace it or the getters it
//! uses. The host calls it with WebView2's `ExecuteScript`, whose answer comes
//! back to the host only.

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
    var ua = navigator.userActivation;
    var get = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(ua), "isActive").get;
    var apply = Reflect.apply;
    Object.defineProperty(window, "{name}", {{ value: function () {{ return apply(get, ua, []) === true; }} }});
  }} catch (_) {{}}
}})();"#,
        name = name()
    )
}

/// The page in `webview` had a click or key press in the last few seconds
/// (Chromium's transient activation, about 5 s). False when it cannot tell.
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
