use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub status: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub thumbnail_data_url: Option<String>,
    pub source: String,
    #[serde(default)]
    pub has_session: bool,
}

impl PartialEq for MediaInfo {
    fn eq(&self, other: &Self) -> bool {
        self.title == other.title
            && self.artist == other.artist
            && self.album == other.album
            && self.status == other.status
            && self.position_ms / 500 == other.position_ms / 500
            && self.duration_ms == other.duration_ms
            && self.source == other.source
            && self.has_session == other.has_session
            && self.thumbnail_data_url.is_some() == other.thumbnail_data_url.is_some()
    }
}

impl MediaInfo {
    pub fn idle() -> Self {
        Self {
            status: "no-session".into(),
            has_session: false,
            ..Default::default()
        }
    }

    fn looks_active(&self) -> bool {
        self.has_session
            && (self.status == "playing"
                || self.status == "paused"
                || !self.title.trim().is_empty()
                || !self.artist.trim().is_empty())
    }
}

#[derive(Clone, Default)]
pub struct MediaState {
    pub current: Arc<Mutex<MediaInfo>>,
    pub last_error: Arc<Mutex<Option<String>>>,
}

impl MediaState {
    pub fn snapshot(&self) -> MediaInfo {
        self.current.lock().unwrap().clone()
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }
}

/// Require several empty polls before flipping UI to idle (GSMTC flickers).
const IDLE_STREAK_BEFORE_CLEAR: u8 = 4;

pub fn start_media_listener(app: AppHandle, state: MediaState) {
    tauri::async_runtime::spawn(async move {
        let mut last_err: Option<String> = None;
        let mut idle_streak: u8 = 0;
        loop {
            let info = tokio::task::spawn_blocking(poll_media_blocking)
                .await
                .unwrap_or_else(|e| Err(AppError::msg(e.to_string())));

            match info {
                Ok(info) => {
                    {
                        let mut err = state.last_error.lock().unwrap();
                        if err.is_some() {
                            *err = None;
                        }
                    }
                    last_err = None;

                    if info.looks_active() {
                        idle_streak = 0;
                        publish_media(&app, &state, info);
                    } else {
                        idle_streak = idle_streak.saturating_add(1);
                        if idle_streak >= IDLE_STREAK_BEFORE_CLEAR {
                            publish_media(&app, &state, MediaInfo::idle());
                        }
                        // else keep last good snapshot
                    }
                }
                Err(err) => {
                    let msg = err.to_string();
                    if last_err.as_deref() != Some(msg.as_str()) {
                        tracing::warn!("media poll: {msg}");
                        last_err = Some(msg.clone());
                        *state.last_error.lock().unwrap() = Some(msg);
                    }
                    // Never wipe a good session on a transient WinRT failure.
                    idle_streak = idle_streak.saturating_add(1);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        }
    });
}

fn publish_media(app: &AppHandle, state: &MediaState, info: MediaInfo) {
    let to_emit = {
        let mut current = state.current.lock().unwrap();
        if *current == info {
            current.position_ms = info.position_ms;
            current.status = info.status.clone();
            return;
        }

        // Preserve last art when a poll drops an oversized/unavailable thumb.
        let merged = if info.thumbnail_data_url.is_none()
            && current.thumbnail_data_url.is_some()
            && info.title == current.title
            && info.artist == current.artist
            && info.album == current.album
            && info.source == current.source
            && info.has_session
        {
            let mut m = info;
            m.thumbnail_data_url = current.thumbnail_data_url.clone();
            m
        } else {
            info
        };

        *current = merged.clone();
        merged
    };
    let _ = app.emit("media://update", &to_emit);
}

#[cfg(windows)]
fn session_manager() -> AppResult<
    &'static windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager,
> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

    static MANAGER: OnceLock<
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager,
    > = OnceLock::new();

    if let Some(m) = MANAGER.get() {
        return Ok(m);
    }

    let operation = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(|e| AppError::msg(e.to_string()))?;
    let manager = operation.get().map_err(|e| AppError::msg(e.to_string()))?;
    let _ = MANAGER.set(manager);
    MANAGER
        .get()
        .ok_or_else(|| AppError::msg("GSMTC manager init raced"))
}

#[cfg(windows)]
fn session_status_label(
    session: &windows::Media::Control::GlobalSystemMediaTransportControlsSession,
) -> String {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus;
    let playback = session
        .GetPlaybackInfo()
        .ok()
        .and_then(|p| p.PlaybackStatus().ok());
    match playback {
        Some(PlaybackStatus::Playing) => "playing",
        Some(PlaybackStatus::Paused) => "paused",
        Some(PlaybackStatus::Stopped) => "stopped",
        Some(PlaybackStatus::Opened) => "opened",
        Some(PlaybackStatus::Closed) => "closed",
        Some(PlaybackStatus::Changing) => "changing",
        _ => "unknown",
    }
    .to_string()
}

#[cfg(windows)]
fn session_score(
    session: &windows::Media::Control::GlobalSystemMediaTransportControlsSession,
) -> i32 {
    let status = session_status_label(session);
    let mut score = match status.as_str() {
        "playing" => 100,
        "changing" => 80,
        "paused" => 60,
        "opened" => 30,
        "stopped" => 10,
        _ => 0,
    };
    if let Ok(props) = session.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
        let title = props.Title().unwrap_or_default().to_string();
        let artist = props.Artist().unwrap_or_default().to_string();
        if !title.trim().is_empty() {
            score += 25;
        }
        if !artist.trim().is_empty() {
            score += 10;
        }
    }
    score
}

