//! Lightweight directory watcher that reloads the desktop page and stale
//! windows on save.
//!
//! Trailing-edge debounce: changed paths are collected and handled once, 300 ms
//! after the last event, so editors that truncate-then-write (or write via a
//! temp file + rename) never leave a half-written page cached.
use crate::registry::widgets_root;
use crate::state::AppState;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const DEBOUNCE: Duration = Duration::from_millis(300);

/// Lowercase `/`-separated path relative to the widgets root, or `None` when
/// the path is outside it.
fn rel_key(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/").to_lowercase())
}

/// Matched relative to the Widgets root (not the absolute path — a user named
/// `app` or a `…\dist\…` parent folder must not silence the watcher).
fn should_ignore(rel: &str) -> bool {
    let mut segments = rel.split('/');
    let first = segments.next().unwrap_or("");
    if first == "app" || first == ".versailles" || first == ".deck" {
        return true;
    }
    rel.split('/').any(|s| s == ".git" || s == "node_modules")
}

fn is_relevant(rel: &str) -> bool {
    rel.ends_with("widget.json")
        || rel.ends_with(".html")
        || rel.ends_with(".css")
        || rel.ends_with(".js")
        || rel == "versailles.json"
}

pub fn start_widget_watcher(app: AppHandle) {
    let root = match widgets_root() {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!("widget watcher disabled: {err}");
            return;
        }
    };
    // Blocking recv loop: a dedicated OS thread, never a pinned runtime worker.
    let spawned = std::thread::Builder::new()
        .name("versailles-watcher".into())
        .spawn(move || watch_loop(app, root));
    if let Err(err) = spawned {
        tracing::warn!("widget watcher thread failed: {err}");
    }
}

fn watch_loop(app: AppHandle, root: PathBuf) {
    let (tx, rx) = channel();
    let mut watcher = match RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!("widget watcher disabled: {err}");
            return;
        }
    };

    if let Err(err) = watcher.watch(&root, RecursiveMode::Recursive) {
        tracing::warn!("widget watcher failed to start: {err}");
        return;
    }
    tracing::info!("Watching widgets at {}", root.display());

    let mut pending: HashSet<String> = HashSet::new();
    let mut last_event = Instant::now();
    loop {
        let next = if pending.is_empty() {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            let wait = DEBOUNCE.saturating_sub(last_event.elapsed());
            rx.recv_timeout(wait)
        };
        match next {
            Ok(Ok(event)) => {
                for path in &event.paths {
                    let Some(rel) = rel_key(&root, path) else {
                        continue;
                    };
                    if should_ignore(&rel) || !is_relevant(&rel) {
                        continue;
                    }
                    pending.insert(rel);
                    last_event = Instant::now();
                }
            }
            Ok(Err(err)) => tracing::debug!("watch event error: {err}"),
            Err(RecvTimeoutError::Timeout) => {
                let changed: Vec<String> = pending.drain().collect();
                handle_changes(&app, &root, &changed);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    // Keep watcher alive until channel closes.
    drop(watcher);
}

fn handle_changes(app: &AppHandle, root: &Path, changed: &[String]) {
    let state = app.state::<AppState>();
    let page_rel = state
        .config
        .lock()
        .unwrap()
        .desktop
        .page
        .replace('\\', "/")
        .to_lowercase();
    let page_dir = page_rel.rsplit_once('/').map(|(d, _)| format!("{d}/"));

    let touches_config = changed
        .iter()
        .any(|p| *p == page_rel || p == "versailles.json");
    // Spawn windows load the desktop page; any page asset or SDK edit makes them stale.
    let touches_page_assets = changed.iter().any(|p| {
        *p == page_rel
            || p.starts_with("desktop/")
            || p.starts_with(".sdk/")
            || page_dir.as_deref().is_some_and(|d| p.starts_with(d))
    });

    if touches_config {
        crate::desktop::refresh_page_cache(&state);
        reload_user_config(&state);
    }

    // Rescan registry
    let _ = state.registry.lock().unwrap().scan();
    let _ = app.emit("registry://changed", true);
    let _ = app.emit("desktop://reload", true);

    if touches_config {
        // Hotkey edits apply without a restart (launcher.hotkey / data-hotkey).
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Err(err) = crate::hotkeys::register_page_hotkeys(&app2) {
                tracing::warn!("hotkey rebind failed: {err}");
            }
        });
    }

    for window in crate::window_manager::desktop_window_labels(app)
        .iter()
        .filter_map(|l| app.get_webview_window(l))
    {
        // Cross-origin iframe (Vite shell vs :47831 page) — set src, do not touch contentWindow.
        let _ = window.eval(
            r#"var p=document.getElementById("page"); if(p&&p.src){var u=new URL(p.src); u.searchParams.set("_v", String(Date.now())); p.src=u.toString();}"#,
        );
    }

    rebuild_stale_windows(app, root, changed, touches_page_assets);
}

