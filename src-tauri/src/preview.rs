//! Hover previews (#59): `windows.preview(id, rect)` shows a live thumbnail
//! of an app window in a small card next to a taskbar button;
//! `windows.preview(null)` hides it.
//!
//! The card is a plain Win32 popup of our own (not a webview: it opens at
//! once and costs no browser process), drawn in GDI with the Paper look
//! (paper fill, 2 px ink stroke, 12 px corners, the title in Segoe UI). DWM
//! draws the thumbnail into it (`DwmRegisterThumbnail`), which needs a
//! top-level window of the calling process. It never takes focus or input:
//! tool window, no-activate, topmost, and click-through (layered with
//! `WS_EX_TRANSPARENT`).
//!
//! The page sends the hovered element's rect in its CSS pixels; the host
//! converts it with the calling window's position and scale, then sizes the
//! card for the button's monitor DPI and places it above the button (below
//! for a top bar, beside for a side bar), kept on that monitor's work area.
//! Windows that give no thumbnail (an empty source, some minimized apps)
//! show their icon and title instead.
//!
//! The card hides on `preview(null)` (after a short grace, so moving between
//! buttons does not blink), when the pointer has left both the calling bar
//! and the card, when the window closes, and a minute after the last call.
//! Only bars and overlays may show it, never the wallpaper: it is a topmost
//! window placed from the caller's own position.

use crate::error::{AppError, AppResult};
use crate::layout::Rect;
use serde::Deserialize;
use std::time::{Duration, Instant};

/// A rect in the calling page's CSS pixels (`getBoundingClientRect()`).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Largest thumbnail, logical px.
pub const THUMB_MAX: (f64, f64) = (240.0, 160.0);
const PAD: f64 = 8.0;
const TITLE_H: f64 = 22.0;
const TITLE_GAP: f64 = 4.0;
/// Narrowest card content, so a short title still fits next to a tall thumbnail.
const MIN_INNER: f64 = 160.0;
const ICON: f64 = 32.0;
/// Content width of the icon-and-title card.
const ICON_CARD_INNER: f64 = 240.0;
const ICON_ROW: f64 = 40.0;
/// Between the button and the card.
const GAP: f64 = 8.0;
/// Between the card and the monitor's edge.
const MARGIN: f64 = 4.0;
pub const RADIUS: f64 = 12.0;
pub const STROKE: f64 = 2.0;

/// `preview(null)` hides after this, unless another button asks first.
pub const HIDE_GRACE: Duration = Duration::from_millis(120);
/// The pointer outside the bar and the card this long hides the card.
pub const LEFT_FOR: Duration = Duration::from_millis(300);
/// No call for this long hides the card.
pub const MAX_IDLE: Duration = Duration::from_secs(60);
/// Calls that show a card, per second.
pub const MAX_PER_SECOND: u32 = 30;

/// CSS px in the calling page → physical screen px. `origin` is the page's
/// top-left on screen (the window's client area), `scale` its device pixel ratio.
pub fn to_screen(css: CssRect, origin: (i32, i32), scale: f64) -> AppResult<Rect> {
    let finite = [css.x, css.y, css.width, css.height, scale].iter().all(|v| v.is_finite());
    if !finite || scale <= 0.0 || css.width < 0.0 || css.height < 0.0 {
        return Err(AppError::msg("windows.preview: rect needs finite x, y, width and height"));
    }
    let limit = 100_000.0;
    if [css.x, css.y, css.width, css.height].iter().any(|v| v.abs() > limit) {
        return Err(AppError::msg("windows.preview: rect is out of range"));
    }
    let x = origin.0 + (css.x * scale).round() as i32;
    let y = origin.1 + (css.y * scale).round() as i32;
    Ok(Rect {
        x,
        y,
        width: ((css.width * scale).round() as i32).max(1),
        height: ((css.height * scale).round() as i32).max(1),
    })
}

/// Fit `src` into `max`, keeping its aspect; never enlarged. `None` for an
/// empty source.
pub fn fit(src: (i32, i32), max: (i32, i32)) -> Option<(i32, i32)> {
    if src.0 <= 0 || src.1 <= 0 || max.0 <= 0 || max.1 <= 0 {
        return None;
    }
    let k = (max.0 as f64 / src.0 as f64).min(max.1 as f64 / src.1 as f64).min(1.0);
    Some((((src.0 as f64 * k).round() as i32).max(1), ((src.1 as f64 * k).round() as i32).max(1)))
}

