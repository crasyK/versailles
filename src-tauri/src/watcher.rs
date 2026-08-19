//! Lightweight directory watcher that reloads open widget webviews on save.
use crate::registry::widgets_root;
use crate::state::AppState;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

fn should_ignore(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/").to_lowercase();
    s.contains("/app/")
        || s.ends_with("/app")
        || s.contains("/.versailles/")
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

                    let touches_hotkeys = event.paths.iter().any(|p| {
                        let s = p.to_string_lossy().replace('\\', "/").to_lowercase();
                        s.ends_with("/desktop/index.html") || s.ends_with("/versailles.json")
                    });
                    if touches_hotkeys {
                        let app2 = app.clone();
                        let _ = app.run_on_main_thread(move || {
                            if let Err(err) = crate::hotkeys::register_page_hotkeys(&app2) {
                                tracing::warn!("hotkey rebind failed: {err}");
                            }
                        });
                    }

                    if let Some(window) = app.get_webview_window("desktop") {
                        // Cross-origin iframe (Vite shell vs :47831 page) — set src, do not touch contentWindow.
                        let _ = window.eval(
                            r#"var p=document.getElementById("page"); if(p&&p.src){var u=new URL(p.src); u.searchParams.set("_v", String(Date.now())); p.src=u.toString();}"#,
                        );
                    }

                    // Never eval/navigate a spawn WebView2 from the watcher — that
                    // hangs the UI thread on Windows. Destroy them; next toggle creates fresh.
                    let app_close = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        let state = app_close.state::<AppState>();
                        let ids: Vec<String> = state
                            .window_manager
                            .lock()
                            .unwrap()
                            .open_widgets()
                            .into_iter()
                            .map(|w| w.id)
                            .collect();
                        for id in ids {
                            let _ = crate::window_manager::close_widget_window(
                                &app_close,
                                &state.window_manager,
                                &id,
                            );
                        }
                    });
                }
                Ok(Err(err)) => tracing::debug!("watch event error: {err}"),
                Err(_) => break,
            }
        }

        // Keep watcher alive until channel closes.
        drop(watcher);
    });
}
