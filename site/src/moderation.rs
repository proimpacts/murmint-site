//! Team-only moderation, alpha-tester access, announcements and the /bug and /feedback forms for the Murmint bot.
//!
//! Everything here is a Discord HTTP call made while answering a slash command, so there is no gateway connection.
//! The caller (bot.rs) has already checked that the person is on the Murmint Team before `run` is called.
//! Actions are written to a channel with "mod-log" in its name (or DISCORD_LOG_CHANNEL) when there is one.

use crate::bot::{api, call, card, env, option, owner, record_access, role_id, say, sep, text, ORANGE};
use reqwest::Method;
use serde_json::{json, Value};

const SEND_MESSAGES: u64 = 1 << 11;
const DAY: u64 = 86_400;

pub(crate) fn guild() -> String {
    env("DISCORD_GUILD_ID").unwrap_or_default()
}
pub(crate) fn actor(v: &Value) -> String {
    v["member"]["user"]["id"].as_str().unwrap_or("").to_string()
}
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
pub(crate) fn s_opt<'a>(v: &'a Value, name: &str) -> Option<&'a str> {
    option(v, name)?.as_str().map(str::trim).filter(|s| !s.is_empty())
}
pub(crate) fn i_opt(v: &Value, name: &str) -> Option<i64> {
    option(v, name)?.as_i64()
}

/// "2023-11-14T22:13:20.000Z" for a Unix time, which is what Discord wants for a timeout end.
pub fn iso(secs: u64) -> String {
    let (days, rem) = ((secs / DAY) as i64, secs % DAY);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// When a Discord ID (snowflake) was created, in Unix seconds.
pub fn made_at(id: &str) -> Option<u64> {
    Some((id.parse::<u64>().ok()? >> 22) / 1000 + 1_420_070_400)
}

fn reason_of(v: &Value) -> String {
    s_opt(v, "reason").unwrap_or("No reason given").chars().take(400).collect()
}

pub(crate) fn who(id: &str) -> String {
    format!("<@{id}>")
}

pub(crate) fn failed(code: u16) -> Value {
    say(match code {
        403 => "Discord says I'm not allowed to do that. My role may be below theirs, or it lacks that permission.",
        404 => "I couldn't find that. Is the person in the server, and is the ID right?",
        429 => "Discord is limiting me right now. Try again in a few seconds.",
        _ => "That didn't work. Try again, and check that the person is in the server.",
    })
}

/// The server's "mod-log" channel (or DISCORD_LOG_CHANNEL). Actions are only logged when one exists.
/// The answer is remembered for five minutes, because live events ask for it often.
pub(crate) fn log_channel() -> Option<String> {
    if let Some(c) = env("DISCORD_LOG_CHANNEL") {
        return Some(c);
    }
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, Option<String>)>> = std::sync::Mutex::new(None);
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = g.as_ref() {
        if at.elapsed() < std::time::Duration::from_secs(300) {
            return v.clone();
        }
    }
    let found = channel_named("mod-log");
    *g = Some((std::time::Instant::now(), found.clone()));
    found
}

/// A plain line in the mod-log, if there is one.
pub(crate) fn log_line(line: &str) {
    if let Some(channel) = log_channel() {
        post_card(&channel, vec![text(line)]);
    }
}

pub(crate) fn channel_named(part: &str) -> Option<String> {
    let chans = api(Method::GET, &format!("/guilds/{}/channels", guild()), None)?;
    chans
        .as_array()?
        .iter()
        .find(|c| c["type"].as_u64() == Some(0) && c["name"].as_str().is_some_and(|n| n.to_lowercase().contains(part)))?["id"]
        .as_str()
        .map(String::from)
}

/// Posts a ready-made message body and returns the new message's ID.
pub(crate) fn post_json(channel: &str, body: Value) -> Option<String> {
    call(Method::POST, &format!("/channels/{channel}/messages"), Some(body), None).ok()?["id"].as_str().map(String::from)
}

pub(crate) fn post_card(channel: &str, parts: Vec<Value>) -> bool {
    let body = json!({
        "flags": 1 << 15,
        "allowed_mentions": { "parse": [] },
        "components": [{ "type": 17, "accent_color": ORANGE, "components": parts }],
    });
    call(Method::POST, &format!("/channels/{channel}/messages"), Some(body), None).is_ok()
}

