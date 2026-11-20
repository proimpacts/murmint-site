//! The bot's saved settings and records: welcome message, auto-mod, polls, giveaways, reminders, tags, warnings, tester
//! points, bug-hunt missions and more.
//!
//! They live in one file, DATA_DIR/state.json. DATA_DIR is the folder in the DATA_DIR variable if it is set, otherwise
//! /data if that folder exists (where a persistent volume is mounted), otherwise "data". Many hosts wipe the server's own disk on
//! every deploy, so to keep these across updates, mount a volume at /data.

use crate::bot::{call, env};
use reqwest::Method;
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

/// Sends a text file to a channel (used for ticket transcripts). Returns the new message's ID.
pub fn upload(channel: &str, filename: &str, bytes: Vec<u8>, note: &str) -> Option<String> {
    use reqwest::blocking::multipart::{Form, Part};
    let auth = format!("Bot {}", env("DISCORD_BOT_TOKEN")?);
    let payload = json!({ "content": note, "allowed_mentions": { "parse": [] }, "attachments": [{ "id": 0, "filename": filename }] });
    let form = Form::new()
        .text("payload_json", payload.to_string())
        .part("files[0]", Part::bytes(bytes).file_name(filename.to_string()).mime_str("text/plain").ok()?);
    let r = crate::auth::http()
        .post(format!("https://discord.com/api/v10/channels/{channel}/messages"))
        .header("Authorization", auth)
        .multipart(form)
        .send()
        .ok()?;
    if !r.status().is_success() {
        eprintln!("[store] upload failed: {}", r.status());
        return None;
    }
    r.json::<Value>().ok()?["id"].as_str().map(String::from)
}

/// Posts a message that has files attached. `payload` is the message as Discord wants it, with an "attachments" list that
/// names each file; `files` is (name, bytes, content type) in the same order. Returns the new message's ID.
pub fn post_files(channel: &str, payload: Value, files: Vec<(String, Vec<u8>, String)>) -> Option<String> {
    use reqwest::blocking::multipart::{Form, Part};
    let auth = format!("Bot {}", env("DISCORD_BOT_TOKEN")?);
    let mut form = Form::new().text("payload_json", payload.to_string());
    for (i, (name, bytes, mime)) in files.into_iter().enumerate() {
        form = form.part(format!("files[{i}]"), Part::bytes(bytes).file_name(name).mime_str(&mime).ok()?);
    }
    let r = crate::auth::http().post(format!("https://discord.com/api/v10/channels/{channel}/messages")).header("Authorization", auth).multipart(form).send().ok()?;
    if !r.status().is_success() {
        eprintln!("[store] posting files failed: {}", r.status());
        return None;
    }
    r.json::<Value>().ok()?["id"].as_str().map(String::from)
}

/// The bot used to keep a copy of its data in a hidden "bot-data" channel. That is gone: the data is only in the file above
/// now. This removes the old channel (only that exact one, recognised by its name and its topic) so it doesn't linger.
pub fn remove_old_data_channel() {
    let (Some(_), Some(guild)) = (env("DISCORD_BOT_TOKEN"), env("DISCORD_GUILD_ID")) else { return };
    let Ok(chans) = call(Method::GET, &format!("/guilds/{guild}/channels"), None, None) else { return };
    for c in chans.as_array().into_iter().flatten() {
        let ours = c["name"].as_str() == Some("bot-data") && c["topic"].as_str().is_some_and(|t| t.starts_with("The Murmint bot keeps its saved data"));
        if let (true, Some(id)) = (ours, c["id"].as_str()) {
            if call(Method::DELETE, &format!("/channels/{id}"), None, Some("The bot no longer stores its data in Discord")).is_ok() {
                println!("[store] removed the old bot-data channel");
            }
        }
    }
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
    fn nothing_is_sent_to_discord_in_tests() {
        remove_old_data_channel();
        assert!(upload("1", "a.txt", vec![], "").is_none(), "no token, so nothing is uploaded");
    }

    #[test]
    fn data_goes_in_a_data_folder_next_to_the_program_unless_a_volume_is_there() {
        let p = path();
        assert!(p.ends_with("state.json"));
        assert!(p.starts_with("data") || p.starts_with("/data") || std::env::var("DATA_DIR").is_ok());
    }
}
