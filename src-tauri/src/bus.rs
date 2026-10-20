//! `bus` hook: messages between pieces on the page, relayed by the host, and
//! structured data handed to a spawnable by `spawn(id, data)`.
//!
//! Delivery is a script evaluated in each receiving window, not a Tauri event:
//! any webview may `listen` to any event, so events would reach pieces (and
//! folder widgets) that never declared `bus`.

use crate::error::{AppError, AppResult};
use crate::page::{PageCatalog, PageKind};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

pub const MAX_TOPIC: usize = 128;
/// Serialized payload cap for a bus message or spawn data.
pub const MAX_DATA: usize = 256 * 1024;

/// Labels of open windows hosting pieces that declared `hook`. The wallpaper
/// hosts every `article.widget`, so it counts once.
pub fn hook_window_labels(app: &AppHandle, cat: &PageCatalog, hook: &str) -> Vec<String> {
    let mut labels = Vec::new();
    let mut desktop = false;
    for piece in &cat.pieces {
        if !piece.hooks.iter().any(|h| h == hook) {
            continue;
        }
        match piece.kind {
            PageKind::Widget => desktop = true,
            // `widget-<id>` plus any monitor copies (`widget-<id>:<n>`).
            PageKind::Spawnable => labels.extend(crate::window_manager::piece_window_labels(app, &piece.id)),
        }
    }
    if desktop {
        labels.extend(crate::window_manager::desktop_window_labels(app));
    }
    labels
}

#[derive(Serialize)]
struct BusMessage<'a> {
    topic: &'a str,
    data: &'a Value,
    from: &'a str,
}

fn check_size(data: &Value, what: &str) -> AppResult<String> {
    let json = serde_json::to_string(data)?;
    if json.len() > MAX_DATA {
        return Err(AppError::msg(format!(
            "{what}: data exceeds {} KB",
            MAX_DATA / 1024
        )));
    }
    Ok(json)
}

/// Script that hands `payload_json` to the SDK dispatcher
/// `window.__VERSAILLES_<KIND>__` in a window. The wallpaper window is a host
/// shell (`desktop.html`) whose page runs in a cross-origin `iframe#page`, so
/// when the dispatcher is missing the script forwards the payload there with
/// `postMessage`, addressed to the iframe's origin. The SDK only accepts
/// those messages from `window.parent`.
pub fn frame_script(kind: &str, payload_json: &str) -> String {
    let fn_name = format!("__VERSAILLES_{}__", kind.to_ascii_uppercase());
    format!(
        "(function(m){{try{{var f=window.{fn_name};if(typeof f==='function'){{f(m);return;}}var p=document.getElementById('page');if(p&&p.contentWindow&&p.src)p.contentWindow.postMessage({{__versailles:{{kind:'{kind}',payload:m}}}},new URL(p.src).origin);}}catch(e){{}}}})({payload_json});"
    )
}

/// Script that hands one message to the SDK's bus dispatcher in that window.
fn deliver_script(message_json: &str) -> String {
    frame_script("bus", message_json)
}

/// Relay `data` on `topic` from piece `from` to every open window of a piece
/// that declared `bus`. Returns how many windows it reached; windows that are
/// not open miss the message.
pub fn publish(
    app: &AppHandle,
    cat: &PageCatalog,
    from: &str,
    topic: &str,
    data: &Value,
) -> AppResult<usize> {
    let topic = topic.trim();
    if topic.is_empty() || topic.len() > MAX_TOPIC {
        return Err(AppError::msg(format!("bus: topic must be 1–{MAX_TOPIC} bytes")));
    }
    check_size(data, "bus")?;
    let message = serde_json::to_string(&BusMessage { topic, data, from })?;
    let script = deliver_script(&message);
    let mut reached = 0;
    for label in hook_window_labels(app, cat, "bus") {
        if let Some(window) = app.get_webview_window(&label) {
            if window.eval(&script).is_ok() {
                reached += 1;
            }
        }
    }
    Ok(reached)
}

/// Script that hands spawn data to a spawnable: `window.__VERSAILLES_SPAWN_DATA__`
/// and a `versailles:spawn` window event. Safe as an initialization script.
pub fn spawn_data_script(data: &Value) -> AppResult<String> {
    let json = check_size(data, "spawn")?;
    Ok(format!(
        "(function(d){{try{{window.__VERSAILLES_SPAWN_DATA__=d;window.dispatchEvent(new CustomEvent('versailles:spawn',{{detail:d}}));}}catch(e){{}}}})({json});"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scripts_embed_the_payload_as_json() {
        let msg = serde_json::to_string(&BusMessage {
            topic: "now",
            data: &json!({"title": "a\"b</script>"}),
            from: "player",
        })
        .unwrap();
        let js = deliver_script(&msg);
        assert!(js.contains(r#""topic":"now""#));
        assert!(js.contains(r#""from":"player""#));
        assert!(js.ends_with(");"));
        let js = spawn_data_script(&json!({"q": 1})).unwrap();
        assert!(js.contains(r#"({"q":1})"#));
    }

    #[test]
    fn frame_script_calls_the_dispatcher_or_forwards_to_the_page_iframe() {
        let js = frame_script("push", r#"{"name":"battery"}"#);
        assert!(js.contains("window.__VERSAILLES_PUSH__"));
        assert!(js.contains("getElementById('page')"));
        assert!(js.contains("kind:'push'"));
        assert!(js.contains("new URL(p.src).origin"));
        assert!(js.ends_with(r#"({"name":"battery"});"#));
        assert!(deliver_script("{}").contains("window.__VERSAILLES_BUS__"));
    }

    #[test]
    fn caps_payload_size() {
        let big = json!("x".repeat(MAX_DATA));
        assert!(spawn_data_script(&big).is_err());
        assert!(check_size(&json!({"ok": true}), "bus").is_ok());
    }
}
