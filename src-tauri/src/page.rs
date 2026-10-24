//! Parse `desktop/index.html` for widgets and spawnables.

use crate::error::{AppError, AppResult};
use crate::config::{DimSetting, SpawnableConfig, UserConfig};
use crate::placement::{self, Align, Anchor, DimMode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const KNOWN_HOOKS: &[&str] = &[
    "media", "mouse", "layout", "spawn", "shell", "hotkey", "pty", "storage", "clipboard", "power", "bus",
    "battery", "network", "focus", "audio", "windows", "windows-control", "notify",
    "radios", "audio-control", "display", "power-mode", "wifi", "bluetooth",
    "dnd", "notify-control", "tray", "posture",
];

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
    /// `data-offset="x,y"` — logical inset from the overlay anchor.
    #[serde(default)]
    pub offset: Option<(i32, i32)>,
    /// `data-dim="host|page|none"` — overlay backdrop.
    #[serde(default)]
    pub dim: Option<String>,
    /// `data-monitor="primary|all|cursor|<n>"` — which monitor(s) the piece is on.
    #[serde(default)]
    pub monitor: Option<String>,
    /// `data-dock="reserve|float|auto-hide"` — docked bars (`data-anchor` top / bottom / left / right).
    #[serde(default)]
    pub dock: Option<String>,
    /// `data-peek="<px>"` — what an auto-hide bar leaves showing.
    #[serde(default)]
    pub peek: Option<u32>,
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
    /// `(accelerator, piece id)` in page order. `data-hotkey` may list several,
    /// comma-separated (`"Alt+Space, Win"`). Duplicate combos: first wins.
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
            let accels = if let Some(value) = explicit {
                split_accelerators(value)
            } else if !used_fallback && !fallback.is_empty() {
                used_fallback = true;
                vec![fallback.to_string()]
            } else {
                continue;
            };
            for accel in accels {
                if seen.insert(accel.to_ascii_lowercase()) {
                    out.push((accel, piece.id.clone()));
                }
            }
        }
        out
    }

    pub fn is_overlay(&self, id: &str) -> bool {
        self.spawnable(id).is_some_and(piece_is_overlay)
    }
}

/// Overlay (covers apps): any point of the anchor grid or `cursor`.
/// `tr`, no anchor, or an unknown one stays a docked slide-out.
pub fn piece_is_overlay(piece: &PagePiece) -> bool {
    piece_anchor(piece).is_some()
}

pub fn piece_anchor(piece: &PagePiece) -> Option<Anchor> {
    placement::parse_anchor(piece.anchor.as_deref())
}

/// Overlays other than `c` / `center` / `overlay` hand their layout to the
/// page after the first open: the host keeps it on later reveals.
pub fn piece_keeps_layout(piece: &PagePiece) -> bool {
    piece_is_overlay(piece) && !placement::is_centered(piece.anchor.as_deref())
}

/// Grid axes the host measures a kept layout from (`cursor` measures from the
/// top-left; it is re-placed at the cursor anyway).
pub fn piece_layout_axes(piece: &PagePiece) -> (Align, Align) {
    match piece_anchor(piece) {
        Some(Anchor::Grid(h, v)) => (h, v),
        _ => (Align::Start, Align::Start),
    }
}

/// Logical inset from the anchor: `data-offset`, else the anchor's default.
pub fn piece_offset(piece: &PagePiece) -> (i32, i32) {
    piece
        .offset
        .unwrap_or_else(|| placement::default_offset(piece.anchor.as_deref()))
}

pub fn parse_page(html: &str) -> PageCatalog {
    crate::boot::count_parse_page();
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
        // A commented-out piece is not on the page (the browser skips it too).
        if rest.starts_with("<!--") {
            i += match rest[4..].find("-->") {
                Some(end) => 4 + end + 3,
                // Unterminated (e.g. inside a script string): skip the marker only.
                None => 4,
            };
            continue;
        }
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
        let offset = placement::parse_offset(attr(tag, "data-offset").as_deref());
        let dim = attr(tag, "data-dim").filter(|s| !s.trim().is_empty());
        let monitor = attr(tag, "data-monitor").filter(|s| !s.trim().is_empty());
        let dock = attr(tag, "data-dock").filter(|s| !s.trim().is_empty());
        let peek = attr(tag, "data-peek").and_then(|v| v.trim().parse().ok());
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
            offset,
            dim,
            monitor,
            dock,
            peek,
        });
    }
    PageCatalog { pieces }
}

