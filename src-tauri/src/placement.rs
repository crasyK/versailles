//! Where overlay windows go: the `data-anchor` grid, `data-offset`, and
//! keeping a page's own layout across reveals. Pure rect math (physical px)
//! so the rules are testable without windows.

use crate::layout::Rect;

/// Position on one axis of the anchor grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// `(horizontal, vertical)` point of the 3×3 grid.
    Grid(Align, Align),
    /// Next to the cursor.
    Cursor,
}

/// Overlay anchor for a `data-anchor` value. `None` means a docked slide-out
/// (`tr`, no anchor, or anything unknown).
pub fn parse_anchor(raw: Option<&str>) -> Option<Anchor> {
    use Align::*;
    let raw = raw?.trim().to_ascii_lowercase();
    Some(match raw.as_str() {
        "c" | "center" | "overlay" => Anchor::Grid(Center, Center),
        "tl" => Anchor::Grid(Start, Start),
        "tc" => Anchor::Grid(Center, Start),
        "ml" => Anchor::Grid(Start, Center),
        "mr" => Anchor::Grid(End, Center),
        "bl" => Anchor::Grid(Start, End),
        "bc" => Anchor::Grid(Center, End),
        "br" => Anchor::Grid(End, End),
        "cursor" => Anchor::Cursor,
        _ => return None,
    })
}

/// `c` / `center` / `overlay`: the host re-applies size and centers on every
/// reveal. Every other overlay anchor hands its layout to the page after the
/// first open.
pub fn is_centered(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("c") | Some("center") | Some("overlay")
    )
}

/// `data-offset="x,y"` in logical px. Missing or malformed parts are 0.
pub fn parse_offset(raw: Option<&str>) -> Option<(i32, i32)> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut parts = raw.split([',', ' ']).filter(|s| !s.is_empty());
    let x = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let y = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    Some((x, y))
}

/// Logical offset when the piece has no `data-offset`: `tc` rests 120 px down.
pub fn default_offset(raw_anchor: Option<&str>) -> (i32, i32) {
    match raw_anchor.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("tc") => (0, 120),
        _ => (0, 0),
    }
}

fn axis(align: Align, lo: i32, span: i32, len: i32, inset: i32) -> i32 {
    match align {
        Align::Start => lo + inset,
        Align::Center => lo + (span - len) / 2 + inset,
        Align::End => lo + span - len - inset,
    }
}

fn clamp_axis(v: i32, lo: i32, len: i32, span: i32) -> i32 {
    if len >= span {
        lo
    } else {
        v.clamp(lo, lo + span - len)
    }
}

/// Clamp `r` so it lies on `mon` (pinned to the origin when larger).
pub fn clamp_onto(r: Rect, mon: Rect) -> Rect {
    Rect {
        x: clamp_axis(r.x, mon.x, r.width, mon.width),
        y: clamp_axis(r.y, mon.y, r.height, mon.height),
        ..r
    }
}

/// Rect of a `width` × `height` window at `anchor` on `mon`. `offset` is an
/// inset toward the monitor's middle: from the left/top edge for start
/// anchors, from the right/bottom edge for end anchors, and a plain
/// right/down shift on a centre axis. For `Cursor` it is added to the cursor.
pub fn anchor_rect(
    anchor: Anchor,
    mon: Rect,
    width: i32,
    height: i32,
    offset: (i32, i32),
    cursor: Option<(i32, i32)>,
) -> Rect {
    let (x, y) = match anchor {
        Anchor::Grid(h, v) => (
            axis(h, mon.x, mon.width, width, offset.0),
            axis(v, mon.y, mon.height, height, offset.1),
        ),
        Anchor::Cursor => match cursor {
            Some((cx, cy)) => (cx + offset.0, cy + offset.1),
            None => (
                axis(Align::Center, mon.x, mon.width, width, 0),
                axis(Align::Center, mon.y, mon.height, height, 0),
            ),
        },
    };
    clamp_onto(
        Rect {
            x,
            y,
            width,
            height,
        },
        mon,
    )
}

