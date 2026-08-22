//! Parse `desktop/index.html` for widgets and spawnables.

use crate::error::{AppError, AppResult};
use crate::config::{SpawnableConfig, UserConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const KNOWN_HOOKS: &[&str] = &["media", "mouse", "layout", "spawn", "shell", "hotkey", "pty"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageKind {
    Widget,
    Spawnable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagePiece {
    pub id: String,
    pub kind: PageKind,
    pub width: u32,
    pub height: u32,
    pub hooks: Vec<String>,
    pub anchor: Option<String>,
    /// Global accelerator when the piece lists the `hotkey` hook (`data-hotkey`).
    pub hotkey: Option<String>,
    /// `data-dismiss-on-blur="true|false"` — override blur auto-hide for overlays.
    pub dismiss_on_blur: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PageCatalog {
    pub pieces: Vec<PagePiece>,
}

impl PageCatalog {
    pub fn get(&self, id: &str) -> Option<&PagePiece> {
        let needle = id.trim().to_ascii_lowercase();
        self.pieces
            .iter()
            .find(|p| p.id.eq_ignore_ascii_case(&needle))
            .or_else(|| {
                // calc ↔ calculator
                if needle == "calc" {
                    self.pieces
                        .iter()
                        .find(|p| p.id.eq_ignore_ascii_case("calculator"))
                } else if needle == "calculator" {
                    self.pieces.iter().find(|p| p.id.eq_ignore_ascii_case("calc"))
                } else {
                    None
                }
            })
    }

    pub fn spawnable(&self, id: &str) -> Option<&PagePiece> {
        self.get(id).filter(|p| p.kind == PageKind::Spawnable)
    }

    pub fn is_desktop_widget(&self, id: &str) -> bool {
        self.get(id)
            .is_some_and(|p| p.kind == PageKind::Widget)
    }

    /// First spawnable that declared `data-hooks` including `hotkey`.
    pub fn hotkey_piece(&self) -> Option<&PagePiece> {
        self.pieces.iter().find(|p| {
            p.kind == PageKind::Spawnable && p.hooks.iter().any(|h| h == "hotkey")
        })
    }

    /// Resolve tray / show_launcher target: explicit primary id, else first hotkey piece.
    pub fn hotkey_piece_with_primary(&self, primary: Option<&str>) -> Option<&PagePiece> {
        if let Some(id) = primary.map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(p) = self.get(id) {
                if p.kind == PageKind::Spawnable && p.hooks.iter().any(|h| h == "hotkey") {
                    return Some(p);
                }
            }
        }
        self.hotkey_piece()
    }

    /// Global accelerators for spawnables that list the `hotkey` hook.
    /// `(accelerator, piece id)` in page order. Duplicate combos: first wins.
    /// A piece with no `data-hotkey` uses `fallback` (once), typically `#versailles` `launcher.hotkey`.
    pub fn hotkey_bindings(&self, fallback: &str) -> Vec<(String, String)> {
        let fallback = fallback.trim();
        let mut seen: HashSet<String> = HashSet::new();
        let mut out = Vec::new();
        let mut used_fallback = false;
        for piece in &self.pieces {
            if piece.kind != PageKind::Spawnable {
                continue;
            }
            if !piece.hooks.iter().any(|h| h == "hotkey") {
                continue;
            }
            let explicit = piece
                .hotkey
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let accel = if let Some(value) = explicit {
                value.to_string()
            } else if !used_fallback && !fallback.is_empty() {
                used_fallback = true;
                fallback.to_string()
            } else {
                continue;
            };
            let key = accel.to_ascii_lowercase();
            if !seen.insert(key) {
                continue;
            }
            out.push((accel, piece.id.clone()));
        }
        out
    }

    pub fn is_overlay(&self, id: &str) -> bool {
        self.spawnable(id).is_some_and(piece_is_overlay)
    }
}

/// Centered overlay (covers apps). `tr` and friends stay docked slide-outs.
pub fn piece_is_overlay(piece: &PagePiece) -> bool {
    matches!(
        piece
            .anchor
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref(),
        Some("c") | Some("center") | Some("tc") | Some("overlay")
    )
}

pub fn parse_page(html: &str) -> PageCatalog {
    let mut pieces = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &html[i..];
        let Some(end_rel) = rest.find('>') else {
            break;
        };
        let tag = &rest[..end_rel];
        i += end_rel + 1;
        if tag.starts_with('/') || tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        let tag_l = tag.to_ascii_lowercase();
        let is_widget = class_has(&tag_l, "widget");
        let is_spawn = class_has(&tag_l, "spawnable") || class_has(&tag_l, "action-bar");
        if !is_widget && !is_spawn {
            continue;
        }
        let Some(id) = attr(tag, "data-id") else {
            continue;
        };
        let id = id.trim().to_string();
        if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
            continue;
        }
        let kind = if is_spawn {
            PageKind::Spawnable
        } else {
            PageKind::Widget
        };
        let width = attr(tag, "data-w")
            .and_then(|v| v.parse().ok())
            .unwrap_or(280);
        let height = attr(tag, "data-h")
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        let hooks = attr(tag, "data-hooks")
            .map(|raw| {
                raw.split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let anchor = attr(tag, "data-anchor");
        let hotkey = attr(tag, "data-hotkey").filter(|s| !s.trim().is_empty());
        let dismiss_on_blur = attr(tag, "data-dismiss-on-blur").and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        });
        pieces.push(PagePiece {
            id,
            kind,
            width,
            height,
            hooks,
            anchor,
            hotkey,
            dismiss_on_blur,
        });
    }
    PageCatalog { pieces }
}

/// Host enforcement: unscoped invokes (launcher engine) are allowed.
/// A widget/spawnable `caller` must list the hook on `data-hooks`.
pub fn enforce_caller_hook(html: &str, caller: Option<&str>, hook: &str) -> AppResult<()> {
    let hook = hook.trim().to_ascii_lowercase();
    if !KNOWN_HOOKS.iter().any(|h| *h == hook) {
        return Err(AppError::msg(format!("Unknown hook '{hook}'")));
    }
    let Some(id) = caller.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let cat = parse_page(html);
    let Some(piece) = cat.get(id) else {
        return Err(AppError::msg(format!(
            "hook '{hook}' rejected: unknown page piece '{id}'"
        )));
    };
    if piece.hooks.iter().any(|h| h == &hook) {
        return Ok(());
    }
    Err(AppError::msg(format!(
        "hook '{hook}' not declared on '{id}'"
    )))
}

/// Best-effort ACL when the command only has an optional `caller`.
pub fn enforce_hook_from_disk(caller: Option<&str>, hook: &str) -> Result<(), String> {
    let Ok(root) = crate::registry::widgets_root() else {
        return Ok(());
    };
    let path = root.join("desktop").join("index.html");
    let Ok(page) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    enforce_caller_hook(&page, caller, hook).map_err(|e| e.to_string())
}

fn class_has(tag_lower: &str, token: &str) -> bool {
    let Some(val) = attr(tag_lower, "class") else {
        return false;
    };
    val.split_whitespace().any(|c| c == token)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let l = tag.to_ascii_lowercase();
    let idx = l.find(&needle.to_ascii_lowercase())?;
    let after = tag.get(idx + needle.len()..)?;
    let after = after.trim_start();
    let quote = after.chars().next()?;
    if quote == '"' || quote == '\'' {
        let rest = &after[1..];
        let end = rest.find(quote)?;
        Some(rest[..end].to_string())
    } else {
        let end = after
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(after.len());
        Some(after[..end].to_string())
    }
}

pub fn unknown_spawn(id: &str) -> AppError {
    AppError::msg(format!("Unknown spawnable '{id}'"))
}

fn spawnable_config<'a>(user: &'a UserConfig, piece_id: &str) -> Option<&'a SpawnableConfig> {
    let key = piece_id.trim().to_ascii_lowercase();
    user.spawnables.get(&key).or_else(|| user.spawnables.get(piece_id))
}