/// The card's size and where its parts go, physical px in the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card {
    pub width: i32,
    pub height: i32,
    pub title: Rect,
    /// Where DWM draws the thumbnail; `None` shows the icon instead.
    pub thumb: Option<Rect>,
    pub icon: Option<Rect>,
}

/// Lay out the card at `scale` (monitor DPI / 96) for a source of `source`
/// physical px (`None`: no thumbnail).
pub fn card(scale: f64, source: Option<(i32, i32)>) -> Card {
    let px = |v: f64| (v * scale).round() as i32;
    let pad = px(PAD);
    let thumb = source.and_then(|s| fit(s, (px(THUMB_MAX.0), px(THUMB_MAX.1))));
    match thumb {
        Some((tw, th)) => {
            let inner = tw.max(px(MIN_INNER));
            let title = Rect { x: pad, y: pad, width: inner, height: px(TITLE_H) };
            let top = pad + title.height + px(TITLE_GAP);
            Card {
                width: inner + 2 * pad,
                height: top + th + pad,
                title,
                thumb: Some(Rect { x: pad + (inner - tw) / 2, y: top, width: tw, height: th }),
                icon: None,
            }
        }
        None => {
            let inner = px(ICON_CARD_INNER);
            let row = px(ICON_ROW);
            let icon = px(ICON);
            let gap = px(PAD);
            Card {
                width: inner + 2 * pad,
                height: row + 2 * pad,
                title: Rect { x: pad + icon + gap, y: pad, width: inner - icon - gap, height: row },
                thumb: None,
                icon: Some(Rect { x: pad, y: pad + (row - icon) / 2, width: icon, height: icon }),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Above,
    Below,
    Left,
    Right,
}

/// Which side of the button the card goes: away from the edge the calling
/// bar sits on. A wide caller (a bottom or top bar, a wide overlay) puts it
/// above a button in the monitor's lower half and below one in its upper
/// half; a tall caller (a side bar) beside the button, toward the middle.
pub fn side(caller: Rect, anchor: Rect, monitor: Rect) -> Side {
    if caller.width >= caller.height {
        if anchor.center_y() >= monitor.center_y() {
            Side::Above
        } else {
            Side::Below
        }
    } else if anchor.center_x() >= monitor.center_x() {
        Side::Left
    } else {
        Side::Right
    }
}

fn opposite(s: Side) -> Side {
    match s {
        Side::Above => Side::Below,
        Side::Below => Side::Above,
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

fn clamp_axis(v: i32, size: i32, lo: i32, hi: i32) -> i32 {
    if size >= hi - lo {
        lo
    } else {
        v.clamp(lo, hi - size)
    }
}

/// Top-left of a `size` card next to `anchor` on `side`, inside `area` (the
/// monitor's work area, physical px). A card that does not fit on that side
/// goes on the other; it is centred on the button and slid along to stay
/// inside.
pub fn place(anchor: Rect, size: (i32, i32), side: Side, area: Rect, scale: f64) -> (i32, i32) {
    let gap = (GAP * scale).round() as i32;
    let margin = (MARGIN * scale).round() as i32;
    let (w, h) = size;
    let (lo_x, hi_x) = (area.x + margin, area.right() - margin);
    let (lo_y, hi_y) = (area.y + margin, area.bottom() - margin);
    let at = |s: Side| -> (i32, i32) {
        match s {
            Side::Above => (anchor.center_x() - w / 2, anchor.y - gap - h),
            Side::Below => (anchor.center_x() - w / 2, anchor.bottom() + gap),
            Side::Left => (anchor.x - gap - w, anchor.center_y() - h / 2),
            Side::Right => (anchor.right() + gap, anchor.center_y() - h / 2),
        }
    };
    let fits = |s: Side, (x, y): (i32, i32)| match s {
        Side::Above | Side::Below => y >= lo_y && y + h <= hi_y,
        Side::Left | Side::Right => x >= lo_x && x + w <= hi_x,
    };
    let mut pos = at(side);
    if !fits(side, pos) {
        let other = at(opposite(side));
        if fits(opposite(side), other) {
            pos = other;
        }
    }
    (clamp_axis(pos.0, w, lo_x, hi_x), clamp_axis(pos.1, h, lo_y, hi_y))
}

/// At most `max` calls per second.
#[derive(Debug)]
pub struct RateLimit {
    max: u32,
    start: Option<Instant>,
    count: u32,
}

impl RateLimit {
    pub const fn new(max: u32) -> Self {
        RateLimit { max, start: None, count: 0 }
    }

    pub fn allow(&mut self, now: Instant) -> bool {
        match self.start {
            Some(t) if now.duration_since(t) < Duration::from_secs(1) => {
                if self.count >= self.max {
                    return false;
                }
                self.count += 1;
            }
            _ => {
                self.start = Some(now);
                self.count = 1;
            }
        }
        true
    }
}

/// What the page asked for: window `id` next to `anchor`, from a window at
/// `caller` (both physical screen px).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub id: i64,
    pub anchor: Rect,
    pub caller: Rect,
}

/// Show (or move) the card for listed window `id`.
pub fn show(req: Request) -> AppResult<()> {
    static LIMIT: std::sync::Mutex<RateLimit> = std::sync::Mutex::new(RateLimit::new(MAX_PER_SECOND));
    if !LIMIT.lock().unwrap().allow(Instant::now()) {
        return Err(AppError::msg(format!(
            "windows.preview: too many calls (at most {MAX_PER_SECOND} a second)"
        )));
    }
    crate::app_windows::check_listed(req.id)?;
    imp::show(req)
}

/// Hide the card (after `HIDE_GRACE`).
pub fn hide() {
    imp::hide()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::{Mutex, OnceLock};
    use windows::core::{w, BOOL};
    use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmSetWindowAttribute,
        DwmUnregisterThumbnail, DwmUpdateThumbnailProperties, DWMWA_EXTENDED_FRAME_BOUNDS,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY,
        DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
    };
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen, CreateRoundRectRgn,
        CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetMonitorInfoW, InvalidateRect,
        MonitorFromRect, RoundRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY,
        CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
        FW_NORMAL, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_INSIDEFRAME,
        SRCCOPY, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::{
        GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, DrawIconEx, GetCursorPos, GetWindowRect,
        GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindow, MsgWaitForMultipleObjects, PeekMessageW,
        PostMessageW, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, TranslateMessage, DI_NORMAL,
        HICON, HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA, MA_NOACTIVATE, MSG, PM_REMOVE, QS_ALLINPUT, SWP_NOACTIVATE,
        SWP_SHOWWINDOW, SW_HIDE, WM_APP, WM_DPICHANGED, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT,
        WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };

    /// Paper tokens as GDI colours (0x00BBGGRR).
    const PAPER: COLORREF = COLORREF(0x00FB_FAF9); // #f9fafb
    const INK: COLORREF = COLORREF(0x0014_1414); // #141414
    const WAKE: u32 = WM_APP + 1;

    struct Shared {
        want: Option<Request>,
        hide_at: Option<Instant>,
        updated: Instant,
    }

    fn shared() -> &'static Mutex<Shared> {
        static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
        SHARED.get_or_init(|| Mutex::new(Shared { want: None, hide_at: None, updated: Instant::now() }))
    }

    /// The card window, once its thread made it.
    static CARD: AtomicIsize = AtomicIsize::new(0);

    /// What WM_PAINT draws (the card's own thread).
    #[derive(Default)]
    struct Paint {
        title: String,
        card: Option<Card>,
        scale: f64,
        icon: Option<isize>,
    }

    thread_local! {
        static PAINT: RefCell<Paint> = RefCell::new(Paint::default());
    }

    fn card_window() -> AppResult<HWND> {
        static STARTED: OnceLock<()> = OnceLock::new();
        STARTED.get_or_init(|| {
            let spawned = std::thread::Builder::new().name("versailles-preview".into()).spawn(run);
            if let Err(err) = spawned {
                tracing::warn!("preview: thread failed to start: {err}");
            }
        });
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let h = CARD.load(Ordering::SeqCst);
            if h != 0 {
                return Ok(HWND(h as *mut _));
            }
            if Instant::now() > until {
                return Err(AppError::msg("windows.preview: the preview window did not start"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wake(card: HWND) {
        unsafe {
            let _ = PostMessageW(Some(card), WAKE, WPARAM(0), LPARAM(0));
        }
    }

    pub fn show(req: Request) -> AppResult<()> {
        let card = card_window()?;
        {
            let mut s = shared().lock().unwrap();
            s.want = Some(req);
            s.hide_at = None;
            s.updated = Instant::now();
        }
        wake(card);
        Ok(())
    }

    pub fn hide() {
        let h = CARD.load(Ordering::SeqCst);
        if h == 0 {
            return;
        }
        {
            let mut s = shared().lock().unwrap();
            if s.want.is_none() {
                return;
            }
            s.hide_at.get_or_insert(Instant::now() + HIDE_GRACE);
        }
        wake(HWND(h as *mut _));
    }

    fn rect_of(r: RECT) -> Rect {
        Rect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top }
    }

    fn win_rect(r: Rect) -> RECT {
        RECT { left: r.x, top: r.y, right: r.right(), bottom: r.bottom() }
    }

    fn contains(r: Rect, p: POINT) -> bool {
        p.x >= r.x && p.x < r.right() && p.y >= r.y && p.y < r.bottom()
    }

    fn title_of(h: HWND) -> String {
        unsafe {
            let len = GetWindowTextLengthW(h).max(0) as usize;
            if len == 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len + 1];
            let n = GetWindowTextW(h, &mut buf).max(0) as usize;
            String::from_utf16_lossy(&buf[..n])
        }
    }

    /// The monitor under `anchor`: its work area and scale.
    fn monitor_of(anchor: Rect) -> (Rect, f64) {
        unsafe {
            let mon = MonitorFromRect(&win_rect(anchor), MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let work = if GetMonitorInfoW(mon, &mut info).as_bool() {
                rect_of(info.rcWork)
            } else {
                Rect { x: 0, y: 0, width: 1920, height: 1080 }
            };
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            (work, dx.max(48) as f64 / 96.0)
        }
    }

    /// The live state of the card's thread.
    struct Live {
        card: HWND,
        thumb: Option<(isize, isize)>, // (source hwnd, thumbnail)
        shown: Option<Request>,
        layout: Option<(Option<(i32, i32)>, String, Rect)>, // (source size, title, card rect)
        hover: bool,
        left_at: Option<Instant>,
    }

    impl Live {
        fn release_thumb(&mut self) {
            if let Some((_, t)) = self.thumb.take() {
                unsafe {
                    let _ = DwmUnregisterThumbnail(t);
                }
            }
        }

        fn hide(&mut self) {
            self.release_thumb();
            self.shown = None;
            self.layout = None;
            self.left_at = None;
            unsafe {
                let _ = ShowWindow(self.card, SW_HIDE);
            }
        }

        fn thumbnail(&mut self, src: HWND) -> Option<isize> {
            let key = src.0 as isize;
            if self.thumb.map(|(s, _)| s) != Some(key) {
                self.release_thumb();
                if let Ok(t) = unsafe { DwmRegisterThumbnail(self.card, src) } {
                    self.thumb = Some((key, t));
                }
            }
            self.thumb.map(|(_, t)| t)
        }

        /// The source's visible part (no invisible resize borders) and its size.
        fn source(&mut self, src: HWND) -> Option<(Option<RECT>, (i32, i32))> {
            let thumb = self.thumbnail(src)?;
            let size = unsafe { DwmQueryThumbnailSourceSize(thumb) }.ok()?;
            if size.cx <= 0 || size.cy <= 0 {
                return None;
            }
            if unsafe { IsIconic(src) }.as_bool() {
                return Some((None, (size.cx, size.cy)));
            }
            unsafe {
                let mut frame = RECT::default();
                let mut win = RECT::default();
                let framed = DwmGetWindowAttribute(
                    src,
                    DWMWA_EXTENDED_FRAME_BOUNDS,
                    &mut frame as *mut RECT as *mut _,
                    std::mem::size_of::<RECT>() as u32,
                )
                .is_ok()
                    && GetWindowRect(src, &mut win).is_ok();
                if framed {
                    let crop = RECT {
                        left: (frame.left - win.left).clamp(0, size.cx),
                        top: (frame.top - win.top).clamp(0, size.cy),
                        right: (frame.right - win.left).clamp(0, size.cx),
                        bottom: (frame.bottom - win.top).clamp(0, size.cy),
                    };
                    let (w, h) = (crop.right - crop.left, crop.bottom - crop.top);
                    if w > 0 && h > 0 {
                        return Some((Some(crop), (w, h)));
                    }
                }
            }
            Some((None, (size.cx, size.cy)))
        }

        fn present(&mut self, req: Request) {
            let src = HWND(req.id as isize as *mut _);
            let title = title_of(src);
            let source = self.source(src);
            let size = source.map(|(_, s)| s);
            let (area, scale) = monitor_of(req.anchor);
            let layout = card(scale, size);
            let side = side(req.caller, req.anchor, area);
            let (x, y) = place(req.anchor, (layout.width, layout.height), side, area, scale);
            let rect = Rect { x, y, width: layout.width, height: layout.height };
            if self.shown == Some(req) && self.layout.as_ref() == Some(&(size, title.clone(), rect)) {
                return;
            }
            let icon = if layout.thumb.is_none() { crate::app_windows::icon_handle(req.id) } else { None };
            PAINT.with(|p| {
                *p.borrow_mut() = Paint { title: title.clone(), card: Some(layout), scale, icon };
            });
            unsafe {
                let r = (RADIUS * scale * 2.0).round() as i32;
                let region = CreateRoundRectRgn(0, 0, rect.width + 1, rect.height + 1, r, r);
                // The window owns the region from here on.
                SetWindowRgn(self.card, Some(region), false);
                let _ = SetWindowPos(
                    self.card,
                    Some(HWND_TOPMOST),
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                if let Some((_, thumb)) = self.thumb {
                    let (crop, _) = source.unwrap_or((None, (0, 0)));
                    let mut props = DWM_THUMBNAIL_PROPERTIES {
                        dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
                        rcDestination: layout.thumb.map(win_rect).unwrap_or_default(),
                        opacity: 255,
                        fVisible: BOOL::from(layout.thumb.is_some()),
                        fSourceClientAreaOnly: BOOL::from(false),
                        ..Default::default()
                    };
                    if let Some(crop) = crop {
                        props.dwFlags |= DWM_TNP_RECTSOURCE;
                        props.rcSource = crop;
                    }
                    let _ = DwmUpdateThumbnailProperties(thumb, &props);
                }
                let _ = InvalidateRect(Some(self.card), None, false);
            }
            if self.shown.map(|s| (s.id, s.anchor)) != Some((req.id, req.anchor)) {
                // A hover preview follows the pointer; one asked for from the
                // keyboard (pointer elsewhere) does not.
                let mut p = POINT::default();
                self.hover = unsafe { GetCursorPos(&mut p) }.is_ok() && contains(req.caller, p);
                self.left_at = None;
            }
            self.shown = Some(req);
            self.layout = Some((size, title, rect));
        }

        /// One look at what the page wants; whether the card is up.
        fn step(&mut self) -> bool {
            let now = Instant::now();
            let (want, hide_at, updated) = {
                let s = shared().lock().unwrap();
                (s.want, s.hide_at, s.updated)
            };
            let drop_it = |live: &mut Live| {
                let mut s = shared().lock().unwrap();
                s.want = None;
                s.hide_at = None;
                live.hide();
            };
            let Some(req) = want else {
                if self.shown.is_some() {
                    self.hide();
                }
                return false;
            };
            if hide_at.is_some_and(|t| now >= t) || now.duration_since(updated) > MAX_IDLE {
                drop_it(self);
                return false;
            }
            if !unsafe { IsWindow(Some(HWND(req.id as isize as *mut _))) }.as_bool() {
                drop_it(self);
                return false;
            }
            if self.hover {
                let mut p = POINT::default();
                let card = self.layout.as_ref().map(|l| l.2);
                let inside = unsafe { GetCursorPos(&mut p) }.is_ok()
                    && (contains(req.caller, p) || card.is_some_and(|c| contains(c, p)));
                if inside {
                    self.left_at = None;
                } else if now.duration_since(*self.left_at.get_or_insert(now)) >= LEFT_FOR {
                    drop_it(self);
                    return false;
                }
            }
            self.present(req);
            true
        }
    }

    unsafe extern "system" fn proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // Placed by `present` for the button's monitor: keep that size.
            WM_DPICHANGED => LRESULT(0),
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(h);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, w, l),
        }
    }

    fn paint(h: HWND) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            PAINT.with(|p| {
                let p = p.borrow();
                let Some(card) = p.card else {
                    return;
                };
                let (w, ht) = (card.width, card.height);
                let mem = CreateCompatibleDC(Some(dc));
                let bmp = CreateCompatibleBitmap(dc, w, ht);
                let old_bmp = SelectObject(mem, HGDIOBJ(bmp.0));
                let px = |v: f64| (v * p.scale).round() as i32;

                let paper = CreateSolidBrush(PAPER);
                let ink = CreateSolidBrush(INK);
                FillRect(mem, &RECT { left: 0, top: 0, right: w, bottom: ht }, ink);
                let pen = CreatePen(PS_INSIDEFRAME, px(STROKE).max(1), INK);
                let old_pen = SelectObject(mem, HGDIOBJ(pen.0));
                let old_brush = SelectObject(mem, HGDIOBJ(paper.0));
                let r = px(RADIUS * 2.0);
                let _ = RoundRect(mem, 0, 0, w, ht, r, r);
                SelectObject(mem, old_brush);
                SelectObject(mem, old_pen);

                let font = CreateFontW(
                    -px(12.0),
                    0,
                    0,
                    0,
                    FW_NORMAL.0 as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    0,
                    w!("Segoe UI Semibold"),
                );
                let old_font = SelectObject(mem, HGDIOBJ(font.0));
                SetBkMode(mem, TRANSPARENT);
                SetTextColor(mem, INK);
                let mut text: Vec<u16> = p.title.encode_utf16().collect();
                if !text.is_empty() {
                    let mut rc = win_rect(card.title);
                    DrawTextW(mem, &mut text, &mut rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                }
                SelectObject(mem, old_font);

                if let (Some(icon), Some(at)) = (p.icon, card.icon) {
                    let _ = DrawIconEx(mem, at.x, at.y, HICON(icon as *mut _), at.width, at.height, 0, None, DI_NORMAL);
                }

                let _ = BitBlt(dc, 0, 0, w, ht, Some(mem), 0, 0, SRCCOPY);
                SelectObject(mem, old_bmp);
                for obj in [HGDIOBJ(bmp.0), HGDIOBJ(paper.0), HGDIOBJ(ink.0), HGDIOBJ(pen.0), HGDIOBJ(font.0)] {
                    let _ = DeleteObject(obj);
                }
                let _ = DeleteDC(mem);
            });
            let _ = EndPaint(h, &ps);
        }
    }

    fn create() -> Option<HWND> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance,
                lpszClassName: w!("VersaillesPreview"),
                ..Default::default()
            });
            let card = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                w!("VersaillesPreview"),
                w!("Versailles preview"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|err| tracing::warn!("preview: window failed: {err}"))
            .ok()?;
            // Opaque, but layered: with WS_EX_TRANSPARENT, clicks pass through.
            let _ = SetLayeredWindowAttributes(card, COLORREF(0), 255, LWA_ALPHA);
            let corner = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                card,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const _,
                std::mem::size_of_val(&corner) as u32,
            );
            Some(card)
        }
    }

    fn run() {
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let Some(card) = create() else {
            return;
        };
        CARD.store(card.0 as isize, Ordering::SeqCst);
        let mut live = Live { card, thumb: None, shown: None, layout: None, hover: false, left_at: None };
        let mut up = false;
        loop {
            // While up: look every 50 ms (pointer, source, title). Hidden:
            // sleep until a call wakes us.
            let pending = shared().lock().unwrap().want.is_some();
            let timeout = if up || pending { 50 } else { u32::MAX };
            unsafe {
                MsgWaitForMultipleObjects(None, false, timeout, QS_ALLINPUT);
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            up = live.step();
        }
    }

    #[cfg(test)]
    unsafe extern "system" fn probe_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        DefWindowProcW(h, msg, w, l)
    }

    /// Show the card for a throwaway window of this test process, grab the
    /// screen under it to a PNG, then close both. Shows over the screen for
    /// about a second: `cargo test --lib live_preview -- --ignored --nocapture`.
    #[cfg(test)]
    pub fn live_preview(out: &std::path::Path) {
        use windows::Win32::Graphics::Gdi::{
            CreateDIBSection, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            DestroyWindow, SetWindowTextW, SW_SHOWNOACTIVATE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };
        unsafe {
            // As in Versailles (per-monitor aware): physical pixels everywhere.
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = HINSTANCE(GetModuleHandleW(None).unwrap().0);
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(probe_proc),
                hInstance: instance,
                lpszClassName: w!("VersaillesPreviewProbe"),
                hbrBackground: CreateSolidBrush(COLORREF(0x0000_80FF)), // orange
                ..Default::default()
            });
            let probe = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("VersaillesPreviewProbe"),
                w!("Preview probe"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                -3000,
                -3000,
                800,
                500,
                None,
                None,
                Some(instance),
                None,
            )
            .expect("probe");
            let _ = ShowWindow(probe, SW_SHOWNOACTIVATE);
            let _ = SetWindowTextW(probe, w!("Versailles preview probe — a long title to check the ellipsis at the end"));
            let pump = |ms: u64| {
                let until = Instant::now() + Duration::from_millis(ms);
                while Instant::now() < until {
                    let mut msg = MSG::default();
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&msg);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            pump(300);
            // A button in the middle of a 44 px bar at the bottom of the primary monitor.
            let (work, scale) = monitor_of(Rect { x: 0, y: 0, width: 1, height: 1 });
            let bar_h = (44.0 * scale) as i32;
            let caller = Rect { x: work.x, y: work.bottom() - bar_h, width: work.width, height: bar_h };
            let anchor = Rect { x: work.center_x() - 80, y: caller.y + 6, width: 160, height: bar_h - 12 };
            show(Request { id: probe.0 as isize as i64, anchor, caller }).expect("show");
            pump(600);
            let (size, rect) = {
                let card = HWND(CARD.load(Ordering::SeqCst) as *mut _);
                let mut r = RECT::default();
                let _ = GetWindowRect(card, &mut r);
                ((r.right - r.left, r.bottom - r.top), rect_of(r))
            };
            eprintln!("card at {rect:?}, scale {scale}, anchor {anchor:?}");
            // Grab the screen under the card.
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size.0,
                    biHeight: -size.1,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).expect("dib");
            let old = SelectObject(mem, HGDIOBJ(dib.0));
            let _ = BitBlt(mem, 0, 0, size.0, size.1, Some(screen), rect.x, rect.y, SRCCOPY | CAPTUREBLT);
            let len = (size.0 * size.1 * 4) as usize;
            let mut bgra = std::slice::from_raw_parts(bits as *const u8, len).to_vec();
            for px in bgra.chunks_mut(4) {
                px[3] = 255;
            }
            SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(dib.0));
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            let rgba = crate::app_windows::bgra_to_rgba(&bgra, None);
            std::fs::write(out, crate::app_windows::png_rgba(size.0 as u32, size.1 as u32, &rgba)).expect("png");
            hide();
            pump(400);
            let _ = DestroyWindow(probe);
            pump(100);
            assert!(!shared().lock().unwrap().want.is_some(), "card still wanted");
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn show(_req: Request) -> AppResult<()> {
        Err(AppError::msg("windows.preview is only available on Windows"))
    }
    pub fn hide() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect { x, y, width, height }
    }

    #[test]
    fn converts_css_rects_with_the_window_scale() {
        let css = CssRect { x: 100.0, y: 6.0, width: 160.5, height: 32.0 };
        // A bar at the bottom of a 150 % monitor whose client area starts at (0, 1374).
        assert_eq!(to_screen(css, (0, 1374), 1.5).unwrap(), r(150, 1383, 241, 48));
        // A second monitor to the left (negative coordinates), 100 %.
        assert_eq!(to_screen(css, (-1920, 1036), 1.0).unwrap(), r(-1820, 1042, 161, 32));
        // A zero-size rect still anchors at one pixel.
        let dot = CssRect { x: 1.0, y: 1.0, width: 0.0, height: 0.0 };
        assert_eq!(to_screen(dot, (0, 0), 2.0).unwrap(), r(2, 2, 1, 1));
        let bad = CssRect { x: f64::NAN, ..css };
        assert!(to_screen(bad, (0, 0), 1.0).is_err());
        assert!(to_screen(CssRect { width: -1.0, ..css }, (0, 0), 1.0).is_err());
        assert!(to_screen(css, (0, 0), 0.0).is_err());
        assert!(to_screen(CssRect { x: 1e9, ..css }, (0, 0), 1.0).is_err());
    }

    #[test]
    fn fits_thumbnails_keeping_the_aspect() {
        assert_eq!(fit((1920, 1080), (360, 240)), Some((360, 203)));
        assert_eq!(fit((800, 1200), (360, 240)), Some((160, 240)));
        // Never enlarged.
        assert_eq!(fit((200, 100), (360, 240)), Some((200, 100)));
        assert_eq!(fit((0, 100), (360, 240)), None);
    }

    #[test]
    fn lays_out_the_card_for_the_monitor_dpi() {
        // 150 %: thumbnail at most 360 × 240 px, 12 px padding.
        let c = card(1.5, Some((2560, 1440)));
        assert_eq!(c.thumb, Some(r(12, 51, 360, 203)));
        assert_eq!((c.width, c.height), (384, 51 + 203 + 12));
        assert_eq!(c.title, r(12, 12, 360, 33));
        assert!(c.icon.is_none());
        // 100 %: a tall window is centred in the minimum width.
        let c = card(1.0, Some((600, 1200)));
        assert_eq!(c.thumb, Some(r(8 + (160 - 80) / 2, 34, 80, 160)));
        assert_eq!(c.width, 176);
        // No thumbnail: icon and title.
        let c = card(1.5, None);
        assert!(c.thumb.is_none());
        assert_eq!(c.icon, Some(r(12, 12 + (60 - 48) / 2, 48, 48)));
        assert_eq!((c.width, c.height), (384, 84));
        assert_eq!(c.title.x, 12 + 48 + 12);
    }

    #[test]
    fn picks_the_side_from_the_bar() {
        let mon = r(0, 0, 2560, 1440);
        let bottom_bar = r(0, 1374, 2560, 66);
        let top_bar = r(0, 0, 2560, 66);
        let left_bar = r(0, 0, 66, 1440);
        let right_bar = r(2494, 0, 66, 1440);
        assert_eq!(side(bottom_bar, r(100, 1380, 200, 54), mon), Side::Above);
        // The first button of a bottom bar is near the left edge too: still above.
        assert_eq!(side(bottom_bar, r(0, 1380, 60, 54), mon), Side::Above);
        assert_eq!(side(top_bar, r(100, 6, 200, 54), mon), Side::Below);
        assert_eq!(side(left_bar, r(6, 400, 54, 54), mon), Side::Right);
        assert_eq!(side(right_bar, r(2500, 400, 54, 54), mon), Side::Left);
    }

    #[test]
    fn places_the_card_on_the_buttons_monitor() {
        // 150 % monitor, work area above a reserved 66 px bottom bar.
        let work = r(0, 0, 2560, 1374);
        let button = r(1000, 1383, 240, 48);
        let (x, y) = place(button, (384, 266), Side::Above, work, 1.5);
        // Centred on the button; 12 px above it would overlap the margin
        // above the bar, so it sits 6 px above the work area's edge.
        assert_eq!((x, y), (1120 - 192, 1374 - 6 - 266));
        // Near the left edge: slid right to the margin.
        let (x, _) = place(r(0, 1383, 60, 48), (384, 266), Side::Above, work, 1.5);
        assert_eq!(x, 6);
        // Near the right edge.
        let (x, _) = place(r(2520, 1383, 40, 48), (384, 266), Side::Above, work, 1.5);
        assert_eq!(x, 2560 - 6 - 384);
        // A top bar: below the button.
        let work = r(0, 66, 2560, 1374);
        let (_, y) = place(r(500, 9, 240, 48), (384, 266), Side::Below, work, 1.5);
        assert_eq!(y, 66 + 6);
        let (_, y) = place(r(500, 9, 240, 48), (384, 266), Side::Below, r(0, 0, 2560, 1440), 1.5);
        assert_eq!(y, 9 + 48 + 12);
        // No room above (a bar near the top of a short screen): below instead.
        let (_, y) = place(r(500, 100, 240, 48), (384, 266), Side::Above, r(0, 0, 1280, 720), 1.0);
        assert_eq!(y, 100 + 48 + 8);
        // A monitor to the left with negative coordinates, 100 %.
        let left = r(-1920, 0, 1920, 1040);
        let (x, y) = place(r(-1910, 1000, 160, 32), (256, 176), Side::Above, left, 1.0);
        assert_eq!((x, y), (-1916, 1000 - 8 - 176));
        // A side bar: beside the button, kept on screen vertically.
        let (x, y) = place(r(6, 1300, 54, 54), (384, 266), Side::Right, r(66, 0, 2494, 1440), 1.5);
        assert_eq!((x, y), (6 + 54 + 12, 1440 - 6 - 266));
        // Larger than the monitor: pinned to its top-left margin.
        let (x, y) = place(r(10, 10, 10, 10), (5000, 5000), Side::Below, r(0, 0, 800, 600), 1.0);
        assert_eq!((x, y), (4, 4));
    }

    #[test]
    fn limits_the_call_rate() {
        let mut limit = RateLimit::new(3);
        let t = Instant::now();
        assert!(limit.allow(t));
        assert!(limit.allow(t + Duration::from_millis(10)));
        assert!(limit.allow(t + Duration::from_millis(20)));
        assert!(!limit.allow(t + Duration::from_millis(30)));
        // A new second.
        assert!(limit.allow(t + Duration::from_millis(1001)));
    }

    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_preview() {
        let out = std::env::temp_dir().join("versailles-preview-live.png");
        imp::live_preview(&out);
        eprintln!("wrote {}", out.display());
    }
}