/// Label of the primary monitor's wallpaper window (one document hosting every
/// piece on the page). Other monitors' wallpapers are `desktop-<n>`.
pub const DESKTOP_LABEL: &str = "desktop";

/// Whether `label` is a wallpaper window (`desktop` or `desktop-<n>`).
pub fn is_desktop_label(label: &str) -> bool {
    label == DESKTOP_LABEL
        || label
            .strip_prefix("desktop-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Who invoked a command, derived from the calling webview's label — never
/// from a self-declared IPC argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invoker {
    /// `widget-<id>` window bound to a page piece; `spawn` = spawnable template.
    Piece { id: String, spawn: bool },
    /// `widget-<id>` window for a registry folder widget; hooks come from its
    /// `widget.json` permissions.
    Folder { id: String, hooks: Vec<String> },
    /// The desktop wallpaper.
    Desktop,
    /// Host chrome (`launcher-dim`, `guides`, prewarm) or an unknown label.
    Host,
}

/// `Exec` commands run code or drive a shell (`cli_exec*`, `cli_search_files`,
/// `pty_*`); `Control` commands change a system setting (`quick_set`). Both
/// are refused from the wallpaper and from folder widgets, only allowed from
/// a spawn window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookClass {
    Plain,
    Exec,
    Control,
}

/// Map a webview label to an [`Invoker`]. `folder_hooks` resolves registry
/// widgets that are not page pieces.
pub fn classify_invoker(
    label: &str,
    cat: &PageCatalog,
    folder_hooks: impl FnOnce(&str) -> Option<Vec<String>>,
) -> Invoker {
    if is_desktop_label(label) {
        return Invoker::Desktop;
    }
    let Some(id) = label.strip_prefix("widget-").filter(|s| !s.is_empty()) else {
        return Invoker::Host;
    };
    if let Some(piece) = cat.get(id) {
        return Invoker::Piece {
            id: piece.id.clone(),
            spawn: piece.kind == PageKind::Spawnable,
        };
    }
    // `widget-<id>:<n>`: the copy of a `data-monitor="all"` bar on display n
    // acts as the same piece.
    if let Some((base, _)) = split_monitor_copy(id) {
        if let Some(piece) = cat.spawnable(base).filter(|p| crate::docks::on_every_monitor(p)) {
            return Invoker::Piece {
                id: piece.id.clone(),
                spawn: true,
            };
        }
    }
    match folder_hooks(id) {
        Some(hooks) => Invoker::Folder {
            id: id.to_string(),
            hooks,
        },
        None => Invoker::Host,
    }
}

/// `taskbar:2` → `("taskbar", 2)`: the part of a window label after `widget-`
/// for a bar's copy on display 2. `None` without a `:<number>` suffix.
pub fn split_monitor_copy(rest: &str) -> Option<(&str, u32)> {
    let (base, n) = rest.rsplit_once(':')?;
    if base.is_empty() || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((base, n.parse().ok().filter(|n| *n > 0)?))
}

/// Piece id a `widget-…` window label belongs to: `widget-<id>` or a
/// monitor copy `widget-<id>:<n>`. Matches ids as written in the label.
pub fn label_piece_id(label: &str) -> Option<&str> {
    let rest = label.strip_prefix("widget-").filter(|s| !s.is_empty())?;
    Some(split_monitor_copy(rest).map(|(base, _)| base).unwrap_or(rest))
}

