//! The page SDK (`.sdk/versailles.js`) ships inside the app. On start the
//! host compares the desktop's copy with it: an unchanged older release is
//! replaced (the old file kept as `versailles.js.bak`); a copy edited by hand
//! is left alone and logged once. A missing copy is served from the app by
//! `/files/.sdk/versailles.js`. Nothing outside `.sdk/` is ever written.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The SDK this host ships (the starter's copy).
pub const BUNDLED: &str = include_str!("../../templates/starter/.sdk/versailles.js");

/// Published path of the SDK under `/files/`.
pub const FILES_PATH: &str = ".sdk/versailles.js";

/// Every SDK that shipped, by content hash (see [`content_hash`]): a desktop
/// copy with one of these hashes is unmodified and safe to replace.
/// Never edit a released entry. When the SDK changes for a release, bump the
/// marker on its first line and add a line for the new version; while it is
/// unreleased, update the last line (the test prints the hash).
pub const SHIPPED: &[(&str, u64)] = &[
    ("0.1.5", 0x3f8f_e36e_5882_8410),
    ("0.1.6", 0xc3ec_4875_75de_38cf),
    ("0.2.0", 0x2358_1e54_ec0d_e611),
    ("0.3.0", 0x46c4_485d_5879_e603),
    ("0.4.0", 0xae01_c999_cc1c_431f),
    ("0.4.1", 0x59c5_d244_c4f2_e422),
    ("0.5.0-dev", 0x752f_0a90_91b6_00c7),
    ("0.5.0", 0xa3b9_6eba_04bb_a482),
    ("0.6.0", 0x522d_32e1_71f5_b25d),
    ("0.7.0", 0x1899_1130_1e34_5743),
    // Installed from a test build before Wi-Fi / Bluetooth (#34).
    ("0.9.0-dev1", 0xafe0_42f0_577d_f647),
    // Installed from a test build before notify-control / dnd (#33).
    ("0.9.0-dev2", 0x8227_2d77_d9e3_9cbe),
    ("0.9.0", 0xfda8_3495_1985_eba2),
    ("0.10.0", 0x3423_a61f_e74b_679e),
];

/// Version on the SDK's first line (`/*! versailles-sdk 0.6.0 */`). Releases
/// before 0.6.0 have none.
pub fn marker(sdk: &str) -> Option<&str> {
    let first = sdk.trim_start_matches('\u{feff}').lines().next()?;
    let rest = first.split("versailles-sdk ").nth(1)?;
    let version = rest.split(|c: char| c.is_whitespace() || c == '*').next()?;
    (!version.is_empty()).then_some(version)
}

/// FNV-1a 64 of the text with any BOM and `\r` removed, so a copy checked
/// out with CRLF line endings matches its release.
pub fn content_hash(sdk: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in sdk.trim_start_matches('\u{feff}').bytes().filter(|b| *b != b'\r') {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `0.10.1` > `0.9.0`; pre-release suffixes (`-dev`) sort before the release.
fn version_key(v: &str) -> (Vec<u64>, bool) {
    let (core, pre) = match v.split_once('-') {
        Some((core, _)) => (core, true),
        None => (v, false),
    };
    (core.split('.').map(|n| n.parse().unwrap_or(0)).collect(), !pre)
}

/// What to do with the desktop's SDK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "version")]
pub enum Decision {
    /// No `.sdk/versailles.js`: `/files` serves the bundled one.
    Missing,
    /// Same as the bundled SDK.
    Current,
    /// An unmodified older release (its version): replace it.
    Update(String),
    /// Changed by hand: leave it.
    Edited,
    /// Marked newer than this host's SDK (a newer Versailles wrote it): leave it.
    Newer(String),
}

pub fn decide(desktop: Option<&str>, bundled: &str) -> Decision {
    let Some(desktop) = desktop else {
        return Decision::Missing;
    };
    let hash = content_hash(desktop);
    if hash == content_hash(bundled) {
        return Decision::Current;
    }
    if let (Some(theirs), Some(ours)) = (marker(desktop), marker(bundled)) {
        if version_key(theirs) > version_key(ours) {
            return Decision::Newer(theirs.to_string());
        }
    }
    match SHIPPED.iter().find(|(_, h)| *h == hash) {
        Some((version, _)) => Decision::Update(version.to_string()),
        None => Decision::Edited,
    }
}

/// Result of the start-up check, for `get_spawnable_engine_context`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkStatus {
    /// Version of the SDK bundled in this host.
    pub bundled: String,
    /// What the desktop copy was, and whether it was updated.
    pub desktop: Decision,
    pub updated: bool,
}

fn status_slot() -> &'static OnceLock<SdkStatus> {
    static SLOT: OnceLock<SdkStatus> = OnceLock::new();
    &SLOT
}

pub fn status() -> Option<SdkStatus> {
    status_slot().get().cloned()
}

pub fn bundled_version() -> &'static str {
    marker(BUNDLED).unwrap_or("unknown")
}

/// Replace `<root>/.sdk/versailles.js` with `bundled`, keeping the old file
/// as `versailles.js.bak`. Writes go through a temporary file in `.sdk/`.
fn replace(sdk_dir: &Path, bundled: &str) -> std::io::Result<()> {
    let file = sdk_dir.join("versailles.js");
    let tmp = sdk_dir.join("versailles.js.new");
    std::fs::write(&tmp, bundled)?;
    std::fs::copy(&file, sdk_dir.join("versailles.js.bak"))?;
    std::fs::rename(&tmp, &file)
}

