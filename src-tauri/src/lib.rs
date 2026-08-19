mod api;
mod apps;
mod cli;
mod commands;
mod config;
mod desktop;
mod error;
mod page;
mod hotkeys;
mod layout;
mod media;
mod protocol;
mod quote;
mod pty;
mod registry;
mod single_instance_fixup;
mod state;
mod watcher;
mod weather;
mod window_manager;

use api::start_api_server;
use config::ConfigStore;
use hotkeys::register_page_hotkeys;
use media::{start_media_listener, MediaState};
use registry::{widgets_root, WidgetRegistry};
use state::AppState;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use window_manager::{
    open_widget_window, spawn_webview_prewarm, WindowManager,
};
use tauri_plugin_log::{Target, TargetKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Never attach a console in release — stdout logging was popping a terminal on boot.
    #[cfg(all(windows, not(debug_assertions)))]
    {
        use windows::Win32::System::Console::FreeConsole;
        unsafe {
            let _ = FreeConsole();
        }
    }

    let log_builder = {
        let mut b = tauri_plugin_log::Builder::new().level(log::LevelFilter::Info);
        #[cfg(debug_assertions)]
        {
            b = b.targets([
                Target::new(TargetKind::Stdout),
                Target::new(TargetKind::LogDir {
                    file_name: Some("Versailles".into()),
                }),
            ]);
        }
        #[cfg(not(debug_assertions))]
        {
            // File only — Stdout on a GUI app can still surface a console via Windows Terminal.
            b = b.targets([Target::new(TargetKind::LogDir {
                file_name: Some("Versailles".into()),
            })]);
        }
        b
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(log_builder.build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--autostart"])
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = window_manager::show_launcher(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .register_uri_scheme_protocol("widget", |_ctx, request| {
            protocol::serve_widget_request(request)
        })
        .setup(|app| {
            let root = widgets_root()?;
            let store = ConfigStore::new(&root)?;
            let config = store.load()?;
            let mut registry = WidgetRegistry::new(root);
            let _ = registry.scan();

            let media = MediaState::default();
            start_media_listener(app.handle().clone(), media.clone());

            let mut config = config;
            config.api_bound_port = None;
            // Bind file/API server BEFORE session restore so widget URLs work.
            match start_api_server(app.handle().clone(), &mut config) {
                Ok(port) => {
                    if let Err(err) = store.save_runtime_from_app(&config) {
                        tracing::warn!("Failed to persist runtime state for API port {port}: {err}");
                    }
                }
                Err(err) => {
                    tracing::error!("Versailles HTTP server failed to start: {err}");
                }
            }

            app.manage(AppState {
                store: Mutex::new(store),
                config: Mutex::new(config.clone()),
                registry: Mutex::new(registry),
                window_manager: Mutex::new(WindowManager::default()),
                media,
                tray_desktop_item: Mutex::new(None),
            });
            app.manage(pty::PtyState::default());

            spawn_webview_prewarm(app.handle());

            // Tray — only visible UI on boot unless restoring session widgets.
            // Left-click still opens the action bar; the menu is desktop page + quit.
            let desktop_item = MenuItem::with_id(
                app,
                "desktop",
                desktop::desktop_tray_label(false),
                true,
                None::<&str>,
            )?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&desktop_item, &quit_item])?;
            *app.state::<AppState>().tray_desktop_item.lock().unwrap() = Some(desktop_item);

            let _tray = TrayIconBuilder::new()
                .icon(
                    app.default_window_icon()
                        .cloned()
                        .ok_or_else(|| "missing tray icon".to_string())?,
                )
                .menu(&menu)
                .tooltip("Versailles")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "desktop" => {
                        let enable = app.get_webview_window("desktop").is_none();
                        if enable {
                            if let Err(err) = desktop::reveal_desktop_window(app) {
                                tracing::error!("desktop page failed to open: {err}");
                            }
                        } else if let Err(err) = desktop::hide_desktop_window(app) {
                            tracing::error!("desktop page failed to close: {err}");
                        }
                        let state = app.state::<AppState>();
                        let mut config = state.config.lock().unwrap();
                        if config.desktop.enabled != enable {
                            config.desktop.enabled = enable;
                            let _ = state.store.lock().unwrap().save_runtime_from_app(&config);
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(id) = window_manager::hotkey_piece_id(app) {
                            window_manager::toggle_hotkey_piece(app, &id);
                        }
                    }
                })
                .build(app)?;

            // Spawnable hotkeys (data-hotkey on pieces with the hotkey hook).
            if let Err(err) = register_page_hotkeys(app.handle()) {
                tracing::warn!("page hotkeys failed: {err}");
            }
            let _ = crate::commands::apply_autostart(app.handle(), config.autostart);

            // Pre-create the dim window off the hotkey path so Alt+Space only
            // shows existing surfaces (creating WebView2 on the UI thread hangs Windows).
            let prewarm = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                let app = prewarm.clone();
                let _ = prewarm.run_on_main_thread(move || {
                    if let Err(err) = window_manager::ensure_launcher_dim(&app) {
                        tracing::warn!("overlay-dim prewarm failed: {err}");
                    }
                });
                tracing::info!("overlay dim ready");
            });

            // Login races often steal Alt+Space — re-register after the shell settles.
            let hotkey_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let app = hotkey_app.clone();
                let _ = hotkey_app.run_on_main_thread(move || {
                    match register_page_hotkeys(&app) {
                        Ok(()) => tracing::info!("page hotkeys re-registered"),
                        Err(err) => tracing::warn!("page hotkeys re-register failed: {err}"),
                    }
                });
            });

            // Plugin creates a visible 15×15 WS_POPUP at (0,0); hide with retries.
            single_instance_fixup::schedule_hide_helpers();

            watcher::start_widget_watcher(app.handle().clone());

            // Session restore off the setup path; bounded to 2 concurrent WebView2
            // creations to avoid the documented deadlock risk.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Brief settle delay: the file server is already bound in setup,
                // so only wait for monitors so clamp/restore use real geometry.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let (session, page_html) = {
                    let state = handle.state::<AppState>();
                    let html = crate::desktop::read_desktop_html(&state);
                    let session = state.config.lock().unwrap().session_widgets.clone();
                    (session, html)
                };

                let semaphore = Arc::new(Semaphore::new(2));
                let mut join_set = JoinSet::new();
                for widget in session {
                    if crate::desktop::html_embeds_widget(&page_html, &widget.id) {
                        continue;
                    }
                    if crate::page::parse_page(&page_html)
                        .spawnable(&widget.id)
                        .is_some()
                    {
                        continue;
                    }
                    let app_handle = handle.clone();
                    let semaphore = Arc::clone(&semaphore);
                    let id = widget.id.clone();
                    let position = widget.position.clone();
                    let always_on_top = widget.always_on_top;

                    join_set.spawn(async move {
                        let _permit = semaphore
                            .acquire_owned()
                            .await
                            .expect("session restore semaphore closed");

                        let (tx, rx) = tokio::sync::oneshot::channel();
                        let app = app_handle.clone();
                        let id_for_thread = id.clone();
                        if let Err(err) = app_handle.run_on_main_thread(move || {
                            let result = {
                                let state = app.state::<AppState>();
                                open_widget_window(
                                    &app,
                                    &state.window_manager,
                                    &id_for_thread,
                                    Some(position),
                                    Some(always_on_top),
                                )
                            };
                            let _ = tx.send(result);
                        }) {
                            tracing::warn!("Restore schedule failed for {id}: {err}");
                            return;
                        }

                        match rx.await {
                            Ok(Err(err)) => tracing::warn!("Failed to restore {id}: {err}"),
                            Err(_) => tracing::warn!("Restore result channel closed for {id}"),
                            Ok(Ok(_)) => {}
                        }
                    });
                }

                while join_set.join_next().await.is_some() {}

                // Final pass: re-apply saved physical positions after all windows exist
                // (creation/show can nudge them once).
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                let session = {
                    let state = handle.state::<AppState>();
                    let config = state.config.lock().unwrap();
                    config.session_widgets.clone()
                };
                for widget in session {
                    if crate::desktop::html_embeds_widget(&page_html, &widget.id) {
                        continue;
                    }
                    if crate::page::parse_page(&page_html)
                        .spawnable(&widget.id)
                        .is_some()
                    {
                        continue;
                    }
                    let app = handle.clone();
                    let id = widget.id.clone();
                    let x = widget.position.x;
                    let y = widget.position.y;
                    let _ = handle.run_on_main_thread(move || {
                        let state = app.state::<AppState>();
                        let _ = crate::window_manager::apply_position(
                            &app,
                            &state.window_manager,
                            &id,
                            x,
                            y,
                        );
                    });
                }

                let desktop_enabled = {
                    let state = handle.state::<AppState>();
                    let enabled = state.config.lock().unwrap().desktop.enabled;
                    enabled
                };
                if desktop_enabled {
                    let app = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        if let Err(err) = desktop::reveal_desktop_window(&app) {
                            tracing::warn!("desktop surface restore failed: {err}");
                            desktop::set_desktop_tray_label(&app, false);
                        }
                    });
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_widgets,
            commands::open_widget,
            commands::close_widget,
            commands::list_open_widgets,
            commands::move_widget_cmd,
            commands::set_widget_always_on_top,
            commands::set_widget_opacity,
            commands::get_mouse_position_cmd,
            commands::get_monitors,
            commands::toggle_launcher,
            commands::dismiss_launcher,
            commands::list_layouts,
            commands::save_layout,
            commands::apply_layout,
            commands::load_layout,
            commands::media_now,
            commands::media_play_pause_cmd,
            commands::media_next_cmd,
            commands::media_previous_cmd,
            commands::clear_guides,
            commands::popup_widget_menu,
            commands::get_api_info,
            commands::get_runtime_status,
            commands::open_log_folder,
            apps::list_catalog,
            apps::ack_catalog,
            apps::hide_catalog_entry,
            commands::toggle_slideout,
            desktop::get_desktop_layout,
            desktop::get_desktop_html,
            desktop::toggle_desktop_surface,
            desktop::open_desktop_surface,
            desktop::close_desktop_surface,
            desktop::show_launcher_seeded,
            desktop::shell_show_desktop,
            cli::cli_exec,
            cli::cli_exec_term,
            cli::cli_open,
            cli::cli_home,
            cli::cli_search_files,
            cli::sys_stats,
            pty::pty_open,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_is_alive,
            pty::pty_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Versailles");
}