/// Host enforcement. Returns the piece id the call acts as.
///
/// - Spawn / piece window: acts as its own piece; `caller` is ignored.
/// - Desktop wallpaper: may claim any piece on the page via `caller`; a missing
///   caller is rejected. `Exec` hooks are never allowed from the wallpaper.
/// - Folder widget: its `widget.json` permissions; never `Exec`.
/// - Host windows: no hooks.
pub fn authorize_hook(
    cat: &PageCatalog,
    invoker: &Invoker,
    caller: Option<&str>,
    hook: &str,
    class: HookClass,
) -> AppResult<String> {
    let hook = hook.trim().to_ascii_lowercase();
    if !KNOWN_HOOKS.iter().any(|h| *h == hook) {
        return Err(AppError::msg(format!("Unknown hook '{hook}'")));
    }
    let piece = match invoker {
        Invoker::Piece { id, spawn } => {
            if class != HookClass::Plain && !spawn {
                return Err(AppError::msg(format!(
                    "hook '{hook}' rejected: '{id}' is not a spawn window"
                )));
            }
            cat.get(id)
                .ok_or_else(|| AppError::msg(format!("hook '{hook}' rejected: unknown page piece '{id}'")))?
        }
        Invoker::Folder { id, hooks } => {
            if class == HookClass::Exec {
                return Err(AppError::msg(format!(
                    "hook '{hook}' rejected: folder widget '{id}' cannot run commands"
                )));
            }
            if class == HookClass::Control {
                return Err(AppError::msg(format!(
                    "hook '{hook}' rejected: folder widget '{id}' cannot change settings"
                )));
            }
            if hooks.iter().any(|h| *h == hook) {
                return Ok(id.clone());
            }
            return Err(AppError::msg(format!("hook '{hook}' not declared on '{id}'")));
        }
        Invoker::Desktop => {
            if class == HookClass::Exec {
                return Err(AppError::msg(format!(
                    "hook '{hook}' rejected: commands cannot run from the desktop wallpaper"
                )));
            }
            if class == HookClass::Control {
                return Err(AppError::msg(format!(
                    "hook '{hook}' rejected: settings cannot change from the desktop wallpaper (use a bar or overlay)"
                )));
            }
            let Some(id) = caller.map(str::trim).filter(|s| !s.is_empty()) else {
                return Err(AppError::msg(format!(
                    "hook '{hook}' rejected: the desktop page must name its piece (versailles.for(id))"
                )));
            };
            cat.get(id)
                .ok_or_else(|| AppError::msg(format!("hook '{hook}' rejected: unknown page piece '{id}'")))?
        }
        Invoker::Host => {
            return Err(AppError::msg(format!(
                "hook '{hook}' rejected: host window has no page piece"
            )));
        }
    };
    if piece.hooks.iter().any(|h| *h == hook) {
        return Ok(piece.id.clone());
    }
    Err(AppError::msg(format!(
        "hook '{hook}' not declared on '{}'",
        piece.id
    )))
}

