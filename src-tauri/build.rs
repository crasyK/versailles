fn main() {
    // first_run.rs embeds the starter with include_dir!, which tracks the
    // files it saw but not files added later.
    println!("cargo:rerun-if-changed=../templates/starter");
    // Load comctl32 on first use instead of at start. The app's manifest
    // still picks version 6 then; test executables have no manifest, and
    // when the linker keeps Tauri's runtime in them (it depends on how code
    // is split into units), the version-6-only TaskDialogIndirect import made
    // them fail to start (STATUS_ENTRYPOINT_NOT_FOUND).
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }
    // Keep in sync with lib.rs invoke_handler — `npm run verify:acl` fails the build if not.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "list_widgets",
                "open_widget",
                "close_widget",
                "list_open_widgets",
                "move_widget_cmd",
                "layout_update",
                "storage_op",
                "clipboard_read",
                "clipboard_write",
                "power_action",
                "quick_set",
                "bus_publish",
                "sense_now",
                "monitor_info",
                "windows_list",
                "windows_icon",
                "windows_control",
                "windows_preview",
                "tray_list",
                "tray_click",
                "tray_hover",
                "set_widget_always_on_top",
                "set_widget_opacity",
                "get_mouse_position_cmd",
                "get_monitors",
                "toggle_launcher",
                "dismiss_launcher",
                "arm_overlay_focus_guard",
                "focus_spawnable",
                "get_spawnable_engine_context",
                "get_engine_runtime",
                "patch_engine_runtime",
                "cli_exec",
                "cli_exec_term",
                "cli_open",
                "cli_home",
                "cli_search_files",
                "cli_search",
                "cli_recent_files",
                "sys_stats",
                "pty_open",
                "pty_write",
                "pty_resize",
                "pty_is_alive",
                "pty_close",
                "list_layouts",
                "save_layout",
                "apply_layout",
                "load_layout",
                "media_now",
                "media_play_pause_cmd",
                "media_next_cmd",
                "media_previous_cmd",
                "clear_guides",
                "popup_widget_menu",
                "get_api_info",
                "get_runtime_status",
                "open_log_folder",
                "list_catalog",
                "ack_catalog",
                "hide_catalog_entry",
                "catalog_icon",
                "get_desktop_layout",
                "get_desktop_html",
                "toggle_desktop_surface",
                "open_desktop_surface",
                "close_desktop_surface",
                "show_launcher_seeded",
                "toggle_slideout",
                "shell_show_desktop",
                // Only the first-run window (capabilities/welcome.json).
                "welcome_setup",
                "welcome_skip",
            ]),
        ),
    )
    .expect("failed to run tauri-build");
}