fn near(a: i32, b: i32) -> bool {
    (a - b).abs() <= 2
}

/// Whether `win` covers `mon` (within rounding).
pub fn covers(win: Rect, mon: Rect) -> bool {
    near(win.width, mon.width) && near(win.height, mon.height)
}

/// Where an existing overlay lands on reveal when the page owns its layout:
/// same size, same offset from the anchor point (`h`, `v`) of the monitor it
/// sat on, scaled to the target DPI and clamped onto `dst`. A window covering
/// its monitor covers `dst`.
pub fn keep_layout_rect(
    win: Rect,
    src: Rect,
    src_scale: f64,
    dst: Rect,
    dst_scale: f64,
    h: Align,
    v: Align,
) -> Rect {
    if covers(win, src) {
        return dst;
    }
    let ratio = if src_scale > 0.0 && dst_scale > 0.0 {
        dst_scale / src_scale
    } else {
        1.0
    };
    let scale = |n: i32| ((n as f64) * ratio).round() as i32;
    let width = scale(win.width);
    let height = scale(win.height);
    // Anchor point on the window and on the monitor, per axis.
    let point = |align: Align, lo: i32, len: i32| match align {
        Align::Start => lo,
        Align::Center => lo + len / 2,
        Align::End => lo + len,
    };
    let place = |align: Align, p: i32, len: i32| match align {
        Align::Start => p,
        Align::Center => p - len / 2,
        Align::End => p - len,
    };
    let dx = point(h, win.x, win.width) - point(h, src.x, src.width);
    let dy = point(v, win.y, win.height) - point(v, src.y, src.height);
    let x = place(h, point(h, dst.x, dst.width) + scale(dx), width);
    let y = place(v, point(v, dst.y, dst.height) + scale(dy), height);
    clamp_onto(
        Rect {
            x,
            y,
            width,
            height,
        },
        dst,
    )
}

/// How an overlay's backdrop is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DimMode {
    /// The host's `launcher-dim` window (default).
    #[default]
    Host,
    /// The overlay window covers the monitor; the page draws its own backdrop.
    Page,
    /// No backdrop.
    None,
}

