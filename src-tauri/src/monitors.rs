//! Monitors: the list pages see (`monitor_info`), which monitors get a
//! wallpaper window, `data-monitor` selections, and the watcher that reacts
//! when monitors are added, removed, resized or rescaled.

use crate::layout::Rect;
use crate::page::{PageCatalog, PageKind};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// One monitor. Rects are physical pixels; `scale` converts to logical.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    /// Windows display number (Settings → System → Display), from `\\.\DISPLAYn`.
    pub number: u32,
    pub primary: bool,
    pub name: String,
    pub scale: f64,
    pub rect: Rect,
    pub work: Rect,
}

/// `data-monitor` on a piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorSel {
    Primary,
    All,
    Cursor,
    Number(u32),
}

pub fn parse_sel(raw: Option<&str>) -> Option<MonitorSel> {
    let raw = raw?.trim().to_ascii_lowercase();
    Some(match raw.as_str() {
        "primary" | "main" => MonitorSel::Primary,
        "all" => MonitorSel::All,
        "cursor" => MonitorSel::Cursor,
        n => MonitorSel::Number(n.parse().ok().filter(|n: &u32| *n > 0)?),
    })
}

/// `\\.\DISPLAY2` → 2. Falls back to `fallback` for unexpected names.
pub fn display_number(name: &str, fallback: u32) -> u32 {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok().filter(|n| *n > 0).unwrap_or(fallback)
}

/// Window label of the wallpaper on `m`: the primary keeps `desktop`.
pub fn wallpaper_label(m: &MonitorInfo) -> String {
    if m.primary {
        crate::page::DESKTOP_LABEL.to_string()
    } else {
        format!("{}-{}", crate::page::DESKTOP_LABEL, m.number)
    }
}

/// Monitor numbers that get a wallpaper: always the primary, plus every
/// monitor a wallpaper widget selects with `data-monitor`. Without any
/// `data-monitor`, that is the primary alone (the single-wallpaper behaviour).
pub fn wallpaper_numbers(monitors: &[MonitorInfo], selections: &[MonitorSel]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut push = |n: u32| {
        if !out.contains(&n) {
            out.push(n);
        }
    };
    if let Some(primary) = monitors.iter().find(|m| m.primary).or(monitors.first()) {
        push(primary.number);
    }
    for sel in selections {
        match sel {
            MonitorSel::All => monitors.iter().for_each(|m| push(m.number)),
            MonitorSel::Number(n) if monitors.iter().any(|m| m.number == *n) => push(*n),
            _ => {}
        }
    }
    out
}

/// `data-monitor` selections of wallpaper widgets on the page.
pub fn widget_selections(cat: &PageCatalog) -> Vec<MonitorSel> {
    cat.pieces
        .iter()
        .filter(|p| p.kind == PageKind::Widget)
        .filter_map(|p| parse_sel(p.monitor.as_deref()))
        .collect()
}

/// The monitor a selection names, if any (`Cursor` / `All` name none).
pub fn select<'a>(monitors: &'a [MonitorInfo], sel: MonitorSel) -> Option<&'a MonitorInfo> {
    match sel {
        MonitorSel::Primary => monitors.iter().find(|m| m.primary).or(monitors.first()),
        MonitorSel::Number(n) => monitors.iter().find(|m| m.number == n),
        MonitorSel::All | MonitorSel::Cursor => None,
    }
}

/// Monitor holding the point (physical).
pub fn at_point(monitors: &[MonitorInfo], x: i32, y: i32) -> Option<&MonitorInfo> {
    monitors.iter().find(|m| {
        x >= m.rect.x && x < m.rect.x + m.rect.width && y >= m.rect.y && y < m.rect.y + m.rect.height
    })
}

/// Page form: logical sizes (CSS px) plus the physical origin.
pub fn page_json(m: &MonitorInfo) -> Value {
    let s = if m.scale > 0.0 { m.scale } else { 1.0 };
    let logical = |r: &Rect| {
        json!({
            "x": r.x, "y": r.y,
            "width": (r.width as f64 / s).round(),
            "height": (r.height as f64 / s).round(),
        })
    };
    json!({
        "number": m.number,
        "primary": m.primary,
        "name": m.name,
        "scale": m.scale,
        "bounds": logical(&m.rect),
        "work": logical(&m.work),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorReport {
    pub monitors: Vec<Value>,
    /// Number of the monitor the calling window is on.
    pub current: Option<u32>,
}

/// Every monitor, with work area and primary flag from Win32.
pub fn list(app: &AppHandle) -> Vec<MonitorInfo> {
    let Some(window) = app.webview_windows().into_values().next() else {
        return Vec::new();
    };
    let Ok(monitors) = window.available_monitors() else {
        return Vec::new();
    };
    monitors
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let rect = Rect {
                x: m.position().x,
                y: m.position().y,
                width: m.size().width as i32,
                height: m.size().height as i32,
            };
            let name = m.name().cloned().unwrap_or_default();
            let (work, primary) = work_area(rect).unwrap_or((rect, i == 0));
            MonitorInfo {
                number: display_number(&name, i as u32 + 1),
                primary,
                name,
                scale: m.scale_factor(),
                rect,
                work,
            }
        })
        .collect()
}

#[cfg(windows)]
fn work_area(rect: Rect) -> Option<(Rect, bool)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    const MONITORINFOF_PRIMARY: u32 = 1;
    unsafe {
        let center = POINT {
            x: rect.x + rect.width / 2,
            y: rect.y + rect.height / 2,
        };
        let hmon = MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut info).as_bool() {
            return None;
        }
        let w = info.rcWork;
        Some((
            Rect {
                x: w.left,
                y: w.top,
                width: (w.right - w.left).max(1),
                height: (w.bottom - w.top).max(1),
            },
            info.dwFlags & MONITORINFOF_PRIMARY != 0,
        ))
    }
}