/// Prefer a real Playing session from GetSessions(); GetCurrentSession alone is flaky
/// (often an empty/stale shell session while Brave/Spotify actually play).
#[cfg(windows)]
fn best_session(
) -> AppResult<Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession>> {
    let manager = session_manager()?;

    let mut best: Option<(i32, windows::Media::Control::GlobalSystemMediaTransportControlsSession)> =
        None;

    if let Ok(sessions) = manager.GetSessions() {
        let count = sessions.Size().unwrap_or(0);
        for i in 0..count {
            if let Ok(session) = sessions.GetAt(i) {
                let score = session_score(&session);
                if score <= 0 {
                    continue;
                }
                if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                    best = Some((score, session));
                }
            }
        }
    }

    if let Some((_, session)) = best {
        return Ok(Some(session));
    }

    match manager.GetCurrentSession() {
        Ok(session) => {
            if session_score(&session) > 0 {
                Ok(Some(session))
            } else {
                Ok(None)
            }
        }
        Err(_) => Ok(None),
    }
}

#[cfg(windows)]
fn all_sessions(
) -> AppResult<Vec<windows::Media::Control::GlobalSystemMediaTransportControlsSession>> {
    let manager = session_manager()?;
    let mut out = Vec::new();
    if let Ok(sessions) = manager.GetSessions() {
        let count = sessions.Size().unwrap_or(0);
        for i in 0..count {
            if let Ok(session) = sessions.GetAt(i) {
                out.push(session);
            }
        }
    }
    if out.is_empty() {
        if let Ok(session) = manager.GetCurrentSession() {
            out.push(session);
        }
    }
    // Highest score first for controls.
    out.sort_by_key(|s| std::cmp::Reverse(session_score(s)));
    Ok(out)
}

#[cfg(windows)]
fn poll_media_blocking() -> AppResult<MediaInfo> {
    let Some(session) = best_session()? else {
        return Ok(MediaInfo::idle());
    };

    let props = match session.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
        Ok(p) => p,
        Err(err) => {
            // Soft-fail: still expose playback status/source so UI doesn't go idle.
            tracing::debug!("media properties: {err}");
            let source = session
                .SourceAppUserModelId()
                .map(|s| s.to_string())
                .unwrap_or_else(|_| "unknown".into());
            let status = session_status_label(&session);
            return Ok(MediaInfo {
                title: String::new(),
                artist: String::new(),
                album: String::new(),
                status: status.clone(),
                position_ms: 0,
                duration_ms: 0,
                thumbnail_data_url: None,
                source,
                has_session: status == "playing" || status == "paused" || status == "changing",
            });
        }
    };

    let title = props.Title().unwrap_or_default().to_string();
    let artist = props.Artist().unwrap_or_default().to_string();
    let album = props.AlbumTitle().unwrap_or_default().to_string();
    let status = session_status_label(&session);

    let timeline = session.GetTimelineProperties().ok();
    let position_ms = timeline
        .as_ref()
        .and_then(|t| t.Position().ok())
        .map(|t| (t.Duration / 10_000) as u64)
        .unwrap_or(0);
    let duration_ms = timeline
        .as_ref()
        .and_then(|t| t.EndTime().ok())
        .map(|t| (t.Duration / 10_000) as u64)
        .unwrap_or(0);

    let source = session
        .SourceAppUserModelId()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "unknown".into());

    let thumbnail_data_url = read_thumbnail(&props);

    let has_session = status == "playing"
        || status == "paused"
        || status == "changing"
        || !title.trim().is_empty()
        || !artist.trim().is_empty();

    Ok(MediaInfo {
        title,
        artist,
        album,
        status,
        position_ms,
        duration_ms,
        thumbnail_data_url,
        source,
        has_session,
    })
}