pub fn parse_dim(raw: &str) -> Option<DimMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "host" | "true" | "1" | "yes" => Some(DimMode::Host),
        "page" => Some(DimMode::Page),
        "none" | "false" | "0" | "no" | "off" => Some(DimMode::None),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Align::*;

    fn r(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    const MON: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn parses_the_anchor_grid() {
        assert_eq!(parse_anchor(Some("TC")), Some(Anchor::Grid(Center, Start)));
        assert_eq!(parse_anchor(Some("center")), Some(Anchor::Grid(Center, Center)));
        assert_eq!(parse_anchor(Some("br")), Some(Anchor::Grid(End, End)));
        assert_eq!(parse_anchor(Some("cursor")), Some(Anchor::Cursor));
        // `tr` stays the docked slide-out.
        assert_eq!(parse_anchor(Some("tr")), None);
        assert_eq!(parse_anchor(None), None);
        assert_eq!(parse_anchor(Some("sideways")), None);
    }

    #[test]
    fn parses_offsets() {
        assert_eq!(parse_offset(Some("12,-8")), Some((12, -8)));
        assert_eq!(parse_offset(Some(" 24 ")), Some((24, 0)));
        assert_eq!(parse_offset(Some("x,5")), Some((0, 5)));
        assert_eq!(parse_offset(Some("")), None);
        assert_eq!(default_offset(Some("tc")), (0, 120));
        assert_eq!(default_offset(Some("br")), (0, 0));
    }

    #[test]
    fn anchor_rect_matches_the_documented_tc_and_c_places() {
        let tc = anchor_rect(Anchor::Grid(Center, Start), MON, 800, 440, (0, 120), None);
        assert_eq!(tc, r(560, 120, 800, 440));
        let c = anchor_rect(Anchor::Grid(Center, Center), MON, 800, 440, (0, 0), None);
        assert_eq!(c, r(560, 320, 800, 440));
    }

    #[test]
    fn anchor_rect_insets_toward_the_middle() {
        let br = anchor_rect(Anchor::Grid(End, End), MON, 300, 200, (16, 24), None);
        assert_eq!(br, r(1920 - 300 - 16, 1080 - 200 - 24, 300, 200));
        let tl = anchor_rect(Anchor::Grid(Start, Start), MON, 300, 200, (16, 24), None);
        assert_eq!(tl, r(16, 24, 300, 200));
        let ml = anchor_rect(Anchor::Grid(Start, Center), MON, 300, 200, (0, -40), None);
        assert_eq!(ml, r(0, 440 - 40, 300, 200));
    }

    #[test]
    fn anchor_rect_follows_the_cursor_and_stays_on_screen() {
        let at = anchor_rect(Anchor::Cursor, MON, 300, 200, (8, 8), Some((100, 100)));
        assert_eq!(at, r(108, 108, 300, 200));
        let edge = anchor_rect(Anchor::Cursor, MON, 300, 200, (8, 8), Some((1900, 1070)));
        assert_eq!(edge, r(1620, 880, 300, 200));
    }

    #[test]
    fn keep_layout_holds_the_page_layout_on_the_same_monitor() {
        let bar = r(560, 120, 800, 440);
        assert_eq!(keep_layout_rect(bar, MON, 1.0, MON, 1.0, Center, Start), bar);
        let moved = r(100, 600, 800, 300);
        assert_eq!(keep_layout_rect(moved, MON, 1.0, MON, 1.0, Center, Start), moved);
    }

    #[test]
    fn keep_layout_covering_window_covers_the_target() {
        assert_eq!(keep_layout_rect(MON, MON, 1.0, MON, 1.0, Center, Start), MON);
        assert_eq!(
            keep_layout_rect(r(0, 0, 1919, 1081), MON, 1.0, MON, 1.0, Center, Start),
            MON
        );
        let right = r(1920, 0, 2560, 1440);
        assert_eq!(keep_layout_rect(MON, MON, 1.0, right, 1.0, End, End), right);
    }

    #[test]
    fn keep_layout_keeps_the_anchor_on_a_bigger_monitor() {
        let right = r(1920, 0, 2560, 1440);
        let bar = r(560, 120, 800, 440);
        let tc = keep_layout_rect(bar, MON, 1.0, right, 1.0, Center, Start);
        assert_eq!(tc, r(1920 + (2560 - 800) / 2, 120, 800, 440));
        // Bottom-right piece stays 16 px in from the bottom-right corner.
        let br = r(1920 - 316, 1080 - 216, 300, 200);
        let next = keep_layout_rect(br, MON, 1.0, right, 1.0, End, End);
        assert_eq!(next, r(1920 + 2560 - 316, 1440 - 216, 300, 200));
    }

    #[test]
    fn keep_layout_scales_with_dpi() {
        let bar = r(560, 120, 800, 440);
        let hi = r(-3840, 0, 3840, 2160);
        let next = keep_layout_rect(bar, MON, 1.0, hi, 2.0, Center, Start);
        assert_eq!(next, r(-3840 + (3840 - 1600) / 2, 240, 1600, 880));
    }

    #[test]
    fn keep_layout_clamps_onto_the_target() {
        let small = r(0, 0, 1280, 720);
        let next = keep_layout_rect(r(1500, 900, 800, 440), MON, 1.0, small, 1.0, Start, Start);
        assert_eq!(next, r(1280 - 800, 720 - 440, 800, 440));
        let next = keep_layout_rect(r(0, 0, 1600, 900), MON, 1.0, small, 1.0, Start, Start);
        assert_eq!((next.x, next.y), (0, 0));
    }

    #[test]
    fn parses_dim_modes() {
        assert_eq!(parse_dim("page"), Some(DimMode::Page));
        assert_eq!(parse_dim("NONE"), Some(DimMode::None));
        assert_eq!(parse_dim("host"), Some(DimMode::Host));
        assert_eq!(parse_dim("false"), Some(DimMode::None));
        assert_eq!(parse_dim("blur"), None);
    }
}