/// `true` when `target` names the same piece as `acting` (case / calc alias aware).
pub fn same_piece(cat: &PageCatalog, acting: &str, target: &str) -> bool {
    let resolve = |id: &str| {
        cat.get(id)
            .map(|p| p.id.to_ascii_lowercase())
            .unwrap_or_else(|| id.trim().to_ascii_lowercase())
    };
    resolve(acting) == resolve(target)
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

/// Backdrop of an overlay: `data-dim` beats `spawnables.<id>.overlay.dim`;
/// default is the host dim. Slide-outs never dim.
pub fn piece_dim(piece: &PagePiece, user: &UserConfig) -> DimMode {
    if !piece_is_overlay(piece) {
        return DimMode::None;
    }
    if let Some(mode) = piece.dim.as_deref().and_then(placement::parse_dim) {
        return mode;
    }
    let configured = spawnable_config(user, &piece.id)
        .and_then(|cfg| cfg.overlay.as_ref())
        .and_then(|overlay| overlay.dim.as_ref());
    match configured {
        Some(DimSetting::Flag(true)) => DimMode::Host,
        Some(DimSetting::Flag(false)) => DimMode::None,
        Some(DimSetting::Mode(raw)) => placement::parse_dim(raw).unwrap_or_default(),
        None => DimMode::Host,
    }
}

/// `spawnables.<id>.opts.coverMonitor` on a `tc` overlay: the page (the
/// example bar) makes its window cover the monitor and lays its chrome out
/// inside, marking `html[data-stage]` when it has.
pub fn piece_cover_monitor(piece: &PagePiece, user: &UserConfig) -> bool {
    piece_anchor(piece) == Some(Anchor::Grid(Align::Center, Align::Start))
        && spawnable_config(user, &piece.id)
            .and_then(|cfg| cfg.opts.as_ref())
            .and_then(|opts| opts.cover_monitor)
            .unwrap_or(false)
}

/// Whether an overlay's window covers its monitor: `page` dim, or
/// [`piece_cover_monitor`].
pub fn piece_covers_monitor(piece: &PagePiece, user: &UserConfig) -> bool {
    piece_dim(piece, user) == DimMode::Page || piece_cover_monitor(piece, user)
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
    // Slide-outs drag by default; overlays and docked bars are placed by the host.
    !piece_is_overlay(piece) && crate::docks::piece_dock(piece).is_none()
}

fn find_ignore_ascii_case(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    h.windows(n.len()).position(|w| w.eq_ignore_ascii_case(n))
}

fn rfind_ignore_ascii_case(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    h.windows(n.len())
        .rposition(|w| w.eq_ignore_ascii_case(n))
}

/// Inner JSON of `<script type="application/json" id="versailles">…</script>`.
/// The JSON must not contain the literal `</script>`.
/// Scans in place — does not allocate a lowercased copy of `html`.
pub fn extract_versailles_json(html: &str) -> Option<&str> {
    let mut search_from = 0;
    while let Some(id_rel) = find_ignore_ascii_case(&html[search_from..], "id=") {
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
        let Some(script_rel) = rfind_ignore_ascii_case(&html[..id_at], "<script") else {
            search_from = id_at + 3;
            continue;
        };
        let Some(tag_end_rel) = html[script_rel..].find('>') else {
            return None;
        };
        let inner_start = script_rel + tag_end_rel + 1;
        let Some(close_rel) = find_ignore_ascii_case(&html[inner_start..], "</script>") else {
            return None;
        };
        return Some(html[inner_start..inner_start + close_rel].trim());
    }
    None
}

/// `"Alt+Space, Win"` → both. A comma right after `+` is the key itself
/// (`"Ctrl+,"`), not a separator.
fn split_accelerators(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in value.chars() {
        if ch == ',' && !cur.trim_end().ends_with('+') {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
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
        assert!(!piece_keeps_layout(cat.get("action-bar").unwrap()));
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
        let desk = Invoker::Desktop;
        let plain = HookClass::Plain;
        assert!(authorize_hook(&cat, &desk, None, "media", plain).is_err());
        assert!(authorize_hook(&cat, &desk, Some("now-playing"), "media", plain).is_ok());
        assert!(authorize_hook(&cat, &desk, Some("clock"), "media", plain).is_err());
        assert!(authorize_hook(&cat, &desk, Some("now-playing"), "shell", plain).is_err());
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
    fn centered_overlays_do_not_keep_layout() {
        let html = r#"
<template class="spawnable" data-id="draw" data-anchor="c"></template>
<template class="spawnable action-bar" data-id="action-bar" data-anchor="tc"></template>
<template class="spawnable" data-id="toast" data-anchor="br" data-offset="16,24"></template>
<template class="spawnable" data-id="notes" data-anchor="tr"></template>
"#;
        let cat = parse_page(html);
        assert!(cat.is_overlay("draw"));
        assert!(cat.is_overlay("action-bar"));
        assert!(cat.is_overlay("toast"));
        assert!(!cat.is_overlay("notes"));
        assert!(!piece_keeps_layout(cat.spawnable("draw").unwrap()));
        assert!(piece_keeps_layout(cat.spawnable("action-bar").unwrap()));
        assert!(piece_keeps_layout(cat.spawnable("toast").unwrap()));
        assert_eq!(piece_offset(cat.spawnable("action-bar").unwrap()), (0, 120));
        assert_eq!(piece_offset(cat.spawnable("toast").unwrap()), (16, 24));
        assert_eq!(
            piece_layout_axes(cat.spawnable("toast").unwrap()),
            (Align::End, Align::End)
        );
    }

    #[test]
    fn dim_attribute_beats_config() {
        let html = r#"
<template class="spawnable" data-id="a" data-anchor="c" data-dim="page"></template>
<template class="spawnable" data-id="b" data-anchor="c"></template>
<template class="spawnable" data-id="c" data-anchor="c"></template>
<template class="spawnable" data-id="d" data-anchor="tr" data-dim="page"></template>
"#;
        let cat = parse_page(html);
        let user: UserConfig = serde_json::from_str(
            r#"{ "spawnables": { "a": { "overlay": { "dim": false } }, "b": { "overlay": { "dim": "none" } } } }"#,
        )
        .unwrap();
        assert_eq!(piece_dim(cat.spawnable("a").unwrap(), &user), DimMode::Page);
        assert_eq!(piece_dim(cat.spawnable("b").unwrap(), &user), DimMode::None);
        assert_eq!(piece_dim(cat.spawnable("c").unwrap(), &user), DimMode::Host);
        // Slide-outs never dim.
        assert_eq!(piece_dim(cat.spawnable("d").unwrap(), &user), DimMode::None);
    }

    #[test]
    fn cover_monitor_is_a_tc_opt_or_page_dim() {
        let html = r#"
<template class="spawnable action-bar" data-id="action-bar" data-anchor="tc"></template>
<template class="spawnable" data-id="draw" data-anchor="c"></template>
<template class="spawnable" data-id="plain" data-anchor="tc"></template>
<template class="spawnable" data-id="spot" data-anchor="c" data-dim="page"></template>
"#;
        let cat = parse_page(html);
        let user: UserConfig = serde_json::from_str(
            r#"{ "spawnables": {
                "action-bar": { "opts": { "coverMonitor": true } },
                "draw": { "opts": { "coverMonitor": true } },
                "plain": { "opts": { "coverMonitor": false } }
            } }"#,
        )
        .unwrap();
        let piece = |id| cat.spawnable(id).unwrap();
        assert!(piece_cover_monitor(piece("action-bar"), &user));
        // The opt only means something on a `tc` piece.
        assert!(!piece_cover_monitor(piece("draw"), &user));
        assert!(!piece_cover_monitor(piece("plain"), &user));
        assert!(!piece_cover_monitor(piece("spot"), &user));
        assert!(piece_covers_monitor(piece("action-bar"), &user));
        assert!(piece_covers_monitor(piece("spot"), &user));
        assert!(!piece_covers_monitor(piece("draw"), &user));
        assert!(!piece_covers_monitor(piece("action-bar"), &UserConfig::default()));
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
    fn data_hotkey_lists_several() {
        assert_eq!(split_accelerators("Alt+Space, Win,Copilot"), vec!["Alt+Space", "Win", "Copilot"]);
        assert_eq!(split_accelerators("Ctrl+,, Win"), vec!["Ctrl+,", "Win"]);
        assert_eq!(split_accelerators(" Alt+Space "), vec!["Alt+Space"]);
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

    #[test]
    fn extracts_versailles_json_case_insensitive_id() {
        let html = r#"<html><head>
<SCRIPT type="application/json" ID="Versailles">
{ "autostart": false }
</SCRIPT>
</head></html>"#;
        let json = extract_versailles_json(html).expect("block");
        assert!(json.contains("\"autostart\": false"));
    }

    fn bulky_html(widgets: usize, pad: usize) -> String {
        let mut s = String::with_capacity(widgets * (pad + 80) + 256);
        s.push_str(
            r#"<html><head><script type="application/json" id="versailles">{ "autostart": false }</script></head><body>"#,
        );
        let blob = "x".repeat(pad);
        for i in 0..widgets {
            s.push_str(&format!(
                r#"<article class="widget" data-id="w{i}" data-hooks="media">{blob}</article>"#
            ));
        }
        s.push_str(
            r#"<template class="spawnable" data-id="action-bar" data-hooks="shell,hotkey" data-hotkey="Alt+Space"></template></body></html>"#,
        );
        s
    }

    #[test]
    fn parse_page_and_extract_hold_on_large_html() {
        let html = bulky_html(400, 500);
        assert!(html.len() > 200_000, "fixture {} bytes", html.len());
        let json = extract_versailles_json(&html).expect("json");
        assert!(json.contains("autostart"));
        let start = std::time::Instant::now();
        let mut last = None;
        for _ in 0..50 {
            last = Some(parse_page(&html));
        }
        let ms = start.elapsed().as_millis();
        eprintln!("parse_page x50 on {} bytes = {ms}ms", html.len());
        let cat = last.expect("cat");
        assert_eq!(cat.pieces.len(), 401);
        assert!(ms < 5_000, "parse_page x50 took {ms}ms");
    }

    #[test]
    fn cached_acl_is_cheaper_than_reparse() {
        let html = bulky_html(400, 500);
        let cat = parse_page(&html);
        let cached_start = std::time::Instant::now();
        let desk = Invoker::Desktop;
        for _ in 0..1_000 {
            assert!(authorize_hook(&cat, &desk, Some("w0"), "media", HookClass::Plain).is_ok());
        }
        let cached = cached_start.elapsed();
        let parsed_start = std::time::Instant::now();
        for _ in 0..1_000 {
            let cat = parse_page(&html);
            assert!(authorize_hook(&cat, &desk, Some("w0"), "media", HookClass::Plain).is_ok());
        }
        let parsed = parsed_start.elapsed();
        eprintln!("acl cached={cached:?} reparse={parsed:?}");
        assert!(
            cached.as_nanos() * 10 < parsed.as_nanos() || cached.as_micros() < 5_000,
            "cached ACL should dwarf full reparse (cached={cached:?} parsed={parsed:?})"
        );
    }

    fn live_like_page() -> PageCatalog {
        parse_page(
            r#"
<article class="widget now" data-id="now-playing" data-hooks="media"></article>
<article class="widget" data-id="watch"></article>
<template class="spawnable" data-id="draw" data-anchor="c" data-hooks="layout,spawn"></template>
<template class="spawnable action-bar" data-id="action-bar" data-hooks="shell,hotkey,pty" data-anchor="tc"></template>
<template class="spawnable" data-id="calc" data-anchor="tr"></template>
"#,
        )
    }

    fn folder_none(_: &str) -> Option<Vec<String>> {
        None
    }

    #[test]
    fn classifies_windows_by_label() {
        let cat = live_like_page();
        assert_eq!(classify_invoker("desktop", &cat, folder_none), Invoker::Desktop);
        assert_eq!(classify_invoker("desktop-2", &cat, folder_none), Invoker::Desktop);
        assert_eq!(classify_invoker("desktop-x", &cat, folder_none), Invoker::Host);
        assert_eq!(classify_invoker("launcher-dim", &cat, folder_none), Invoker::Host);
        assert_eq!(classify_invoker("guides", &cat, folder_none), Invoker::Host);
        assert_eq!(classify_invoker("prewarm", &cat, folder_none), Invoker::Host);
        assert_eq!(classify_invoker("widget-", &cat, folder_none), Invoker::Host);
        assert_eq!(
            classify_invoker("widget-action-bar", &cat, folder_none),
            Invoker::Piece { id: "action-bar".into(), spawn: true }
        );
        assert_eq!(
            classify_invoker("widget-calculator", &cat, folder_none),
            Invoker::Piece { id: "calc".into(), spawn: true }
        );
        assert_eq!(
            classify_invoker("widget-dice", &cat, |id| (id == "dice").then(|| vec!["media".into()])),
            Invoker::Folder { id: "dice".into(), hooks: vec!["media".into()] }
        );
        assert_eq!(classify_invoker("widget-unknown", &cat, folder_none), Invoker::Host);
    }

    #[test]
    fn spawn_window_acts_as_its_piece_regardless_of_caller() {
        let cat = live_like_page();
        let bar = classify_invoker("widget-action-bar", &cat, folder_none);
        // bar.js calls pty_* / cli_* without a caller.
        for hook in ["pty", "shell"] {
            assert_eq!(authorize_hook(&cat, &bar, None, hook, HookClass::Exec).unwrap(), "action-bar");
        }
        assert!(authorize_hook(&cat, &bar, None, "shell", HookClass::Plain).is_ok());
        // Claiming another piece does not borrow its hooks.
        assert!(authorize_hook(&cat, &bar, Some("now-playing"), "media", HookClass::Plain).is_err());
        let draw = classify_invoker("widget-draw", &cat, folder_none);
        assert!(authorize_hook(&cat, &draw, Some("action-bar"), "pty", HookClass::Exec).is_err());
        assert!(authorize_hook(&cat, &draw, Some("action-bar"), "shell", HookClass::Plain).is_err());
        assert_eq!(authorize_hook(&cat, &draw, None, "spawn", HookClass::Plain).unwrap(), "draw");
    }

    #[test]
    fn desktop_needs_caller_and_never_execs() {
        let cat = live_like_page();
        let desk = Invoker::Desktop;
        // Live desktop: watch -> now-playing media, Draw button -> draw spawn.
        assert_eq!(
            authorize_hook(&cat, &desk, Some("now-playing"), "media", HookClass::Plain).unwrap(),
            "now-playing"
        );
        assert!(authorize_hook(&cat, &desk, Some("draw"), "spawn", HookClass::Plain).is_ok());
        assert!(authorize_hook(&cat, &desk, None, "spawn", HookClass::Plain).is_err());
        assert!(authorize_hook(&cat, &desk, Some(" "), "media", HookClass::Plain).is_err());
        assert!(authorize_hook(&cat, &desk, Some("ghost"), "media", HookClass::Plain).is_err());
        // shell (open URL) may be claimed; exec class never from the wallpaper.
        assert!(authorize_hook(&cat, &desk, Some("action-bar"), "shell", HookClass::Plain).is_ok());
        for hook in ["pty", "shell"] {
            assert!(authorize_hook(&cat, &desk, Some("action-bar"), hook, HookClass::Exec).is_err());
        }
    }

    #[test]
    fn host_and_folder_windows() {
        let cat = live_like_page();
        assert!(authorize_hook(&cat, &Invoker::Host, Some("now-playing"), "media", HookClass::Plain).is_err());
        let folder = Invoker::Folder { id: "dice".into(), hooks: vec!["media".into()] };
        assert!(authorize_hook(&cat, &folder, None, "media", HookClass::Plain).is_ok());
        assert!(authorize_hook(&cat, &folder, None, "mouse", HookClass::Plain).is_err());
        assert!(authorize_hook(&cat, &folder, None, "media", HookClass::Exec).is_err());
        // Floating (non-spawn) page piece window never gets exec.
        let watch = Invoker::Piece { id: "now-playing".into(), spawn: false };
        assert!(authorize_hook(&cat, &watch, None, "media", HookClass::Plain).is_ok());
        assert!(authorize_hook(&cat, &watch, None, "media", HookClass::Exec).is_err());
        assert!(authorize_hook(&cat, &Invoker::Desktop, Some("now-playing"), "bogus", HookClass::Plain).is_err());
    }

    #[test]
    fn control_class_only_from_a_spawn_window() {
        let cat = parse_page(
            r#"<template class="spawnable" data-id="qs" data-hooks="radios,display" data-anchor="top"></template>
<article class="widget" data-id="wifi-tile" data-hooks="radios"></article>"#,
        );
        let bar = classify_invoker("widget-qs", &cat, folder_none);
        assert_eq!(authorize_hook(&cat, &bar, None, "radios", HookClass::Control).unwrap(), "qs");
        assert!(authorize_hook(&cat, &bar, None, "power-mode", HookClass::Control).is_err()); // undeclared
        // The wallpaper reads but never changes a setting.
        let desk = Invoker::Desktop;
        assert!(authorize_hook(&cat, &desk, Some("wifi-tile"), "radios", HookClass::Plain).is_ok());
        let refused = authorize_hook(&cat, &desk, Some("wifi-tile"), "radios", HookClass::Control).unwrap_err();
        assert!(refused.to_string().contains("settings cannot change"));
        let tile = Invoker::Piece { id: "wifi-tile".into(), spawn: false };
        assert!(authorize_hook(&cat, &tile, None, "radios", HookClass::Control).is_err());
        let folder = Invoker::Folder { id: "dice".into(), hooks: vec!["radios".into()] };
        assert!(authorize_hook(&cat, &folder, None, "radios", HookClass::Control).is_err());
    }

    #[test]
    fn commented_out_pieces_are_not_on_the_page() {
        let cat = parse_page(
            r#"
<article class="widget" data-id="clock"></article>
<!-- Example: remove the comment markers to try it.
<template class="spawnable" data-id="topbar" data-anchor="top" data-hooks="battery"></template>
-->
<template class="spawnable" data-id="calc" data-anchor="tr"></template>
<!-- unterminated <article class="widget" data-id="late"></article>
"#,
        );
        assert!(cat.get("clock").is_some());
        assert!(cat.get("topbar").is_none());
        assert!(cat.get("calc").is_some());
        // An unterminated comment does not swallow the rest of the page.
        assert!(cat.get("late").is_some());
    }

    #[test]
    fn starter_examples_are_off_until_uncommented() {
        let starter = include_str!("../../templates/starter/desktop/index.html").replace('\r', "");
        let cat = parse_page(&starter);
        for id in ["clock", "note", "draw", "action-bar"] {
            assert!(cat.get(id).is_some(), "{id} is on the starter page");
        }
        assert!(cat.get("note").unwrap().hooks == vec!["storage".to_string()]);
        for id in ["topbar", "hello", "taskbar"] {
            assert!(cat.get(id).is_none(), "{id} is commented out");
        }
        // Deleting the "try me" marker lines turns the examples on.
        let on: String = starter
            .lines()
            .filter(|l| !l.trim_start().starts_with("<!-- try me") && !l.trim_start().starts_with("try me"))
            .map(|l| format!("{l}\n"))
            .collect();
        let cat = parse_page(&on);
        let top = cat.spawnable("topbar").expect("top bar");
        assert_eq!(crate::docks::piece_dock(top).map(|d| d.edge), Some(crate::docks::Edge::Top));
        assert!(cat.is_overlay("hello"));
        assert_eq!(piece_offset(cat.spawnable("hello").unwrap()), (24, 24));
        let taskbar = cat.spawnable("taskbar").expect("taskbar");
        assert!(crate::docks::on_every_monitor(taskbar));
        assert_eq!(taskbar.hooks, ["windows", "windows-control", "tray", "posture"].map(String::from));
        assert!(cat.get("clock").is_some() && cat.get("action-bar").is_some());
    }

    #[test]
    fn monitor_copies_act_as_their_bar() {
        let cat = parse_page(
            r#"
<template class="spawnable" data-id="taskbar" data-anchor="bottom" data-monitor="all" data-hooks="battery"></template>
<template class="spawnable" data-id="topbar" data-anchor="top" data-hooks="battery"></template>
<template class="spawnable" data-id="calc" data-anchor="tr"></template>
"#,
        );
        assert_eq!(split_monitor_copy("taskbar:2"), Some(("taskbar", 2)));
        assert_eq!(split_monitor_copy("taskbar"), None);
        assert_eq!(split_monitor_copy("taskbar:x"), None);
        assert_eq!(split_monitor_copy("taskbar:0"), None);
        assert_eq!(split_monitor_copy(":2"), None);
        assert_eq!(label_piece_id("widget-taskbar:3"), Some("taskbar"));
        assert_eq!(label_piece_id("widget-taskbar"), Some("taskbar"));
        assert_eq!(label_piece_id("desktop"), None);
        assert_eq!(
            classify_invoker("widget-taskbar:2", &cat, folder_none),
            Invoker::Piece { id: "taskbar".into(), spawn: true }
        );
        let copy = classify_invoker("widget-taskbar:2", &cat, folder_none);
        assert_eq!(authorize_hook(&cat, &copy, None, "battery", HookClass::Plain).unwrap(), "taskbar");
        assert!(authorize_hook(&cat, &copy, None, "power", HookClass::Plain).is_err());
        // Only bars on every monitor have copies.
        assert_eq!(classify_invoker("widget-topbar:2", &cat, folder_none), Invoker::Host);
        assert_eq!(classify_invoker("widget-calc:2", &cat, folder_none), Invoker::Host);
    }

    #[test]
    fn same_piece_handles_alias_and_case() {
        let cat = live_like_page();
        assert!(same_piece(&cat, "calc", "Calculator"));
        assert!(same_piece(&cat, "draw", "DRAW"));
        assert!(!same_piece(&cat, "draw", "action-bar"));
    }
}
