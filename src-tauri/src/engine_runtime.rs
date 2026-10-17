//! Per-spawnable engine runtime (recents, pins) stored under `.versailles/engines.json`.

use crate::error::AppResult;
use crate::state::AppState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineRuntimeState {
    #[serde(default)]
    pub recents: Vec<String>,
    #[serde(default)]
    pub pins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_term_seed: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EngineRuntimeFile {
    #[serde(default)]
    engines: HashMap<String, EngineRuntimeState>,
}

fn engines_path(state: &AppState) -> PathBuf {
    state.store.lock().unwrap().engines_runtime_path()
}

fn load_file(state: &AppState) -> AppResult<EngineRuntimeFile> {
    load_path(&engines_path(state))
}

/// Missing / empty → defaults. Corrupt → back up as `engines.json.corrupt-<ts>`
/// and start fresh (recents/pins are convenience data; never fail the bar).
fn load_path(path: &Path) -> AppResult<EngineRuntimeFile> {
    if !path.exists() {
        return Ok(EngineRuntimeFile::default());
    }
    let raw = String::from_utf8_lossy(&fs::read(path)?).into_owned();
    if raw.trim().is_empty() {
        return Ok(EngineRuntimeFile::default());
    }
    match serde_json::from_str(&raw) {
        Ok(file) => Ok(file),
        Err(err) => {
            let backup = crate::config::backup_corrupt(path);
            tracing::warn!(
                "invalid engines.json ({err}); backed up to {:?} and reset",
                backup
            );
            Ok(EngineRuntimeFile::default())
        }
    }
}

fn save_file(state: &AppState, file: &EngineRuntimeFile) -> AppResult<()> {
    let json = serde_json::to_string_pretty(file)?;
    crate::config::atomic_write(&engines_path(state), &json)
}

fn norm_id(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineRuntimePatch {
    #[serde(default)]
    pub push_recent: Option<String>,
    #[serde(default)]
    pub toggle_pin: Option<String>,
    #[serde(default)]
    pub pins: Option<Vec<String>>,
    #[serde(default)]
    pub last_term_seed: Option<String>,
}

#[tauri::command]
pub fn get_engine_runtime(
    engine_id: String,
    state: State<'_, AppState>,
) -> Result<EngineRuntimeState, String> {
    let id = norm_id(&engine_id);
    let file = load_file(&state).map_err(|e| e.to_string())?;
    Ok(file
        .engines
        .get(&id)
        .cloned()
        .unwrap_or_default())
}

#[tauri::command]
pub fn patch_engine_runtime(
    engine_id: String,
    patch: EngineRuntimePatch,
    state: State<'_, AppState>,
) -> Result<EngineRuntimeState, String> {
    let id = norm_id(&engine_id);
    let mut file = load_file(&state).map_err(|e| e.to_string())?;
    let entry = file.engines.entry(id).or_default();

    if let Some(recent) = patch.push_recent {
        let key = recent.trim().to_ascii_lowercase();
        if !key.is_empty() {
            entry.recents.retain(|r| r != &key);
            entry.recents.insert(0, key);
            entry.recents.truncate(24);
        }
    }
    if let Some(pin) = patch.toggle_pin {
        let key = pin.trim().to_ascii_lowercase();
        if !key.is_empty() {
            if let Some(idx) = entry.pins.iter().position(|p| p == &key) {
                entry.pins.remove(idx);
            } else {
                entry.pins.push(key);
            }
        }
    }
    if let Some(pins) = patch.pins {
        entry.pins = pins
            .into_iter()
            .map(|p| p.trim().to_ascii_lowercase())
            .filter(|p| !p.is_empty())
            .collect();
    }
    if patch.last_term_seed.is_some() {
        entry.last_term_seed = patch.last_term_seed;
    }

    let out = entry.clone();
    save_file(&state, &file).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_engines_json_self_heals() {
        let dir = std::env::temp_dir().join(format!("versailles-eng-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("engines.json");
        fs::write(&path, "{ \"engines\": { \"action-bar\": { \"recents\": [").unwrap();
        let file = load_path(&path).unwrap();
        assert!(file.engines.is_empty());
        let backups = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("engines.json.corrupt-"))
            .count();
        assert_eq!(backups, 1);
        let _ = fs::remove_dir_all(dir);
    }
}
