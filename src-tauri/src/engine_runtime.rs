//! Per-spawnable engine runtime (recents, pins) stored under `.versailles/engines.json`.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
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
    let path = engines_path(state);
    if !path.exists() {
        return Ok(EngineRuntimeFile::default());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(EngineRuntimeFile::default());
    }
    serde_json::from_str(&raw).map_err(|e| AppError::msg(format!("invalid engines.json: {e}")))
}

fn save_file(state: &AppState, file: &EngineRuntimeFile) -> AppResult<()> {
    let path = engines_path(state);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(file)?;
    fs::write(path, json)?;
    Ok(())
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