/// Pull the user part of the config (`#versailles` / `versailles.json`) into
/// the running state. API port/enable and the page path still need a restart.
fn reload_user_config(state: &AppState) {
    let user = match state.store.lock().unwrap().load_user_config() {
        Ok(user) => user,
        Err(err) => {
            tracing::warn!("config reload failed: {err}");
            return;
        }
    };
    let mut config = state.config.lock().unwrap();
    config.apply_user_config(user);
    tracing::info!(
        "config reloaded (hotkey {}, primary {:?})",
        config.launcher_hotkey,
        config.launcher_primary
    );
}

/// A window that is closed now and opened again once WebView2 tore it down.
enum Reopen {
    Spawn(String),
    Folder {
        id: String,
        position: crate::config::Position,
        always_on_top: bool,
    },
}

/// Never eval/navigate a spawn WebView2 from the watcher — that hangs the UI
/// thread on Windows. Close only the windows whose source changed: spawn
/// windows when the page / SDK changed (visible ones come back, hidden ones are
/// rebuilt on the next toggle), folder widgets when their own folder changed
/// (reopened in place). The session is persisted afterwards.
fn rebuild_stale_windows(app: &AppHandle, root: &Path, changed: &[String], page_assets: bool) {
    let state = app.state::<AppState>();
    let cat = crate::desktop::page_catalog(&state);
    let open = state.window_manager.lock().unwrap().open_widgets();
    let folders: Vec<(String, String)> = state
        .registry
        .lock()
        .unwrap()
        .list()
        .iter()
        .filter_map(|w| {
            let rel = rel_key(root, &w.path)?;
            Some((w.manifest.id.clone(), format!("{rel}/")))
        })
        .collect();

    let mut close = Vec::new();
    let mut reopen = Vec::new();
    for w in open {
        if cat.spawnable(&w.id).is_some() {
            if !page_assets {
                continue;
            }
            let visible = app
                .get_webview_window(&w.label)
                .and_then(|win| win.is_visible().ok())
                .unwrap_or(false);
            if visible {
                reopen.push(Reopen::Spawn(w.id.clone()));
            }
            close.push(w.id);
        } else if let Some((_, dir)) = folders.iter().find(|(id, _)| *id == w.id) {
            if changed.iter().any(|p| p.starts_with(dir.as_str())) {
                reopen.push(Reopen::Folder {
                    id: w.id.clone(),
                    position: crate::config::Position { x: w.x, y: w.y },
                    always_on_top: w.always_on_top,
                });
                close.push(w.id);
            }
        }
    }
    if close.is_empty() {
        return;
    }

    let app_close = app.clone();
    let ids = close.clone();
    let _ = app.run_on_main_thread(move || {
        let state = app_close.state::<AppState>();
        for id in &ids {
            let _ = crate::window_manager::close_widget_window(&app_close, &state.window_manager, id);
        }
    });

    let app = app.clone();
    std::thread::spawn(move || {
        // Wait for the deferred closes to finish before re-creating the labels.
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && close
                .iter()
                .any(|id| app.get_webview_window(&crate::window_manager::widget_label(id)).is_some())
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let state = handle.state::<AppState>();
            for item in reopen {
                let result = match &item {
                    Reopen::Spawn(id) => crate::window_manager::toggle_slideout_widget(
                        &handle,
                        &state.window_manager,
                        id,
                    )
                    .map(|_| ()),
                    Reopen::Folder {
                        id,
                        position,
                        always_on_top,
                    } => crate::window_manager::open_widget_window(
                        &handle,
                        &state.window_manager,
                        id,
                        Some(position.clone()),
                        Some(*always_on_top),
                    )
                    .map(|_| ()),
                };
                if let Err(err) = result {
                    tracing::warn!("rebuild after edit failed: {err}");
                }
            }
            crate::window_manager::persist_session(&handle);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_is_relative_to_the_widgets_root() {
        let root = PathBuf::from(r"C:\Users\app\Documents\dist\Widgets");
        let page = root.join("desktop").join("index.html");
        let rel = rel_key(&root, &page).unwrap();
        assert_eq!(rel, "desktop/index.html");
        assert!(!should_ignore(&rel));
        assert!(is_relevant(&rel));
        assert!(should_ignore(&rel_key(&root, &root.join("app").join("src").join("x.ts")).unwrap()));
        assert!(should_ignore(".versailles/config.json"));
        assert!(should_ignore("desktop/node_modules/x.js"));
        assert!(should_ignore("archive/legacy-widgets/dice/.git/x.js"));
        assert!(!should_ignore("desktop/target/x.js"));
        assert!(is_relevant("versailles.json"));
        assert!(!is_relevant("shortcuts.json"));
        assert!(rel_key(&root, Path::new(r"D:\elsewhere\x.js")).is_none());
    }
}
