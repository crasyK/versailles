mod api;
mod apps;
mod cli;
mod commands;
mod config;
mod desktop;
mod error;
mod hotkeys;
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

use api::start_runtime_server;
use config::ConfigStore;
use hotkeys::register_launcher_hotkey;
use media::{start_media_listener, MediaState};
use registry::{widgets_root, WidgetRegistry};
use state::AppState;
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use window_manager::spawn_webview_prewarm;
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
            let desktop_enabled = app
                .try_state::<AppState>()
                .map(|state| state.config.lock().unwrap().desktop.enabled)
                .unwrap_or(true);
            if desktop_enabled {
                if let Err(err) = desktop::reveal_desktop_window(app) {
                    tracing::warn!("single-instance desktop reveal failed: {err}");
                    let _ = window_manager::show_launcher(app);
                }
            } else {
                let _ = window_manager::show_launcher(app);
            }
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
            // Bind localhost runtime BEFORE desktop restore so widget URLs work.
            match start_runtime_server(app.handle().clone(), &mut config) {
                Ok(port) => {
                    if let Err(err) = store.save(&config) {
                        tracing::warn!("Failed to persist runtime port {port}: {err}");
                    }
                }
                Err(err) => {
                    tracing::error!("Versailles localhost runtime failed to start: {err}");
                }
            }

            app.manage(AppState {
                store: Mutex::new(store),
                config: Mutex::new(config.clone()),
                registry: Mutex::new(registry),
                media,
            });
            app.manage(pty::PtyState::default());

            spawn_webview_prewarm(app.handle());

            let desktop_item =
                MenuItem::with_id(app, "desktop", "Desktop page", true, None::<&str>)?;
            let launcher_item =
                MenuItem::with_id(app, "launcher", "Launcher", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&desktop_item, &launcher_item, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .icon(
                    app.default_window_icon()
                        .cloned()
                        .ok_or_else(|| "missing tray icon".to_string())?,
                )
                .menu(&menu)
                .tooltip("Versailles")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "launcher" => {
                        let _ = window_manager::show_launcher(app);
                    }
                    "desktop" => {
                        let enable = app.get_webview_window("desktop").is_none();
                        if enable {
                            if let Err(err) = desktop::reveal_desktop_window(app) {
                                tracing::error!("desktop page failed to open: {err}");
                            }
                        } else {
                            let _ = window_manager::close_desktop_window(app);
                        }
                        let state = app.state::<AppState>();
                        let mut config = state.config.lock().unwrap();
                        if config.desktop.enabled != enable {
                            config.desktop.enabled = enable;
                            let _ = state.store.lock().unwrap().save(&config);
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
                        let enable = app.get_webview_window("desktop").is_none();
                        if enable {
                            if let Err(err) = desktop::reveal_desktop_window(app) {
                                tracing::error!("desktop page failed to open: {err}");
                                let _ = window_manager::show_launcher(app);
                            }
                        } else {
                            let _ = window_manager::close_desktop_window(app);
                        }
                        if let Some(state) = app.try_state::<AppState>() {
                            let mut config = state.config.lock().unwrap();
                            if config.desktop.enabled != enable {
                                config.desktop.enabled = enable;
                                let _ = state.store.lock().unwrap().save(&config);
                            }
                        }
                    }
                })
                .build(app)?;

            // Launcher is created lazily on first Alt+Space / tray action.
            let _ = register_launcher_hotkey(app.handle(), &config.launcher_hotkey);
            let _ = crate::commands::apply_autostart(app.handle(), config.autostart);

            // Pre-create launcher + dim off the hotkey path so Alt+Space only shows
            // existing windows (creating two WebView2s on the UI thread hangs Windows).
            let prewarm = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Boot is busy — give WebView2 / compositor a moment before creating more windows.
                tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                for attempt in 1..=4u32 {
                    let app = prewarm.clone();
                    if let Err(err) = prewarm.run_on_main_thread(move || {
                        if let Err(err) = window_manager::ensure_launcher_window(&app) {
                            tracing::warn!("launcher prewarm attempt {attempt}: {err}");
                        }
                    }) {
                        tracing::warn!("launcher prewarm schedule failed: {err}");
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                    if prewarm.get_webview_window("launcher").is_some() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(400 * attempt as u64)).await;
                }
                let app = prewarm.clone();
                let _ = prewarm.run_on_main_thread(move || {
                    if let Err(err) = window_manager::ensure_launcher_dim(&app) {
                        tracing::warn!("launcher-dim prewarm failed: {err}");
                    }
                });
                if prewarm.get_webview_window("launcher").is_none() {
                    tracing::error!(
                        "launcher window missing after prewarm — Alt+Space will create it lazily"
                    );
                } else {
                    tracing::info!("launcher windows ready");
                }
            });

            // Login races often steal Alt+Space — re-register after the shell settles.
            let hotkey_app = app.handle().clone();
            let hotkey_accel = config.launcher_hotkey.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let accel = hotkey_accel.clone();
                let app = hotkey_app.clone();
                let _ = hotkey_app.run_on_main_thread(move || {
                    match register_launcher_hotkey(&app, &accel) {
                        Ok(()) => tracing::info!("launcher hotkey re-registered ({accel})"),
                        Err(err) => tracing::warn!("launcher hotkey re-register failed: {err}"),
                    }
                });
            });

            // Plugin creates a visible 15×15 WS_POPUP at (0,0); hide with retries.
            single_instance_fixup::schedule_hide_helpers();

            watcher::start_widget_watcher(app.handle().clone());

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
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
                        }
                    });
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::toggle_launcher,
            commands::dismiss_launcher,
            commands::media_now,
            commands::media_play_pause_cmd,
            commands::media_next_cmd,
            commands::media_previous_cmd,
            commands::get_runtime_status,
            commands::open_log_folder,
            apps::list_catalog,
            apps::ack_catalog,
            apps::hide_catalog_entry,
            desktop::get_desktop_layout,
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