/// Compare and, when safe, update the desktop's SDK under `root`
/// (`Documents\Widgets`). Runs once at start, before the wallpaper loads.
pub fn sync(root: &Path) -> SdkStatus {
    let sdk_dir: PathBuf = root.join(".sdk");
    let file = sdk_dir.join("versailles.js");
    let desktop = std::fs::read_to_string(&file).ok();
    let decision = decide(desktop.as_deref(), BUNDLED);
    let ours = bundled_version();
    let mut updated = false;
    match &decision {
        Decision::Missing => {
            tracing::info!("sdk: no {}; serving the bundled SDK {ours}", file.display());
        }
        Decision::Current => tracing::info!("sdk: desktop SDK is current ({ours})"),
        Decision::Update(old) => match replace(&sdk_dir, BUNDLED) {
            Ok(()) => {
                updated = true;
                tracing::info!(
                    "sdk: updated {} from {old} to {ours} (old copy in versailles.js.bak)",
                    file.display()
                );
            }
            Err(err) => tracing::warn!("sdk: could not update {} ({err})", file.display()),
        },
        Decision::Edited => tracing::warn!(
            "sdk: {} was edited by hand, so it is left as it is; new hooks of SDK {ours} may be missing \
             (copy templates/starter/.sdk/versailles.js over it to update)",
            file.display()
        ),
        Decision::Newer(theirs) => tracing::warn!(
            "sdk: {} is SDK {theirs}, newer than this Versailles ({ours}); left as it is",
            file.display()
        ),
    }
    let status = SdkStatus {
        bundled: ours.to_string(),
        desktop: decision,
        updated,
    };
    let _ = status_slot().set(status.clone());
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sdk_is_marked_and_listed() {
        let version = marker(BUNDLED).expect("first line carries `/*! versailles-sdk X.Y.Z */`");
        assert!(
            BUNDLED.contains(&format!("const SDK_VERSION = \"{version}\";")),
            "SDK_VERSION in versailles.js must match its marker {version}"
        );
        let hash = content_hash(BUNDLED);
        let (last, listed) = SHIPPED.last().unwrap();
        assert_eq!(
            (*last, *listed),
            (version, hash),
            "sdk::SHIPPED must end with (\"{version}\", {hash:#018x}). Unreleased: update the last line. \
             Released already: bump the SDK marker and add a line."
        );
        let mut seen = std::collections::HashSet::new();
        assert!(SHIPPED.iter().all(|(v, h)| seen.insert(*v) && *h != 0), "one line per version");
    }

    #[test]
    fn hash_ignores_line_endings_and_bom() {
        assert_eq!(content_hash("versailles"), 0x6a8d_d3b0_9984_6fe1);
        assert_eq!(content_hash("a\r\nb\r\n"), content_hash("a\nb\n"));
        assert_eq!(content_hash("\u{feff}a\nb"), content_hash("a\nb"));
        assert_ne!(content_hash("a\nb"), content_hash("a\nc"));
    }

    #[test]
    fn reads_the_marker() {
        assert_eq!(marker("/*! versailles-sdk 0.6.0 */\n(function(){})"), Some("0.6.0"));
        assert_eq!(marker("\u{feff}/*! versailles-sdk 1.2.3-dev */"), Some("1.2.3-dev"));
        assert_eq!(marker("/**\n * Versailles widget SDK"), None);
        assert!(version_key("0.10.0") > version_key("0.9.1"));
        assert!(version_key("0.6.0") > version_key("0.6.0-dev"));
    }

    #[test]
    fn decides_what_to_do_with_the_desktop_copy() {
        let bundled = "/*! versailles-sdk 0.6.0 */\nnew";
        assert_eq!(decide(None, bundled), Decision::Missing);
        assert_eq!(decide(Some(bundled), bundled), Decision::Current);
        assert_eq!(decide(Some(&bundled.replace('\n', "\r\n")), bundled), Decision::Current);
        assert_eq!(decide(Some("// my own tweaks"), bundled), Decision::Edited);
        assert_eq!(
            decide(Some("/*! versailles-sdk 0.9.0 */\nfuture"), bundled),
            Decision::Newer("0.9.0".into())
        );
        // An older marked copy that was edited is not a release.
        assert_eq!(decide(Some("/*! versailles-sdk 0.5.9 */\nedited"), bundled), Decision::Edited);
    }

    #[test]
    fn recognises_released_sdks() {
        // 0.5.0 as checked out on Windows (CRLF): an unmodified release.
        let v050 = std::process::Command::new("git")
            .args(["show", "v0.5.0:templates/starter/.sdk/versailles.js"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).replace('\n', "\r\n"));
        if let Some(old) = v050 {
            assert_eq!(decide(Some(&old), BUNDLED), Decision::Update("0.5.0".into()));
            let edited = old.replace("use strict", "use strict\"; // mine\n\"");
            assert_eq!(decide(Some(&edited), BUNDLED), Decision::Edited);
        }
    }

    #[test]
    fn updates_only_inside_sdk_and_keeps_a_backup() {
        let root = std::env::temp_dir().join(format!("versailles-sdk-test-{}", std::process::id()));
        let sdk = root.join(".sdk");
        std::fs::create_dir_all(&sdk).unwrap();
        std::fs::write(sdk.join("versailles.js"), "old release").unwrap();
        replace(&sdk, "new release").unwrap();
        assert_eq!(std::fs::read_to_string(sdk.join("versailles.js")).unwrap(), "new release");
        assert_eq!(std::fs::read_to_string(sdk.join("versailles.js.bak")).unwrap(), "old release");
        assert!(!sdk.join("versailles.js.new").exists());
        let mut names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec![".sdk".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