/// Whether an overlay spawnable should auto-hide when its window loses focus.
pub fn piece_dismiss_on_blur(piece: &PagePiece, user: &UserConfig) -> bool {
    if let Some(v) = piece.dismiss_on_blur {
        return v;
    }
    if let Some(cfg) = spawnable_config(user, &piece.id) {
        if let Some(overlay) = &cfg.overlay {
            if let Some(v) = overlay.dismiss_on_blur {
                return v;
            }
        }
    }
    // PTY spawnables keep sessions alive across blur by default.
    if piece.hooks.iter().any(|h| h == "pty") {
        return false;
    }
    true
}

/// Whether the spawn window background may start a native drag.
pub fn piece_draggable(piece: &PagePiece, user: &UserConfig) -> bool {
    if let Some(cfg) = spawnable_config(user, &piece.id) {
        if let Some(overlay) = &cfg.overlay {
            if let Some(v) = overlay.draggable {
                return v;
            }
        }
    }
    !piece_is_overlay(piece)
}

/// Inner JSON of `<script type="application/json" id="versailles">…</script>`.
/// The JSON must not contain the literal `</script>`.
pub fn extract_versailles_json(html: &str) -> Option<&str> {
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(id_rel) = lower[search_from..].find("id=") {
        let id_at = search_from + id_rel;
        let after = html.get(id_at + 3..).unwrap_or("").trim_start();
        let Some(quote) = after.chars().next() else {
            search_from = id_at + 3;
            continue;
        };
        if quote != '"' && quote != '\'' {
            search_from = id_at + 3;
            continue;
        }
        let rest = &after[1..];
        let Some(end) = rest.find(quote) else {
            search_from = id_at + 3;
            continue;
        };
        if !rest[..end].eq_ignore_ascii_case("versailles") {
            search_from = id_at + 3;
            continue;
        }
        let Some(script_rel) = lower[..id_at].rfind("<script") else {
            search_from = id_at + 3;
            continue;
        };
        let Some(tag_end_rel) = html[script_rel..].find('>') else {
            return None;
        };
        let inner_start = script_rel + tag_end_rel + 1;
        let Some(close_rel) = lower[inner_start..].find("</script>") else {
            return None;
        };
        return Some(html[inner_start..inner_start + close_rel].trim());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_widgets_and_spawnables() {
        let html = r#"
<section id="desktop">
  <article class="widget" data-id="clock" data-w="320" data-h="150">x</article>
  <article class="widget" data-id="now-playing" data-hooks="media">y</article>
</section>
<template class="spawnable" data-id="calc" data-w="280" data-h="420" data-anchor="tr"></template>
<template class="spawnable action-bar" data-id="action-bar" data-hooks="shell,hotkey" data-hotkey="Alt+Space" data-anchor="c" data-w="640" data-h="420"></template>
"#;
        let cat = parse_page(html);
        assert_eq!(cat.pieces.len(), 4);
        assert!(cat.is_desktop_widget("clock"));
        assert_eq!(cat.spawnable("calc").unwrap().width, 280);
        assert!(cat.get("calculator").is_some());
        assert_eq!(cat.get("action-bar").unwrap().kind, PageKind::Spawnable);
        assert!(cat.hotkey_piece().is_some());
        assert!(cat.is_overlay("action-bar"));
        assert_eq!(
            cat.get("action-bar").unwrap().hotkey.as_deref(),
            Some("Alt+Space")
        );
        assert!(cat
            .get("now-playing")
            .unwrap()
            .hooks
            .iter()
            .any(|h| h == "media"));
        assert_eq!(
            cat.hotkey_bindings("Alt+Space"),
            vec![("Alt+Space".into(), "action-bar".into())]
        );
        assert!(enforce_caller_hook(html, None, "media").is_ok());
        assert!(enforce_caller_hook(html, Some("now-playing"), "media").is_ok());
        assert!(enforce_caller_hook(html, Some("clock"), "media").is_err());
        assert!(enforce_caller_hook(html, Some("now-playing"), "shell").is_err());
    }

    #[test]
    fn extracts_versailles_json_block() {
        let html = r#"<html><head>
<script type="application/json" id="versailles">
{ "autostart": true, "api": { "port": 47831 } }
</script>
</head></html>"#;
        let json = extract_versailles_json(html).expect("block");
        assert!(json.contains("\"autostart\": true"));
        assert!(serde_json::from_str::<serde_json::Value>(json).is_ok());
    }

    #[test]
    fn extracts_versailles_json_missing() {
        let html = r#"<html><head><script type="application/json" id="other">{}</script></head></html>"#;
        assert!(extract_versailles_json(html).is_none());
        assert!(extract_versailles_json("").is_none());
    }

    #[test]
    fn binds_every_hotkey_spawnable() {
        let html = r#"
<template class="spawnable" data-id="draw" data-hooks="layout,hotkey" data-hotkey="Ctrl+Shift+D" data-anchor="tr"></template>
<template class="spawnable action-bar" data-id="action-bar" data-hooks="shell,hotkey" data-hotkey="Alt+Space" data-anchor="c"></template>
<template class="spawnable" data-id="notes" data-hooks="hotkey" data-anchor="tr"></template>
<article class="widget" data-id="clock" data-hooks="hotkey" data-hotkey="Ctrl+K"></article>
<template class="spawnable" data-id="calc" data-anchor="tr" data-hotkey="Ctrl+Shift+C"></template>
"#;
        let cat = parse_page(html);
        assert_eq!(
            cat.hotkey_bindings("Super+K"),
            vec![
                ("Ctrl+Shift+D".into(), "draw".into()),
                ("Alt+Space".into(), "action-bar".into()),
                ("Super+K".into(), "notes".into()),
            ]
        );
    }

    #[test]
    fn duplicate_hotkey_first_wins() {
        let html = r#"
<template class="spawnable" data-id="a" data-hooks="hotkey" data-hotkey="Alt+Space"></template>
<template class="spawnable" data-id="b" data-hooks="hotkey" data-hotkey="Alt+Space"></template>
<template class="spawnable" data-id="c" data-hooks="hotkey"></template>
"#;
        let cat = parse_page(html);
        assert_eq!(
            cat.hotkey_bindings("Alt+Space"),
            vec![("Alt+Space".into(), "a".into())]
        );
    }
}
