//! Parse `desktop/index.html` for widgets, spawnables, and the action bar.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const KNOWN_HOOKS: &[&str] = &["media", "mouse", "layout", "spawn", "shell", "hotkey", "pty"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageKind {
    Widget,
    Spawnable,
    ActionBar,
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
        self.get(id).filter(|p| {
            matches!(p.kind, PageKind::Spawnable | PageKind::ActionBar)
        })
    }

    pub fn is_desktop_widget(&self, id: &str) -> bool {
        self.get(id)
            .is_some_and(|p| p.kind == PageKind::Widget)
    }
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
        let is_spawn = class_has(&tag_l, "spawnable");
        let is_bar = class_has(&tag_l, "action-bar");
        if !is_widget && !is_spawn && !is_bar {
            continue;
        }
        let Some(id) = attr(tag, "data-id") else {
            continue;
        };
        let id = id.trim().to_string();
        if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
            continue;
        }
        let kind = if is_bar || (is_spawn && id.eq_ignore_ascii_case("action-bar")) {
            PageKind::ActionBar
        } else if is_spawn {
            PageKind::Spawnable
        } else {
            PageKind::Widget
        };
        let width = attr(tag, "data-w")
            .and_then(|v| v.parse().ok())
            .unwrap_or(if kind == PageKind::ActionBar { 640 } else { 280 });
        let height = attr(tag, "data-h")
            .and_then(|v| v.parse().ok())
            .unwrap_or(if kind == PageKind::ActionBar { 420 } else { 200 });
        let hooks = attr(tag, "data-hooks")
            .map(|raw| {
                raw.split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let anchor = attr(tag, "data-anchor");
        pieces.push(PagePiece {
            id,
            kind,
            width,
            height,
            hooks,
            anchor,
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
<template class="spawnable action-bar" data-id="action-bar" data-hooks="shell,hotkey" data-w="640" data-h="420"></template>
"#;
        let cat = parse_page(html);
        assert_eq!(cat.pieces.len(), 4);
        assert!(cat.is_desktop_widget("clock"));
        assert_eq!(cat.spawnable("calc").unwrap().width, 280);
        assert!(cat.get("calculator").is_some());
        assert_eq!(cat.get("action-bar").unwrap().kind, PageKind::ActionBar);
        assert!(cat
            .get("now-playing")
            .unwrap()
            .hooks
            .iter()
            .any(|h| h == "media"));
        assert!(enforce_caller_hook(html, None, "media").is_ok());
        assert!(enforce_caller_hook(html, Some("now-playing"), "media").is_ok());
        assert!(enforce_caller_hook(html, Some("clock"), "media").is_err());
        assert!(enforce_caller_hook(html, Some("now-playing"), "shell").is_err());
    }
}