fn log(v: &Value, action: &str, target: &str, detail: &str) {
    let Some(channel) = log_channel() else { return };
    let line = format!("**{action}** {} by {}\n{detail}", who(target), who(&actor(v)));
    post_card(&channel, vec![text(&line)]);
}

/// A private message to the person, ignored if their DMs are closed.
pub(crate) fn dm(user: &str, message: &str) {
    let Ok(ch) = call(Method::POST, "/users/@me/channels", Some(json!({ "recipient_id": user })), None) else { return };
    if let Some(id) = ch["id"].as_str() {
        let _ = call(Method::POST, &format!("/channels/{id}/messages"), Some(json!({ "content": message })), None);
    }
}

pub(crate) fn member(user: &str) -> Result<Value, u16> {
    call(Method::GET, &format!("/guilds/{}/members/{user}", guild()), None, None)
}

/// Refuses the person who ran the command, the bot, the owner and anyone on the Murmint Team.
fn protected(v: &Value, target: &str) -> Option<Value> {
    if target == actor(v) {
        return Some(say("You can't do that to yourself."));
    }
    if target == env("DISCORD_CLIENT_ID").unwrap_or_default() {
        return Some(say("I'm not going to do that to myself."));
    }
    if target == owner() {
        return Some(say("That's the server owner."));
    }
    if let (Ok(m), Some(team)) = (member(target), role_id("Murmint Team")) {
        if m["roles"].as_array().is_some_and(|r| r.iter().any(|x| x.as_str() == Some(team.as_str()))) {
            return Some(say("That person is on the Murmint Team."));
        }
    }
    None
}

fn user_arg(v: &Value) -> Result<String, Value> {
    s_opt(v, "user").map(String::from).ok_or_else(|| say("Pick a user."))
}

pub fn run(name: &str, v: &Value) -> Value {
    let guild = guild();
    match name {
        "kick" | "ban" | "timeout" | "warn" => {
            let user = match user_arg(v) {
                Ok(u) => u,
                Err(e) => return e,
            };
            if let Some(refused) = protected(v, &user) {
                return refused;
            }
            let reason = reason_of(v);
            match name {
                "warn" => {
                    dm(&user, &format!("**A warning from the Murmint team**\n{reason}"));
                    log(v, "Warned", &user, &reason);
                    let count = crate::community::record_warning(&user, &actor(v), &reason);
                    // the third warning mutes for an hour, the fifth for a day
                    let minutes = match count { 3 => 60, n if n >= 5 => 1440, _ => 0 };
                    let mut note = String::new();
                    if minutes > 0 {
                        let body = json!({ "communication_disabled_until": iso(now() + minutes * 60) });
                        if call(Method::PATCH, &format!("/guilds/{guild}/members/{user}"), Some(body), Some("Warning limit reached")).is_ok() {
                            note = format!(" That's warning {count}, so they're muted for {}.", if minutes == 60 { "an hour" } else { "a day" });
                            log(v, "Muted for repeated warnings", &user, &format!("{count} warnings"));
                        }
                    }
                    say(&format!("Warned {}. They have {count} warning{}.{note} They got a private message with the reason.", who(&user), if count == 1 { "" } else { "s" }))
                }
                "kick" => {
                    dm(&user, &format!("You were removed from the Murmint server.\n**Reason:** {reason}"));
                    match call(Method::DELETE, &format!("/guilds/{guild}/members/{user}"), None, Some(&reason)) {
                        Ok(_) => {
                            log(v, "Kicked", &user, &reason);
                            say(&format!("Kicked {}.", who(&user)))
                        }
                        Err(c) => failed(c),
                    }
                }
                "ban" => {
                    let days = i_opt(v, "delete_days").unwrap_or(0).clamp(0, 7);
                    dm(&user, &format!("You were banned from the Murmint server.\n**Reason:** {reason}"));
                    let body = json!({ "delete_message_seconds": days * DAY as i64 });
                    match call(Method::PUT, &format!("/guilds/{guild}/bans/{user}"), Some(body), Some(&reason)) {
                        Ok(_) => {
                            log(v, "Banned", &user, &reason);
                            say(&format!("Banned {}.", who(&user)))
                        }
                        Err(c) => failed(c),
                    }
                }
                _ => {
                    let minutes = i_opt(v, "minutes").unwrap_or(10).clamp(1, 40_320) as u64;
                    let until = iso(now() + minutes * 60);
                    let body = json!({ "communication_disabled_until": until });
                    match call(Method::PATCH, &format!("/guilds/{guild}/members/{user}"), Some(body), Some(&reason)) {
                        Ok(_) => {
                            dm(&user, &format!("You were muted in the Murmint server for {minutes} minutes.\n**Reason:** {reason}"));
                            log(v, "Timed out", &user, &format!("{minutes} minutes. {reason}"));
                            say(&format!("Muted {} for {minutes} minutes.", who(&user)))
                        }
                        Err(c) => failed(c),
                    }
                }
            }
        }
        "untimeout" => {
            let user = match user_arg(v) {
                Ok(u) => u,
                Err(e) => return e,
            };
            let body = json!({ "communication_disabled_until": Value::Null });
            match call(Method::PATCH, &format!("/guilds/{guild}/members/{user}"), Some(body), Some("Timeout removed")) {
                Ok(_) => {
                    log(v, "Timeout removed for", &user, "");
                    say(&format!("{} can talk again.", who(&user)))
                }
                Err(c) => failed(c),
            }
        }
        "unban" => {
            let Some(id) = s_opt(v, "user_id").filter(|s| s.chars().all(|c| c.is_ascii_digit())) else {
                return say("Give the person's numeric Discord ID.");
            };
            match call(Method::DELETE, &format!("/guilds/{guild}/bans/{id}"), None, Some(&reason_of(v))) {
                Ok(_) => {
                    log(v, "Unbanned", id, &reason_of(v));
                    say(&format!("Unbanned {}.", who(id)))
                }
                Err(c) => failed(c),
            }
        }
        "purge" => purge(v),
        "slowmode" => {
            let seconds = i_opt(v, "seconds").unwrap_or(0).clamp(0, 21_600);
            let channel = v["channel_id"].as_str().unwrap_or("");
            match call(Method::PATCH, &format!("/channels/{channel}"), Some(json!({ "rate_limit_per_user": seconds })), None) {
                Ok(_) => {
                    let msg = if seconds == 0 { "Slowmode is off.".to_string() } else { format!("Slowmode is now {seconds} seconds.") };
                    say(&msg)
                }
                Err(c) => failed(c),
            }
        }
        "lock" | "unlock" => lock(v, name == "lock"),
        "announce" => announce(v),
        "whois" => whois(v),
        "alpha" => alpha(v),
        _ => say("I don't know that command yet."),
    }
}

