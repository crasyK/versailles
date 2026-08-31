//! Boot clock, counters, and bench-mode flags.
//!
//! `VERSAILLES_BENCH=1` (or `--bench-boot`) isolates the process for
//! `npm run bench:boot`: no single-instance bounce, no autostart mutation,
//! no global hotkeys. Marks are exposed at `GET /debug/boot`.

use crate::registry::widgets_root;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static BENCH: OnceLock<bool> = OnceLock::new();

static HTML_READS: AtomicU64 = AtomicU64::new(0);
static PARSE_PAGE: AtomicU64 = AtomicU64::new(0);
static IFRAME_NAVS: AtomicU64 = AtomicU64::new(0);
static MEDIA_NOW: AtomicU64 = AtomicU64::new(0);

static API_BOUND_MS: AtomicU64 = AtomicU64::new(0);
static DESKTOP_WINDOW_MS: AtomicU64 = AtomicU64::new(0);
static LAYOUT_EMITTED_MS: AtomicU64 = AtomicU64::new(0);
static IFRAME_NAVIGATED_MS: AtomicU64 = AtomicU64::new(0);
static WALLPAPER_LOADED_MS: AtomicU64 = AtomicU64::new(0);
static READY: AtomicBool = AtomicBool::new(false);

pub fn note_start() {
    let _ = START.set(Instant::now());
    let _ = BENCH.set(
        std::env::var("VERSAILLES_BENCH")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            || std::env::args().any(|a| a == "--bench-boot"),
    );
}

pub fn is_bench() -> bool {
    *BENCH.get().unwrap_or(&false)
        || std::env::var("VERSAILLES_BENCH")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
}

pub fn elapsed_ms() -> u64 {
    START
        .get()
        .map(|t| t.elapsed().as_millis() as u64)
        .unwrap_or(0)
}

pub fn count_html_read() {
    HTML_READS.fetch_add(1, Ordering::Relaxed);
}

pub fn count_parse_page() {
    PARSE_PAGE.fetch_add(1, Ordering::Relaxed);
}

pub fn count_iframe_nav() {
    let n = IFRAME_NAVS.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 {
        IFRAME_NAVIGATED_MS.store(elapsed_ms(), Ordering::Relaxed);
    }
    persist();
}

pub fn count_media_now() {
    MEDIA_NOW.fetch_add(1, Ordering::Relaxed);
}

pub fn mark_api_bound() {
    API_BOUND_MS.store(elapsed_ms(), Ordering::Relaxed);
    persist();
}

pub fn mark_desktop_window() {
    DESKTOP_WINDOW_MS.store(elapsed_ms(), Ordering::Relaxed);
    persist();
}

pub fn mark_layout_emitted() {
    LAYOUT_EMITTED_MS.store(elapsed_ms(), Ordering::Relaxed);
    persist();
}

pub fn mark_wallpaper_loaded() {
    if READY.swap(true, Ordering::SeqCst) {
        return;
    }
    WALLPAPER_LOADED_MS.store(elapsed_ms(), Ordering::Relaxed);
    persist();
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootCounters {
    pub html_reads: u64,
    pub parse_page_calls: u64,
    pub iframe_navigations: u64,
    pub media_now_calls: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootMarks {
    pub api_bound: u64,
    pub desktop_window: u64,
    pub layout_emitted: u64,
    pub iframe_navigated: u64,
    pub wallpaper_loaded: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootSnapshot {
    pub ready: bool,
    pub ready_ms: u64,
    pub marks: BootMarks,
    pub counters: BootCounters,
}

pub fn snapshot() -> BootSnapshot {
    let ready = READY.load(Ordering::Relaxed);
    let wallpaper = WALLPAPER_LOADED_MS.load(Ordering::Relaxed);
    BootSnapshot {
        ready,
        ready_ms: if ready { wallpaper } else { elapsed_ms() },
        marks: BootMarks {
            api_bound: API_BOUND_MS.load(Ordering::Relaxed),
            desktop_window: DESKTOP_WINDOW_MS.load(Ordering::Relaxed),
            layout_emitted: LAYOUT_EMITTED_MS.load(Ordering::Relaxed),
            iframe_navigated: IFRAME_NAVIGATED_MS.load(Ordering::Relaxed),
            wallpaper_loaded: wallpaper,
        },
        counters: BootCounters {
            html_reads: HTML_READS.load(Ordering::Relaxed),
            parse_page_calls: PARSE_PAGE.load(Ordering::Relaxed),
            iframe_navigations: IFRAME_NAVS.load(Ordering::Relaxed),
            media_now_calls: MEDIA_NOW.load(Ordering::Relaxed),
        },
    }
}

fn persist() {
    if !is_bench() {
        return;
    }
    let Ok(root) = widgets_root() else {
        return;
    };
    let dir = root.join(".versailles");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_vec_pretty(&snapshot()) {
        let _ = std::fs::write(dir.join("boot.json"), json);
    }
}
