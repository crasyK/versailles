//! In-app updates (#41) with `tauri-plugin-updater`, driven from Rust only
//! (pages get no updater permission).
//!
//! - A check runs 60 s after start and then once a day, unless `#versailles`
//!   has `"updates": { "check": false }` (read before every check). The
//!   endpoint is the `latest.json` attached to the newest GitHub release.
//! - When a newer release exists, the tray menu shows "Update to x.y.z" at
//!   the top; otherwise the item is not in the menu. Nothing downloads or
//!   installs without a click.
//! - On a click: download (the plugin checks the minisign signature against
//!   the public key built into the app), then [`UPDATING`] stops config
//!   re-apply (taskbar, docks; the tray follows the taskbar), then on the main
//!   thread the same releases as a Quit (`tray`, `taskbar`, `docks`), then
//!   `install`, which starts the NSIS installer (passive: a progress bar, no
//!   questions) and ends the process with `exit(0)`. `RunEvent::Exit` never
//!   fires on that path, which is why the releases run here first. The
//!   supervisor sees exit code 0 and ends too; the installer relaunches the
//!   new version.
//! - If the download or the installer start fails: clear the flag, re-apply
//!   the config, log it, and offer the item again.
//!
//! Testing (debug and test builds only): `VERSAILLES_UPDATE_URL` replaces the
//! endpoint and the first check runs after 5 s. See CONTRIBUTING.

use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tauri::menu::{Menu, MenuItem};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Endpoint override for tests (debug builds and `build:test` builds).
const URL_ENV: &str = "VERSAILLES_UPDATE_URL";
const FIRST_CHECK: Duration = Duration::from_secs(60);
const FIRST_CHECK_TESTING: Duration = Duration::from_secs(5);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The loop wakes this often and checks when a day has passed on the wall
/// clock (a monotonic sleep may not count time the machine slept).
const WAKE_EVERY: Duration = Duration::from_secs(60 * 60);
/// Marks the placeholder in tauri.conf.json (no key pair made yet).
const PLACEHOLDER: &str = "REPLACE_WITH";

/// Set from a successful download until the installer takes over (or the
/// attempt fails): config reloads and dock watchers must not take the
/// taskbar, the tray or screen space back meanwhile.
static UPDATING: AtomicBool = AtomicBool::new(false);
/// A click is being handled (download or install running).
static BUSY: AtomicBool = AtomicBool::new(false);
/// `on_before_exit` ran: the app is torn down and must exit.
static TORN_DOWN: AtomicBool = AtomicBool::new(false);

pub fn updating() -> bool {
    UPDATING.load(Ordering::SeqCst)
}

struct Tray {
    menu: Menu<Wry>,
    item: MenuItem<Wry>,
    shown: bool,
}

fn tray() -> &'static Mutex<Option<Tray>> {
    static TRAY: OnceLock<Mutex<Option<Tray>>> = OnceLock::new();
    TRAY.get_or_init(|| Mutex::new(None))
}

fn pending() -> &'static Mutex<Option<Update>> {
    static PENDING: OnceLock<Mutex<Option<Update>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}

/// The tray menu and its (not yet inserted) update item, from `build_tray`.
pub fn set_tray(menu: Menu<Wry>, item: MenuItem<Wry>) {
    *tray().lock().unwrap() = Some(Tray { menu, item, shown: false });
}

/// Show the item with `text` at the top of the menu, or take it out (`None`).
fn show_item(text: Option<&str>, enabled: bool) {
    let mut slot = tray().lock().unwrap();
    let Some(t) = slot.as_mut() else {
        return;
    };
    match text {
        Some(text) => {
            let _ = t.item.set_text(text);
            let _ = t.item.set_enabled(enabled);
            if !t.shown && t.menu.insert(&t.item, 0).is_ok() {
                t.shown = true;
            }
        }
        None => {
            if t.shown && t.menu.remove(&t.item).is_ok() {
                t.shown = false;
            }
        }
    }
}

fn item_text(version: &str) -> String {
    format!("Update to {version}")
}

/// Whether the public key in this build is a real one.
fn has_pubkey(app: &AppHandle) -> bool {
    pubkey_is_real(
        app.config()
            .plugins
            .0
            .get("updater")
            .and_then(|u| u.get("pubkey"))
            .and_then(|k| k.as_str()),
    )
}

pub fn pubkey_is_real(key: Option<&str>) -> bool {
    key.map(str::trim)
        .is_some_and(|k| k.len() > 40 && !k.contains(PLACEHOLDER) && !k.contains(' '))
}