#[cfg(windows)]
fn read_thumbnail(
    props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> Option<String> {
    use base64::Engine;
    use windows::Storage::Streams::DataReader;

    let thumb = props.Thumbnail().ok()?;
    let stream = thumb.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()? as u32;
    // Keep IPC small — multi‑MB art breaks WebView2 postMessage.
    const MAX_BYTES: u32 = 48_000;
    if size == 0 || size > MAX_BYTES {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut buffer = vec![0u8; size as usize];
    reader.ReadBytes(&mut buffer).ok()?;

    let mime = if buffer.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if buffer.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if buffer.starts_with(&[b'R', b'I', b'F', b'F']) {
        "image/webp"
    } else {
        "image/jpeg"
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(buffer);
    Some(format!("data:{mime};base64,{encoded}"))
}

#[cfg(not(windows))]
fn poll_media_blocking() -> AppResult<MediaInfo> {
    Ok(MediaInfo::idle())
}

#[cfg(windows)]
fn try_control_on_sessions(
    mut op: impl FnMut(
        &windows::Media::Control::GlobalSystemMediaTransportControlsSession,
    ) -> AppResult<bool>,
) -> AppResult<()> {
    let sessions = all_sessions()?;
    if sessions.is_empty() {
        return Err(AppError::msg("No media session"));
    }
    let mut last_err: Option<AppError> = None;
    for session in sessions {
        match op(&session) {
            Ok(true) => return Ok(()),
            Ok(false) => continue,
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::msg("Media control rejected by all sessions")))
}

#[cfg(windows)]
pub async fn media_play_pause() -> AppResult<()> {
    tokio::task::spawn_blocking(|| {
        try_control_on_sessions(|session| {
            let ok = session
                .TryTogglePlayPauseAsync()
                .map_err(|e| AppError::msg(e.to_string()))?
                .get()
                .map_err(|e| AppError::msg(e.to_string()))?;
            Ok(ok)
        })
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

#[cfg(windows)]
pub async fn media_next() -> AppResult<()> {
    tokio::task::spawn_blocking(|| {
        try_control_on_sessions(|session| {
            let ok = session
                .TrySkipNextAsync()
                .map_err(|e| AppError::msg(e.to_string()))?
                .get()
                .map_err(|e| AppError::msg(e.to_string()))?;
            Ok(ok)
        })
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

#[cfg(windows)]
pub async fn media_previous() -> AppResult<()> {
    tokio::task::spawn_blocking(|| {
        try_control_on_sessions(|session| {
            let ok = session
                .TrySkipPreviousAsync()
                .map_err(|e| AppError::msg(e.to_string()))?
                .get()
                .map_err(|e| AppError::msg(e.to_string()))?;
            Ok(ok)
        })
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

#[cfg(not(windows))]
pub async fn media_play_pause() -> AppResult<()> {
    Err(AppError::msg("Media controls are Windows-only"))
}

#[cfg(not(windows))]
pub async fn media_next() -> AppResult<()> {
    Err(AppError::msg("Media controls are Windows-only"))
}

#[cfg(not(windows))]
pub async fn media_previous() -> AppResult<()> {
    Err(AppError::msg("Media controls are Windows-only"))
}

#[cfg(windows)]
pub fn get_mouse_position() -> AppResult<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point).map_err(|e| AppError::msg(e.to_string()))?;
    }
    Ok((point.x, point.y))
}

#[cfg(not(windows))]
pub fn get_mouse_position() -> AppResult<(i32, i32)> {
    Ok((0, 0))
}