/// Deletes the latest messages in this channel, optionally only one person's. Discord only allows messages under 14 days old.
fn purge(v: &Value) -> Value {
    let count = i_opt(v, "count").unwrap_or(10).clamp(1, 100) as usize;
    let channel = v["channel_id"].as_str().unwrap_or("");
    let only = s_opt(v, "user");
    let Ok(msgs) = call(Method::GET, &format!("/channels/{channel}/messages?limit=100"), None, None) else {
        return say("I couldn't read this channel.");
    };
    let fresh = now().saturating_sub(14 * DAY - 120);
    let ids: Vec<String> = msgs
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| only.is_none_or(|u| m["author"]["id"].as_str() == Some(u)))
        .filter_map(|m| m["id"].as_str())
        .filter(|id| made_at(id).is_some_and(|t| t > fresh))
        .take(count)
        .map(String::from)
        .collect();
    let result = match ids.len() {
        0 => return say("There's nothing recent enough to delete. Discord only lets me remove messages under 14 days old."),
        1 => call(Method::DELETE, &format!("/channels/{channel}/messages/{}", ids[0]), None, None),
        _ => call(Method::POST, &format!("/channels/{channel}/messages/bulk-delete"), Some(json!({ "messages": ids })), None),
    };
    match result {
        Ok(_) => {
            log(v, "Cleared messages", &actor(v), &format!("{} in <#{channel}>", ids.len()));
            say(&format!("Deleted {} message{}.", ids.len(), if ids.len() == 1 { "" } else { "s" }))
        }
        Err(c) => failed(c),
    }
}