/// The test endpoint, when this build may use one: debug builds, and test
/// builds (whose config allows plain-http endpoints for a local server).
fn test_endpoint(app: &AppHandle) -> Option<url::Url> {
    let raw = std::env::var(URL_ENV).ok().filter(|s| !s.trim().is_empty())?;
    let insecure_ok = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("dangerousInsecureTransportProtocol"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !(cfg!(debug_assertions) || insecure_ok) {
        tracing::warn!("updates: {URL_ENV} is ignored in release builds");
        return None;
    }
    match url::Url::parse(raw.trim()) {
        Ok(url) => Some(url),
        Err(err) => {
            tracing::warn!("updates: {URL_ENV} is not a URL ({err})");
            None
        }
    }
}

fn checks_enabled(app: &AppHandle) -> bool {
    app.state::<AppState>().config.lock().unwrap().updates.check
}

/// Start the check loop (after the tray exists). Not in bench / e2e mode.
pub fn start(app: &AppHandle) {
    if crate::boot::is_bench() {
        return;
    }
    if !has_pubkey(app) {
        tracing::info!(
            "updates: off in this build (no updater public key in tauri.conf.json; see CONTRIBUTING)"
        );
        return;
    }
    let testing = test_endpoint(app).is_some();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(if testing { FIRST_CHECK_TESTING } else { FIRST_CHECK }).await;
        let mut last: Option<SystemTime> = None;
        loop {
            let due = last.is_none_or(|t| t.elapsed().map_or(true, |e| e >= CHECK_EVERY));
            if due && !BUSY.load(Ordering::SeqCst) {
                if checks_enabled(&app) {
                    check(&app).await;
                    last = Some(SystemTime::now());
                } else if last.is_none() {
                    tracing::info!("updates: checks are off (\"updates\": {{ \"check\": false }})");
                    last = Some(SystemTime::now());
                }
            }
            tokio::time::sleep(WAKE_EVERY).await;
        }
    });
}

fn updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let handle = app.clone();
    // Replaces the plugin's default hook (which only calls
    // `cleanup_before_exit`): runs right before the installer starts and the
    // process exits. The tray, taskbar and docks are released already.
    let mut builder = app.updater_builder().on_before_exit(move || {
        TORN_DOWN.store(true, Ordering::SeqCst);
        tracing::info!("updates: starting the installer");
        handle.cleanup_before_exit();
    });
    if let Some(url) = test_endpoint(app) {
        builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    builder.build().map_err(|e| e.to_string())
}

async fn check(app: &AppHandle) {
    let result = match updater(app) {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(err) => Err(err),
    };
    match result {
        Ok(Some(update)) => {
            tracing::info!(
                "updates: {} is available (this is {})",
                update.version,
                update.current_version
            );
            show_item(Some(&item_text(&update.version)), true);
            *pending().lock().unwrap() = Some(update);
        }
        Ok(None) => {
            tracing::info!("updates: up to date");
            *pending().lock().unwrap() = None;
            show_item(None, false);
        }
        // Offline, GitHub down, no release yet: try again tomorrow.
        Err(err) => tracing::warn!("updates: check failed ({err})"),
    }
}

/// Tray "Update to x.y.z".
pub fn install_clicked(app: &AppHandle) {
    let Some(update) = pending().lock().unwrap().clone() else {
        return;
    };
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let version = update.version.clone();
    show_item(Some(&format!("Downloading {version}…")), false);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tracing::info!("updates: downloading {version}");
        // Verifies the signature before returning the bytes.
        let bytes = match update.download(|_, _| {}, || {}).await {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::error!("updates: download of {version} failed ({err})");
                give_up(&app, &version);
                return;
            }
        };
        UPDATING.store(true, Ordering::SeqCst);
        let handle = app.clone();
        let scheduled = app.run_on_main_thread(move || {
            // The releases `RunEvent::Exit` does on a Quit.
            crate::crash::note_exiting();
            crate::tray::release();
            crate::taskbar::release();
            crate::docks::release_all(&handle);
            // Returns only on failure; on success the process exits.
            if let Err(err) = update.install(bytes) {
                if TORN_DOWN.load(Ordering::SeqCst) {
                    // The windows are gone: restart through the supervisor.
                    crate::crash::record_fatal(&format!("update installer failed to start: {err}"));
                    std::process::exit(1);
                }
                tracing::error!("updates: install of {} failed ({err})", update.version);
                crate::crash::note_exit_cancelled();
                give_up(&handle, &update.version);
            }
        });
        if let Err(err) = scheduled {
            tracing::error!("updates: could not reach the main thread ({err})");
            give_up(&app, &version);
        }
    });
}

/// After a failed attempt: take the taskbar, docks and tray back as the
/// config says, and offer the update again.
fn give_up(app: &AppHandle, version: &str) {
    UPDATING.store(false, Ordering::SeqCst);
    let hide = app.state::<AppState>().config.lock().unwrap().shell.hide_taskbar;
    crate::taskbar::apply(hide);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || crate::docks::reconcile(&handle));
    show_item(Some(&item_text(version)), true);
    BUSY.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_key_turns_updates_off() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(
            updater["endpoints"][0].as_str(),
            Some("https://github.com/crasyK/versailles/releases/latest/download/latest.json")
        );
        assert_eq!(updater["windows"]["installMode"].as_str(), Some("passive"));
        // Shipping builds never accept plain-http endpoints.
        assert!(updater.get("dangerousInsecureTransportProtocol").is_none());
        assert!(!pubkey_is_real(Some("REPLACE_WITH_UPDATER_PUBLIC_KEY")));
        assert!(!pubkey_is_real(None));
        assert!(!pubkey_is_real(Some("")));
        // `tauri signer generate` prints the public key as one base64 line.
        let real = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDEyMzQ1Njc4OUFCQ0RFRgpSV1RBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=";
        assert!(pubkey_is_real(Some(real)));
    }

    #[test]
    fn test_config_only_loosens_the_transport() {
        let test: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.test.conf.json")).unwrap();
        assert_eq!(
            test,
            serde_json::json!({ "plugins": { "updater": { "dangerousInsecureTransportProtocol": true } } })
        );
    }
}
