//! `bus` hook: messages between pieces on the page, relayed by the host, and
//! structured data handed to a spawnable by `spawn(id, data)`.
//!
//! Delivery is a script evaluated in each receiving window, not a Tauri event:
//! any webview may `listen` to any event, so events would reach pieces (and
//! folder widgets) that never declared `bus`.

use crate::error::{AppError, AppResult};
use crate::page::{PageCatalog, PageKind, DESKTOP_LABEL};
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
            PageKind::Spawnable => {
                let label = crate::window_manager::widget_label(&piece.id);
                if app.get_webview_window(&label).is_some() {
                    labels.push(label);
                }
            }
        }
    }
    if desktop && app.get_webview_window(DESKTOP_LABEL).is_some() {
        labels.push(DESKTOP_LABEL.to_string());
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

/// Script that hands one message to the SDK's dispatcher in that window.
fn deliver_script(message_json: &str) -> String {
    format!(
        "(function(m){{try{{if(typeof window.__VERSAILLES_BUS__==='function')window.__VERSAILLES_BUS__(m);}}catch(e){{}}}})({message_json});"
    )
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
    fn caps_payload_size() {
        let big = json!("x".repeat(MAX_DATA));
        assert!(spawn_data_script(&big).is_err());
        assert!(check_size(&json!({"ok": true}), "bus").is_ok());
    }
}
