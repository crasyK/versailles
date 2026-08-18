//! Lightweight directory watcher that reloads open widget webviews on save.
use crate::registry::widgets_root;
use crate::state::AppState;
use crate::window_manager::widget_label;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

fn should_ignore(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/").to_lowercase();
    s.contains("/app/")
        || s.ends_with("/app")
        || s.contains("/.deck/")
        || s.contains("/.git/")
        || s.contains("/node_modules/")
        || s.contains("/target/")
        || s.contains("/dist/")
}

pub fn start_widget_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let root = match widgets_root() {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("widget watcher disabled: {err}");
                return;
            }
        };

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
        let mut last_emit = Instant::now() - Duration::from_secs(1);

        loop {
            match rx.recv() {
                Ok(Ok(event)) => {
                    let touches_widget = event.paths.iter().any(|p| {
                        if should_ignore(p) {
                            return false;
                        }
                        let s = p.to_string_lossy().to_lowercase();
                        s.contains("widget.json")
                            || s.ends_with(".html")
                            || s.ends_with(".css")
                            || s.ends_with(".js")
                    });
                    if !touches_widget {
                        continue;
                    }
                    if last_emit.elapsed() < Duration::from_millis(400) {
                        continue;
                    }
                    last_emit = Instant::now();

                    // Rescan registry
                    {
                        let state = app.state::<AppState>();
                        let _ = state.registry.lock().unwrap().scan();
                    }
                    let _ = app.emit("registry://changed", true);
                    let _ = app.emit("desktop://reload", true);

                    if let Some(window) = app.get_webview_window("desktop") {
                        // Cross-origin iframe (Vite shell vs :47831 page) — set src, do not touch contentWindow.
                        let _ = window.eval(
                            r#"var p=document.getElementById("page"); if(p&&p.src){var u=new URL(p.src); u.searchParams.set("_deck", String(Date.now())); p.src=u.toString();}"#,
                        );
                    }

                    // Reload open widget windows
                    let open_ids: Vec<String> = {
                        let state = app.state::<AppState>();
                        let mgr = state.window_manager.lock().unwrap();
                        mgr.open_widgets().into_iter().map(|w| w.id).collect()
                    };
                    for id in open_ids {
                        if let Some(window) = app.get_webview_window(&widget_label(&id)) {
                            let _ = window.eval("location.reload()");
                        }
                    }
                }
                Ok(Err(err)) => tracing::debug!("watch event error: {err}"),
                Err(_) => break,
            }
        }

        // Keep watcher alive until channel closes.
        drop(watcher);
    });
}