#[cfg(not(windows))]
fn work_area(_rect: Rect) -> Option<(Rect, bool)> {
    None
}

/// Watch for monitors being added, removed, resized or rescaled. On a
/// change: re-place the wallpapers, bring stranded windows back on screen,
/// tell the shells to reload their layout, and push `monitor-changed`.
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = list(&app);
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = list(&app);
            if now.is_empty() || now == last {
                continue;
            }
            last = now.clone();
            tracing::info!("monitors changed: {} connected", now.len());
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                crate::window_manager::on_monitors_changed(&handle);
                let monitors: Vec<Value> = now.iter().map(page_json).collect();
                crate::push::to_all(&handle, "monitor-changed", &json!({ "monitors": monitors }));
            });
        }
    });
}

/// Monitor number the window `label` is on (by its centre).
pub fn of_window(app: &AppHandle, label: &str) -> Option<u32> {
    let window = app.get_webview_window(label)?;
    let pos = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let monitors = list(app);
    at_point(
        &monitors,
        pos.x + size.width as i32 / 2,
        pos.y + size.height as i32 / 2,
    )
    .map(|m| m.number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(number: u32, primary: bool, x: i32) -> MonitorInfo {
        let rect = Rect { x, y: 0, width: 1920, height: 1080 };
        MonitorInfo {
            number,
            primary,
            name: format!(r"\\.\DISPLAY{number}"),
            scale: 1.5,
            rect,
            work: Rect { y: 48, height: 1032, ..rect },
        }
    }

    #[test]
    fn parses_selections() {
        assert_eq!(parse_sel(Some("primary")), Some(MonitorSel::Primary));
        assert_eq!(parse_sel(Some(" ALL ")), Some(MonitorSel::All));
        assert_eq!(parse_sel(Some("2")), Some(MonitorSel::Number(2)));
        assert_eq!(parse_sel(Some("cursor")), Some(MonitorSel::Cursor));
        assert_eq!(parse_sel(Some("0")), None);
        assert_eq!(parse_sel(Some("left")), None);
        assert_eq!(parse_sel(None), None);
    }

    #[test]
    fn reads_display_numbers() {
        assert_eq!(display_number(r"\\.\DISPLAY2", 9), 2);
        assert_eq!(display_number(r"\\.\DISPLAY12", 9), 12);
        assert_eq!(display_number("Generic PnP", 3), 3);
    }

    #[test]
    fn wallpaper_on_primary_only_unless_selected() {
        let monitors = [mon(1, true, 0), mon(2, false, 1920), mon(3, false, 3840)];
        assert_eq!(wallpaper_numbers(&monitors, &[]), vec![1]);
        assert_eq!(wallpaper_numbers(&monitors, &[MonitorSel::Number(3)]), vec![1, 3]);
        assert_eq!(wallpaper_numbers(&monitors, &[MonitorSel::All]), vec![1, 2, 3]);
        // A monitor that is not connected gets nothing.
        assert_eq!(wallpaper_numbers(&monitors, &[MonitorSel::Number(7)]), vec![1]);
        // Primary that is not display 1.
        let swapped = [mon(1, false, 0), mon(2, true, 1920)];
        assert_eq!(wallpaper_numbers(&swapped, &[MonitorSel::Primary]), vec![2]);
    }

    #[test]
    fn labels_keep_desktop_for_the_primary() {
        assert_eq!(wallpaper_label(&mon(2, true, 0)), "desktop");
        assert_eq!(wallpaper_label(&mon(2, false, 0)), "desktop-2");
    }

    #[test]
    fn selects_and_locates_monitors() {
        let monitors = [mon(1, true, 0), mon(2, false, 1920)];
        assert_eq!(select(&monitors, MonitorSel::Number(2)).unwrap().number, 2);
        assert_eq!(select(&monitors, MonitorSel::Primary).unwrap().number, 1);
        assert!(select(&monitors, MonitorSel::Cursor).is_none());
        assert_eq!(at_point(&monitors, 2000, 10).unwrap().number, 2);
        assert!(at_point(&monitors, -5, 10).is_none());
    }

    #[test]
    fn page_json_is_logical() {
        let v = page_json(&mon(2, false, 1920));
        assert_eq!(v["bounds"]["width"], 1280.0);
        assert_eq!(v["work"]["height"], 688.0);
        assert_eq!(v["number"], 2);
    }
}