/// Stops (or allows again) @everyone writing in this channel, leaving every other permission as it was.
fn lock(v: &Value, lock: bool) -> Value {
    let guild = guild();
    let channel = v["channel_id"].as_str().unwrap_or("");
    let Ok(info) = call(Method::GET, &format!("/channels/{channel}"), None, None) else { return say("I couldn't read this channel.") };
    let (mut allow, mut deny) = (0u64, 0u64);
    for o in info["permission_overwrites"].as_array().into_iter().flatten() {
        if o["id"].as_str() == Some(guild.as_str()) {
            allow = o["allow"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
            deny = o["deny"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }
    if lock {
        allow &= !SEND_MESSAGES;
        deny |= SEND_MESSAGES;
    } else {
        allow &= !SEND_MESSAGES;
        deny &= !SEND_MESSAGES;
    }
    let body = json!({ "type": 0, "allow": allow.to_string(), "deny": deny.to_string() });
    match call(Method::PUT, &format!("/channels/{channel}/permissions/{guild}"), Some(body), None) {
        Ok(_) => {
            log(v, if lock { "Locked" } else { "Unlocked" }, &actor(v), &format!("<#{channel}>"));
            say(if lock { "This channel is locked. Only people with their own permission to talk can write here." } else { "This channel is open again." })
        }
        Err(c) => failed(c),
    }
}

/// Posts a message as the bot, in the Murmint card style, to the chosen channel.
/// Posts an announcement card, optionally pinging one role.
pub(crate) fn send_announcement(channel: &str, title: Option<&str>, message: &str, ping: Option<&str>) -> bool {
    let mut parts = vec![];
    if let Some(p) = ping {
        parts.push(text(&format!("<@&{p}>")));
    }
    if let Some(title) = title {
        parts.push(text(&format!("## {}", title.chars().take(120).collect::<String>())));
        parts.push(sep());
    }
    parts.push(text(&message.replace("\\n", "\n").chars().take(3500).collect::<String>()));
    let body = json!({
        "flags": 1 << 15,
        "allowed_mentions": { "parse": [], "roles": ping.iter().collect::<Vec<_>>() },
        "components": [{ "type": 17, "accent_color": ORANGE, "components": parts }],
    });
    post_json(channel, body).is_some()
}

fn announce(v: &Value) -> Value {
    let (Some(channel), Some(message)) = (s_opt(v, "channel"), s_opt(v, "message")) else { return say("Pick a channel and write the message.") };
    let ping = s_opt(v, "ping");
    if ping == Some(guild().as_str()) {
        return say("I won't ping @everyone. Pick a specific role.");
    }
    if let Some(when) = s_opt(v, "in") {
        let Some(secs) = crate::features::parse_duration(when).filter(|s| (60..=30 * 86_400).contains(s)) else {
            return say("Give a delay between 1 minute and 30 days, like `30m`, `2h` or `1d`.");
        };
        let at = now() + secs;
        let entry = json!({ "channel": channel, "title": s_opt(v, "title"), "message": message, "ping": ping, "at": at });
        crate::store::write(|s| {
            if !s["scheduled"].is_array() {
                s["scheduled"] = json!([]);
            }
            s["scheduled"].as_array_mut().unwrap().push(entry);
        });
        log(v, "Scheduled an announcement in", &actor(v), &format!("<#{channel}> for <t:{at}:F>"));
        return say(&format!("Scheduled for <t:{at}:F> in <#{channel}>."));
    }
    if send_announcement(channel, s_opt(v, "title"), message, ping) {
        log(v, "Announced in", &actor(v), &format!("<#{channel}>"));
        say(&format!("Posted in <#{channel}>."))
    } else {
        say("I couldn't post there. Can I see and write in that channel?")
    }
}

fn whois(v: &Value) -> Value {
    let user = match user_arg(v) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let Ok(m) = member(&user) else { return say("That person isn't in the server.") };
    let alpha = role_id("Alpha Tester").is_some_and(|a| m["roles"].as_array().is_some_and(|r| r.iter().any(|x| x.as_str() == Some(a.as_str()))));
    let roles: Vec<String> = m["roles"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).map(|r| format!("<@&{r}>")).collect();
    let joined = m["joined_at"].as_str().and_then(|s| s.get(..10)).unwrap_or("unknown");
    let created = made_at(&user).map(|t| format!("<t:{t}:D>")).unwrap_or_else(|| "unknown".into());
    let muted = m["communication_disabled_until"].as_str().map(|t| format!("\n**Muted until:** {}", t.get(..16).unwrap_or(t))).unwrap_or_default();
    card(
        vec![text(&format!(
            "## {}\n**Alpha tester:** {}\n**Joined the server:** {joined}\n**Account created:** {created}\n**Roles:** {}{muted}",
            who(&user),
            if alpha { "yes" } else { "no" },
            if roles.is_empty() { "none".into() } else { roles.join(" ") }
        ))],
        false,
    )
}

/// /alpha list, check and add.
fn alpha(v: &Value) -> Value {
    let sub = v["data"]["options"][0].clone();
    let name = sub["name"].as_str().unwrap_or("");
    let inner = json!({ "data": { "options": sub["options"].clone() } });
    let guild = guild();
    let Some(role) = role_id("Alpha Tester") else { return say("I can't find the **Alpha Tester** role.") };
    match name {
        "list" => {
            let mut found: Vec<String> = vec![];
            let mut after = String::from("0");
            for _ in 0..10 {
                let Ok(page) = call(Method::GET, &format!("/guilds/{guild}/members?limit=1000&after={after}"), None, None) else {
                    return say("I couldn't read the member list. The bot needs the **Server Members Intent** switched on in the Discord Developer Portal.");
                };
                let list = page.as_array().cloned().unwrap_or_default();
                for m in &list {
                    if m["roles"].as_array().is_some_and(|r| r.iter().any(|x| x.as_str() == Some(role.as_str()))) {
                        if let Some(id) = m["user"]["id"].as_str() {
                            found.push(id.to_string());
                        }
                    }
                }
                match list.last().and_then(|m| m["user"]["id"].as_str()) {
                    Some(last) if list.len() == 1000 => after = last.to_string(),
                    _ => break,
                }
            }
            let shown: Vec<String> = found.iter().take(60).map(|id| who(id)).collect();
            let more = if found.len() > 60 { format!("\n…and {} more", found.len() - 60) } else { String::new() };
            card(vec![text(&format!("## Alpha testers: {}\n{}{more}", found.len(), if shown.is_empty() { "No one yet.".into() } else { shown.join(" ") }))], false)
        }
        "check" => {
            let user = match user_arg(&inner) {
                Ok(u) => u,
                Err(e) => return e,
            };
            match member(&user) {
                Ok(m) => {
                    let has = m["roles"].as_array().is_some_and(|r| r.iter().any(|x| x.as_str() == Some(role.as_str())));
                    say(&format!("{} {} an alpha tester.", who(&user), if has { "is" } else { "is not" }))
                }
                Err(_) => say("That person isn't in the server yet."),
            }
        }
        "add" => {
            let Some(id) = s_opt(&inner, "id").filter(|s| s.len() >= 15 && s.chars().all(|c| c.is_ascii_digit())) else {
                return say("Paste the person's numeric Discord ID (turn on Developer Mode, then right-click them and Copy User ID).");
            };
            record_access(id);
            let _ = call(Method::PUT, &format!("/guilds/{guild}/members/{id}/roles/{role}"), None, Some("Alpha access"));
            let inside = member(id).is_ok();
            say(&if inside {
                format!("{} is in the alpha.", who(id))
            } else {
                format!("Saved {}. They get the **Alpha Tester** role the moment they join the server, and website sign-in will let them download.", who(id))
            })
        }
        _ => say("Use `/alpha list`, `/alpha check` or `/alpha add`."),
    }
}

/// The welcome sent to a new alpha tester by /grant or the right-click menu.
pub fn welcome(user: &str) {
    crate::community::mark_alpha(user);
    dm(
        user,
        "**You're in the Murmint closed alpha.** The alpha channels are now open for you. Sign in with Discord on the website to download the installer, and use `/bug` or `/feedback` in the server to tell us what you find.",
    );
}

/// Opens the form for /bug or /feedback.
pub fn form(kind: &str) -> Value {
    let (title, label) = if kind == "bug" {
        ("Report a bug", "What happened, and what did you expect?")
    } else {
        ("Send feedback", "What's on your mind?")
    };
    let mut rows = vec![
        json!({ "type": 1, "components": [{ "type": 4, "custom_id": "title", "style": 1, "label": "In a few words", "min_length": 3, "max_length": 80, "required": true }] }),
        json!({ "type": 1, "components": [{ "type": 4, "custom_id": "details", "style": 2, "label": label, "min_length": 10, "max_length": 1500, "required": true }] }),
    ];
    if kind == "bug" {
        rows.push(json!({ "type": 1, "components": [{ "type": 4, "custom_id": "env", "style": 1, "label": "Murmint and Windows version (optional)", "max_length": 100, "required": false }] }));
        rows.push(json!({ "type": 1, "components": [{ "type": 4, "custom_id": "mission", "style": 1, "label": "Mission code, if this is from a bug hunt (optional)", "max_length": 20, "required": false }] }));
    }
    json!({ "type": 9, "data": { "custom_id": kind, "title": title, "components": rows } })
}

/// A short bug number from the time it was reported, like BUG-1k2x.
fn bug_id() -> String {
    let mut n = now().saturating_sub(1_767_225_600) / 60;
    let mut digits = vec![];
    while n > 0 || digits.is_empty() {
        digits.push(std::char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
    }
    format!("BUG-{}", digits.iter().rev().collect::<String>())
}

/// A submitted /bug or /feedback form: posted to the bugs or feedback channel.
pub fn submitted(v: &Value) -> Value {
    let kind = v["data"]["custom_id"].as_str().unwrap_or("feedback");
    let (mut title, mut details, mut env_note, mut mission_code) = (String::new(), String::new(), String::new(), String::new());
    for r in v["data"]["components"].as_array().into_iter().flatten() {
        for c in r["components"].as_array().into_iter().flatten() {
            match c["custom_id"].as_str() {
                Some("title") => title = c["value"].as_str().unwrap_or("").trim().to_string(),
                Some("details") => details = c["value"].as_str().unwrap_or("").trim().to_string(),
                Some("env") => env_note = c["value"].as_str().unwrap_or("").trim().to_string(),
                Some("mission") => mission_code = c["value"].as_str().unwrap_or("").trim().to_lowercase(),
                _ => {}
            }
        }
    }
    if title.is_empty() || details.is_empty() {
        return say("Both boxes need something in them.");
    }
    let (channel_part, heading) = if kind == "bug" { ("bugs", "Bug report") } else { ("feedback", "Feedback") };
    let Some(channel) = env(&format!("DISCORD_{}_CHANNEL", channel_part.to_uppercase())).or_else(|| channel_named(channel_part)) else {
        return say("I couldn't find the right channel to post in. Please post it in the server directly.");
    };
    let who_sent = who(&actor(v));
    if kind == "bug" {
        let me = actor(v);
        if !crate::community::can_report(&me) {
            return say("You've sent five bug reports in the last day, which is the limit. Please come back tomorrow. If something is urgent, open a ticket.");
        }
        let id = bug_id();
        let code = (!mission_code.is_empty()).then_some(mission_code.as_str());
        let mission_title = crate::community::note_bug(&id, &me, code);
        let mut parts = vec![text(&format!("## {id}: {title}")), text(&format!("**Status:** Open · From {who_sent}")), sep(), text(&details)];
        if !env_note.is_empty() {
            parts.push(text(&format!("-# {env_note}")));
        }
        if let Some(t) = &mission_title {
            parts.push(text(&format!("-# Bug hunt: {t}")));
        }
        parts.extend(crate::help::bug_buttons());
        let note = if code.is_some() && mission_title.is_none() { " (That mission code wasn't recognised or has ended, so it was filed without one.)" } else { "" };
        return if post_card(&channel, parts) {
            say(&format!("Thank you. It's posted as **{id}**.{note} The team will look at it, and you earn points once they confirm it as a bug. You'll get a message when that happens."))
        } else {
            say("I couldn't post that. Please try again in a moment.")
        };
    }
    let _ = heading;
    let id = format!("IDEA-{}", &bug_id()[4..]);
    let ok = post_card(&channel, vec![
        text(&format!("## {id}: {title}")),
        text(&format!("**Status:** New · From {who_sent}")),
        sep(),
        text(&details),
        crate::community::idea_vote_row(&id, 0, 0),
        crate::community::idea_status_row(),
    ]);
    if ok {
        say(&format!("Thank you. It's posted as **{id}**. Others can vote on it, and you'll hear if it's planned."))
    } else {
        say("I couldn't post that. Please try again in a moment.")
    }
}

/// Right-click a person, then Apps: grant or remove alpha access.
pub fn alpha_toggle(v: &Value, grant: bool) -> Value {
    let Some(user) = v["data"]["target_id"].as_str() else { return say("Pick a person.") };
    let Some(role) = role_id("Alpha Tester") else { return say("I can't find the **Alpha Tester** role.") };
    let path = format!("/guilds/{}/members/{user}/roles/{role}", guild());
    let result = if grant { call(Method::PUT, &path, None, Some("Alpha access")) } else { call(Method::DELETE, &path, None, Some("Alpha access removed")) };
    match result {
        Ok(_) => {
            if grant {
                record_access(user);
                welcome(user);
            } else {
                crate::community::unmark_alpha(user);
            }
            log(v, if grant { "Alpha access given to" } else { "Alpha access removed from" }, user, "");
            say(&format!("{} {} the closed alpha.", who(user), if grant { "is in" } else { "was removed from" }))
        }
        Err(c) => failed(c),
    }
}

/// The slash-command and menu definitions registered with Discord.
pub fn definitions() -> Vec<Value> {
    let user = |d: &str| json!({ "type": 6, "name": "user", "description": d, "required": true });
    let reason = json!({ "type": 3, "name": "reason", "description": "Why (they will see this)", "max_length": 400 });
    vec![
        json!({ "name": "kick", "description": "Remove someone from the server (team only)", "default_member_permissions": "2", "options": [user("Who to remove"), reason.clone()] }),
        json!({ "name": "ban", "description": "Ban someone (team only)", "default_member_permissions": "4",
               "options": [user("Who to ban"), reason.clone(), { "type": 4, "name": "delete_days", "description": "Also delete their messages from the last N days", "min_value": 0, "max_value": 7 }] }),
        json!({ "name": "unban", "description": "Lift a ban (team only)", "default_member_permissions": "4",
               "options": [{ "type": 3, "name": "user_id", "description": "Their numeric Discord ID", "required": true }, reason.clone()] }),
        json!({ "name": "timeout", "description": "Mute someone for a while (team only)", "default_member_permissions": "1099511627776",
               "options": [user("Who to mute"), { "type": 4, "name": "minutes", "description": "How long (default 10)", "min_value": 1, "max_value": 40320 }, reason.clone()] }),
        json!({ "name": "untimeout", "description": "Let a muted person talk again (team only)", "default_member_permissions": "1099511627776", "options": [user("Who to unmute")] }),
        json!({ "name": "warn", "description": "Send someone a private warning (team only)", "default_member_permissions": "1099511627776", "options": [user("Who to warn"), reason.clone()] }),
        json!({ "name": "purge", "description": "Delete recent messages in this channel (team only)", "default_member_permissions": "8192",
               "options": [{ "type": 4, "name": "count", "description": "How many (1 to 100)", "required": true, "min_value": 1, "max_value": 100 },
                           { "type": 6, "name": "user", "description": "Only this person's messages" }] }),
        json!({ "name": "slowmode", "description": "Set slowmode in this channel (team only)", "default_member_permissions": "16",
               "options": [{ "type": 4, "name": "seconds", "description": "Seconds between messages, 0 turns it off", "required": true, "min_value": 0, "max_value": 21600 }] }),
        json!({ "name": "lock", "description": "Stop people writing in this channel (team only)", "default_member_permissions": "16" }),
        json!({ "name": "unlock", "description": "Let people write in this channel again (team only)", "default_member_permissions": "16" }),
        json!({ "name": "whois", "description": "Who someone is: roles, join date, alpha status (team only)", "default_member_permissions": "1099511627776", "options": [user("Who to look up")] }),
        json!({ "name": "announce", "description": "Post a message as the bot in a channel (team only)", "default_member_permissions": "32",
               "options": [{ "type": 7, "name": "channel", "description": "Where to post", "required": true, "channel_types": [0, 5] },
                           { "type": 3, "name": "message", "description": "The message. Use \\n for a new line", "required": true, "max_length": 3500 },
                           { "type": 3, "name": "title", "description": "A heading", "max_length": 120 },
                           { "type": 8, "name": "ping", "description": "A role to ping, like Alpha Announcements" },
                           { "type": 3, "name": "in", "description": "Post later: like 30m, 2h or 1d" }] }),
        json!({ "name": "alpha", "description": "Manage the closed alpha (team only)", "default_member_permissions": "32", "options": [
            { "type": 1, "name": "list", "description": "Everyone who has alpha access" },
            { "type": 1, "name": "check", "description": "Is this person an alpha tester?", "options": [user("Who to check")] },
            { "type": 1, "name": "add", "description": "Add someone by Discord ID, even before they join", "options": [{ "type": 3, "name": "id", "description": "Their numeric Discord ID", "required": true }] },
        ] }),
        json!({ "name": "bug", "description": "Report a bug to the Murmint team" }),
        json!({ "name": "feedback", "description": "Send feedback to the Murmint team" }),
        json!({ "type": 2, "name": "Grant alpha", "default_member_permissions": "32" }),
        json!({ "type": 2, "name": "Revoke alpha", "default_member_permissions": "32" }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_the_way_discord_wants() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_700_000_000), "2023-11-14T22:13:20.000Z");
        assert_eq!(iso(1_709_164_800), "2024-02-29T00:00:00.000Z");
    }

    #[test]
    fn an_id_says_when_it_was_made() {
        assert_eq!(made_at("175928847299117063"), Some(1_462_015_105));
        assert_eq!(made_at("not an id"), None);
    }

    #[test]
    fn every_command_has_a_name_and_menus_have_no_description() {
        let d = definitions();
        assert!(d.len() >= 17);
        let mut names: Vec<&str> = d.iter().map(|c| c["name"].as_str().unwrap()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "no command is defined twice");
        for c in &d {
            if c["type"] == 2 {
                assert!(c.get("description").is_none());
            } else {
                assert!(c["description"].as_str().is_some_and(|s| !s.is_empty() && s.len() <= 100), "{}", c["name"]);
            }
        }
    }

    #[test]
    fn the_forms_ask_for_a_title_and_details() {
        for kind in ["bug", "feedback"] {
            let f = form(kind);
            assert_eq!(f["type"], 9);
            assert_eq!(f["data"]["custom_id"], kind);
            assert_eq!(f["data"]["components"].as_array().unwrap().len(), if kind == "bug" { 4 } else { 2 });
        }
    }

    #[test]
    fn bad_input_is_refused_before_anything_is_sent() {
        let ask = |name: &str, opts: Value| run(name, &json!({ "data": { "options": opts }, "member": { "user": { "id": "1" } }, "channel_id": "2" }));
        let said = |r: Value| r["data"]["components"][0]["components"][0]["content"].as_str().unwrap_or("").to_string();
        assert!(said(ask("unban", json!([{ "name": "user_id", "value": "abc" }]))).contains("numeric"));
        assert!(said(ask("kick", json!([]))).contains("Pick a user"));
        assert!(said(ask("kick", json!([{ "name": "user", "value": "1" }]))).contains("yourself"));
        assert!(said(ask("announce", json!([]))).contains("Pick a channel"));
        assert!(said(submitted(&json!({ "data": { "custom_id": "bug", "components": [] } }))).contains("Both boxes"));
    }

    // Discord rejects the whole registration if one definition breaks its rules, so check them here.
    fn check_options(opts: &Value, at: &str) {
        let Some(list) = opts.as_array() else { return };
        let mut seen_optional = false;
        for o in list {
            let name = o["name"].as_str().unwrap_or("");
            let sub = matches!(o["type"].as_u64(), Some(1) | Some(2));
            assert!(!name.is_empty() && name.len() <= 32 && name == name.to_lowercase() && !name.contains(' '), "{at}: option name {name}");
            assert!(o["description"].as_str().is_some_and(|d| !d.is_empty() && d.chars().count() <= 100), "{at}.{name}: description");
            if !sub {
                let required = o["required"].as_bool().unwrap_or(false);
                assert!(!(required && seen_optional), "{at}.{name}: a required option comes after an optional one");
                seen_optional |= !required;
            }
            check_options(&o["options"], &format!("{at}.{name}"));
        }
        assert!(list.len() <= 25, "{at}: too many options");
    }

    #[test]
    fn the_definitions_follow_discords_rules() {
        let mut all = crate::bot::commands_for_test();
        all.extend(definitions());
        assert!(all.len() <= 100);
        for c in &all {
            let name = c["name"].as_str().unwrap();
            assert!(name.len() <= 32, "{name}");
            if c["type"] != 2 {
                assert!(name == name.to_lowercase() && !name.contains(' '), "{name}");
            }
            check_options(&c["options"], name);
        }
    }

    #[test]
    fn people_who_are_not_on_the_team_are_refused() {
        for name in ["kick", "ban", "unban", "timeout", "untimeout", "warn", "purge", "slowmode", "lock", "unlock", "whois", "announce", "alpha", "grant", "revoke", "Grant alpha", "Revoke alpha"] {
            let r = crate::bot::respond(&json!({ "type": 2, "data": { "name": name, "options": [] }, "member": { "roles": [], "user": { "id": "42" } } }));
            let said = r["data"]["components"][0]["components"][0]["content"].as_str().unwrap_or("").to_string();
            assert!(said.starts_with("Only the Murmint Team"), "{name}: {said}");
        }
        let r = crate::bot::respond(&json!({ "type": 2, "data": { "name": "bug" }, "member": { "roles": [], "user": { "id": "42" } } }));
        assert_eq!(r["type"], 9, "anyone can open the bug form");
    }
}
