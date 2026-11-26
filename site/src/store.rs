//! The site's saved records, kept in a file in the data folder.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn path() -> PathBuf {
    let dir = std::env::var("DATA_DIR").ok().filter(|d| !d.trim().is_empty()).unwrap_or_else(|| {
        if std::path::Path::new("/data").is_dir() { "/data".into() } else { "data".into() }
    });
    PathBuf::from(dir).join("state.json")
}

fn cell() -> MutexGuard<'static, Value> {
    static S: OnceLock<Mutex<Value>> = OnceLock::new();
    S.get_or_init(|| {
        let loaded = if cfg!(test) { None } else { std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()) };
        Mutex::new(loaded.unwrap_or_else(|| json!({})))
    })
    .lock()
    .unwrap_or_else(|e| e.into_inner())
}

fn save(v: &Value) {
    if cfg!(test) {
        return;
    }
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, v.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// Reads the settings without changing them.
pub fn read<T>(f: impl FnOnce(&Value) -> T) -> T {
    f(&cell())
}

/// Changes the settings, then saves them.
pub fn write<T>(f: impl FnOnce(&mut Value) -> T) -> T {
    let mut g = cell();
    let out = f(&mut g);
    save(&g);
    out
}

/// A value at a path like "automod.invites", or Null.
pub fn get(path: &str) -> Value {
    read(|v| path.split('.').fold(v, |acc, k| &acc[k]).clone())
}

/// Sets a value at a path like "welcome.channel", creating the objects on the way.
pub fn set(path: &str, value: Value) {
    write(|v| {
        let mut cur = v;
        let keys: Vec<&str> = path.split('.').collect();
        for k in &keys[..keys.len() - 1] {
            if !cur[*k].is_object() {
                cur[*k] = json!({});
            }
            cur = &mut cur[*k];
        }
        cur[keys[keys.len() - 1]] = value;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_set_and_read_by_path() {
        set("t_welcome.channel", json!("123"));
        set("t_welcome.message", json!("hi"));
        assert_eq!(get("t_welcome.channel"), json!("123"));
        assert_eq!(get("t_welcome.message"), json!("hi"));
        assert_eq!(get("t_nothing.here"), Value::Null);
        write(|v| v["t_list"] = json!([1, 2]));
        assert_eq!(read(|v| v["t_list"].as_array().map(|a| a.len())), Some(2));
    }

    #[test]
    fn data_goes_in_a_data_folder_next_to_the_program_unless_a_volume_is_there() {
        let p = path();
        assert!(p.ends_with("state.json"));
        assert!(p.starts_with("data") || p.starts_with("/data") || std::env::var("DATA_DIR").is_ok());
    }
}
