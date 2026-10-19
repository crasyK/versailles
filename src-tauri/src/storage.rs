//! `storage` hook: a small JSON key-value store per piece under
//! `.versailles/storage/<id>.json`. A piece only ever reaches its own file.

use crate::error::{AppError, AppResult};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Serialized size cap per piece.
pub const MAX_BYTES: usize = 1024 * 1024;
const MAX_KEY: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum StorageOp {
    Get { key: String },
    Set { key: String, value: Value },
    Delete { key: String },
    Keys,
    Clear,
}

/// One lock for all stores: read-modify-write must not interleave.
fn io_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

/// File name for a piece id: lower-case, with anything outside
/// `[a-z0-9_-]` percent-encoded so distinct ids never share a file.
pub fn file_name(id: &str) -> String {
    let mut out = String::new();
    for b in id.trim().to_ascii_lowercase().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{out}.json")
}

fn read(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

fn check_key(key: &str) -> AppResult<()> {
    if key.is_empty() || key.len() > MAX_KEY {
        return Err(AppError::msg(format!(
            "storage: key must be 1–{MAX_KEY} bytes"
        )));
    }
    Ok(())
}

/// Run one operation on piece `id`'s store in `dir`. Returns the value for
/// `get` (null when missing), the key list for `keys`, and null otherwise.
pub fn apply(dir: &Path, id: &str, op: StorageOp) -> AppResult<Value> {
    let _guard = io_lock().lock().unwrap();
    let path: PathBuf = dir.join(file_name(id));
    let mut map = read(&path);
    let changed = match op {
        StorageOp::Get { key } => {
            check_key(&key)?;
            return Ok(map.get(&key).cloned().unwrap_or(Value::Null));
        }
        StorageOp::Keys => {
            return Ok(Value::Array(map.keys().cloned().map(Value::String).collect()));
        }
        StorageOp::Set { key, value } => {
            check_key(&key)?;
            map.insert(key, value);
            true
        }
        StorageOp::Delete { key } => {
            check_key(&key)?;
            map.remove(&key).is_some()
        }
        StorageOp::Clear => {
            let had = !map.is_empty();
            map.clear();
            had
        }
    };
    if !changed {
        return Ok(Value::Null);
    }
    if map.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(Value::Null);
    }
    let text = serde_json::to_string(&Value::Object(map))?;
    if text.len() > MAX_BYTES {
        return Err(AppError::msg(format!(
            "storage: '{id}' would exceed {} KB",
            MAX_BYTES / 1024
        )));
    }
    crate::config::atomic_write(&path, &text)?;
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("versailles-storage-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn op(v: Value) -> StorageOp {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn file_names_are_distinct_and_safe() {
        assert_eq!(file_name("Notes"), "notes.json");
        assert_eq!(file_name("a b"), "a%20b.json");
        assert_ne!(file_name("a b"), file_name("a_b"));
        assert_eq!(file_name("../x"), "%2E%2E%2Fx.json");
    }

    #[test]
    fn set_get_keys_delete_clear() {
        let dir = temp_dir();
        assert_eq!(apply(&dir, "todo", op(json!({"op":"get","key":"items"}))).unwrap(), Value::Null);
        apply(&dir, "todo", op(json!({"op":"set","key":"items","value":[1,2]}))).unwrap();
        apply(&dir, "todo", op(json!({"op":"set","key":"title","value":"x"}))).unwrap();
        assert_eq!(apply(&dir, "todo", op(json!({"op":"get","key":"items"}))).unwrap(), json!([1, 2]));
        assert_eq!(apply(&dir, "todo", op(json!({"op":"keys"}))).unwrap(), json!(["items", "title"]));
        apply(&dir, "todo", op(json!({"op":"delete","key":"items"}))).unwrap();
        assert_eq!(apply(&dir, "todo", op(json!({"op":"keys"}))).unwrap(), json!(["title"]));
        apply(&dir, "todo", op(json!({"op":"clear"}))).unwrap();
        assert!(!dir.join("todo.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pieces_do_not_share_a_store() {
        let dir = temp_dir();
        apply(&dir, "a", op(json!({"op":"set","key":"k","value":1}))).unwrap();
        assert_eq!(apply(&dir, "b", op(json!({"op":"get","key":"k"}))).unwrap(), Value::Null);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_bad_keys_and_oversize_stores() {
        let dir = temp_dir();
        assert!(apply(&dir, "a", op(json!({"op":"set","key":"","value":1}))).is_err());
        let big = "x".repeat(MAX_BYTES);
        assert!(apply(&dir, "a", op(json!({"op":"set","key":"k","value":big}))).is_err());
        assert!(!dir.join("a.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
