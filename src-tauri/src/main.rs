#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Release builds: this process becomes a small supervisor that runs the app
    // in a child and restarts it after a crash (see src/crash.rs).
    if let Some(code) = versailles_lib::supervise_if_needed() {
        std::process::exit(code);
    }
    versailles_lib::run();
}
