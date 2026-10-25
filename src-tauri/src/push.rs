//! Host → page pushes: hook data (`media`, `battery`, …) and piece events
//! (`shown`, `hidden`, `idle`, …). Like the bus, delivery is a script run in
//! the receiving windows only, so a piece never sees data for a hook it did
//! not declare. The SDK dispatches `window.__VERSAILLES_PUSH__(push)`.

use crate::page::PageKind;
use crate::state::AppState;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
struct Push<'a> {
    name: &'a str,
    data: &'a Value,
    /// Set for an event meant for one piece; the SDK hands it only to that
    /// piece's subscribers (the wallpaper hosts several pieces).
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
}

fn script(push: &Push) -> Option<String> {
    let json = serde_json::to_string(push).ok()?;
    Some(crate::bus::frame_script("push", &json))
}

fn deliver(app: &AppHandle, labels: &[String], push: &Push) {
    let Some(js) = script(push) else {
        return;
    };
    for label in labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.eval(&js);
        }
    }
}

/// Hook data to every open window of a piece that declared `hook`.
pub fn to_hook(app: &AppHandle, hook: &str, data: &Value) {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let labels = crate::bus::hook_window_labels(app, &cat, hook);
    deliver(app, &labels, &Push { name: hook, data, id: None });
}

/// An event for one piece: its spawn window, or the wallpaper for a widget.
pub fn to_piece(app: &AppHandle, id: &str, name: &str, data: &Value) {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let Some(piece) = cat.get(id) else {
        return;
    };
    // A wallpaper widget may be on several monitors' wallpapers, and a bar
    // on every monitor has a window per monitor.
    let labels = match piece.kind {
        PageKind::Spawnable => crate::window_manager::piece_window_labels(app, &piece.id),
        PageKind::Widget => crate::window_manager::desktop_window_labels(app),
    };
    deliver(app, &labels, &Push { name, data, id: Some(&piece.id) });
    if name == "shown" {
        // Its sensors were not read while it was hidden: read them now.
        crate::presence::wake();
    }
}

/// An event for one window of piece `id` (e.g. one monitor copy of a bar).
pub fn to_window(app: &AppHandle, label: &str, id: &str, name: &str, data: &Value) {
    deliver(app, &[label.to_string()], &Push { name, data, id: Some(id) });
}

/// An event for every page window: the wallpaper and each open spawn window.
pub fn to_all(app: &AppHandle, name: &str, data: &Value) {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let mut labels = crate::window_manager::desktop_window_labels(app);
    labels.extend(
        cat.pieces
            .iter()
            .filter(|p| p.kind == PageKind::Spawnable)
            .flat_map(|p| crate::window_manager::piece_window_labels(app, &p.id)),
    );
    deliver(app, &labels, &Push { name, data, id: None });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn script_carries_name_data_and_optional_id() {
        let data = json!({"percent": 80});
        let js = script(&Push { name: "battery", data: &data, id: None }).unwrap();
        assert!(js.contains(r#"{"name":"battery","data":{"percent":80}}"#));
        let js = script(&Push { name: "shown", data: &Value::Null, id: Some("bar") }).unwrap();
        assert!(js.contains(r#""id":"bar""#));
    }
}
