//! The bot's live connection to Discord, for things that happen without a command: welcoming new members, giving
//! them a starting role, auto-mod, and a log of deleted and edited messages, joins, leaves and bans.
//!
//! It keeps a WebSocket open to Discord's gateway on its own thread and reconnects if it drops. Settings come from
//! store.rs and are changed with /welcome, /autorole and /automod. The bot needs the Server Members and Message Content
//! intents switched on in the Discord Developer Portal (they are on for Murmint).

use crate::bot::{call, env, link, owner, role_id, row, sep, site, text, ORANGE};
use crate::features::fill_welcome;
use crate::moderation::{iso, log_channel, log_line, made_at, now, post_json};
use crate::store;
use reqwest::Method;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

// guilds, members, moderation (bans), messages, message content
const INTENTS: u64 = 1 | (1 << 1) | (1 << 2) | (1 << 9) | (1 << 15);
const URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const OUR_INVITE: &str = "mefdzfyarv";
const ALLOWED_SITES: &[&str] = &[
    "murmint.app", "github.com", "tally.so", "discord.com", "discordapp.com", "discordapp.net", "tenor.com", "giphy.com", "youtube.com", "youtu.be", "imgur.com",
];

static MEMBERS: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Memory {
    messages: HashMap<String, (String, String, String)>, // id -> (author, channel, content)
    order: VecDeque<String>,
    channels: HashMap<String, String>, // id -> name
    removed_by_us: HashSet<String>,
    recent: HashMap<String, Vec<Instant>>, // author -> times of recent messages
    team: Option<(Instant, Option<String>)>,
    alpha: Option<(Instant, Option<String>)>,
}
static MEM: Mutex<Option<Memory>> = Mutex::new(None);

fn mem<T>(f: impl FnOnce(&mut Memory) -> T) -> T {
    let mut g = MEM.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Memory::default))
}

fn guild() -> String {
    env("DISCORD_GUILD_ID").unwrap_or_default()
}

fn team_role() -> Option<String> {
    if let Some(v) = mem(|m| m.team.as_ref().filter(|(t, _)| t.elapsed() < Duration::from_secs(600)).map(|(_, v)| v.clone())) {
        return v;
    }
    let found = role_id("Murmint Team");
    mem(|m| m.team = Some((Instant::now(), found.clone())));
    found
}

fn alpha_role() -> Option<String> {
    if let Some(v) = mem(|m| m.alpha.as_ref().filter(|(t, _)| t.elapsed() < Duration::from_secs(600)).map(|(_, v)| v.clone())) {
        return v;
    }
    let found = role_id("Alpha Tester");
    mem(|m| m.alpha = Some((Instant::now(), found.clone())));
    found
}

/// Someone's roles changed: welcome a new alpha tester, however they got the role.
fn on_member_update(d: &Value) {
    if d["guild_id"].as_str() != Some(&guild()) || d["user"]["bot"] == json!(true) {
        return;
    }
    let (Some(user), Some(role)) = (d["user"]["id"].as_str(), alpha_role()) else { return };
    let has = d["roles"].as_array().is_some_and(|r| r.iter().any(|x| x.as_str() == Some(role.as_str())));
    if has {
        if crate::community::mark_alpha(user) {
            crate::moderation::welcome(user);
        }
    } else {
        crate::community::unmark_alpha(user);
    }
}

fn on(rule: &str) -> bool {
    store::get(&format!("automod.{rule}")) == json!(true)
}

/// The hosts of every link in a message: "see https://Example.com/a" gives ["example.com"].
pub fn link_hosts(content: &str) -> Vec<String> {
    let lower = content.to_lowercase();
    let mut out = vec![];
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("http") {
        let tail = &rest[i..];
        let after = tail.strip_prefix("https://").or_else(|| tail.strip_prefix("http://"));
        if let Some(a) = after {
            let host: String = a.chars().take_while(|c| !c.is_whitespace() && *c != '/' && *c != '?' && *c != '#' && *c != '>').collect();
            let host = host.rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("").to_string();
            if !host.is_empty() {
                out.push(host);
            }
        }
        rest = &tail[4..];
    }
    out
}

pub fn has_foreign_invite(content: &str) -> bool {
    let l = content.to_lowercase();
    ["discord.gg/", "discord.com/invite/", "discordapp.com/invite/", "discord.gg\\"].iter().any(|k| l.contains(k)) && !l.contains(OUR_INVITE)
}

pub fn has_banned_word(content: &str, words: &[String]) -> bool {
    let l = content.to_lowercase();
    let tokens: Vec<&str> = l.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    words.iter().any(|w| if w.contains(' ') { l.contains(w.as_str()) } else { tokens.contains(&w.as_str()) })
}

fn site_allowed(host: &str) -> bool {
    ALLOWED_SITES.iter().any(|a| host == *a || host.ends_with(&format!(".{a}")))
}

pub(crate) fn notice(channel: &str, line: &str) {
    let body = json!({ "flags": 1 << 15, "allowed_mentions": { "parse": [] }, "components": [{ "type": 17, "accent_color": ORANGE, "components": [text(line)] }] });
    if let Some(id) = post_json(channel, body) {
        let channel = channel.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(8));
            let _ = call(Method::DELETE, &format!("/channels/{channel}/messages/{id}"), None, None);
        });
    }
}

fn timeout_member(user: &str, minutes: u64, reason: &str) {
    let until = iso(now() + minutes * 60);
    let _ = call(Method::PATCH, &format!("/guilds/{}/members/{user}", guild()), Some(json!({ "communication_disabled_until": until })), Some(reason));
}

// ---------- events ----------
fn on_message(d: &Value) {
    if d["guild_id"].as_str() != Some(&guild()) || d["author"]["bot"] == json!(true) {
        return;
    }
    let (id, channel, author) = (d["id"].as_str().unwrap_or(""), d["channel_id"].as_str().unwrap_or(""), d["author"]["id"].as_str().unwrap_or(""));
    let content = d["content"].as_str().unwrap_or("");
    mem(|m| {
        m.messages.insert(id.to_string(), (author.to_string(), channel.to_string(), content.to_string()));
        m.order.push_back(id.to_string());
        while m.order.len() > 3000 {
            if let Some(old) = m.order.pop_front() {
                m.messages.remove(&old);
            }
        }
    });
    let roles: Vec<&str> = d["member"]["roles"].as_array().map(|a| a.iter().filter_map(|r| r.as_str()).collect()).unwrap_or_default();
    let exempt = author == owner() || team_role().is_some_and(|t| roles.contains(&t.as_str()));
    if !exempt && automod(d, id, channel, author, content) {
        return;
    }
    crate::community::on_message(d, exempt, private_channel(channel));
}

/// Checks a message against the auto-mod rules. True if it was removed.
fn automod(d: &Value, id: &str, channel: &str, author: &str, content: &str) -> bool {
    let words: Vec<String> = store::get("automod.words").as_array().map(|a| a.iter().filter_map(|w| w.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut why: Option<(&str, u64)> = None; // reason, minutes muted (0 = none)
    if on("invites") && has_foreign_invite(content) {
        why = Some(("invite links to other servers aren't allowed here", 0));
    } else if on("links") && link_hosts(content).iter().any(|h| !site_allowed(h)) {
        why = Some(("that link isn't allowed here", 0));
    } else if !words.is_empty() && has_banned_word(content, &words) {
        why = Some(("please watch your language", 0));
    } else if on("mentions") && d["mentions"].as_array().map(|a| a.len()).unwrap_or(0) >= 5 {
        why = Some(("please don't mass-mention people", 10));
    }
    if let Some((reason, minutes)) = why {
        mem(|m| m.removed_by_us.insert(id.to_string()));
        let _ = call(Method::DELETE, &format!("/channels/{channel}/messages/{id}"), None, Some("Auto-mod"));
        notice(channel, &format!("<@{author}>, {reason}."));
        if minutes > 0 {
            timeout_member(author, minutes, "Auto-mod: mass mentions");
        }
        let shown: String = content.chars().take(300).collect();
        log_line(&format!("**Auto-mod** removed a message from <@{author}> in <#{channel}>: {reason}\n> {shown}"));
        return true;
    }
    if on("spam") {
        let burst = mem(|m| {
            let list = m.recent.entry(author.to_string()).or_default();
            list.push(Instant::now());
            list.retain(|t| t.elapsed() < Duration::from_secs(6));
            let burst = list.len() >= 6;
            if burst {
                list.clear();
            }
            burst
        });
        if burst {
            timeout_member(author, 5, "Auto-mod: spam");
            notice(channel, &format!("<@{author}> is muted for 5 minutes for sending messages too fast."));
            log_line(&format!("**Auto-mod** muted <@{author}> for 5 minutes in <#{channel}>: spam"));
        }
    }
    false
}

pub(crate) fn private_channel(id: &str) -> bool {
    mem(|m| m.channels.get(id).is_some_and(|n| n.contains("staff") || n.starts_with("ticket-") || n.contains("mod-log")))
}

fn on_edit(d: &Value) {
    let (Some(id), Some(new)) = (d["id"].as_str(), d["content"].as_str()) else { return };
    let channel = d["channel_id"].as_str().unwrap_or("");
    if d["guild_id"].as_str() != Some(&guild()) || d["author"]["bot"] == json!(true) {
        return;
    }
    let old = mem(|m| {
        let old = m.messages.get(id).cloned();
        if let Some(entry) = m.messages.get_mut(id) {
            entry.2 = new.to_string();
        }
        old
    });
    if let Some((author, _, before)) = old {
        if before != new && !private_channel(channel) {
            let (b, a): (String, String) = (before.chars().take(400).collect(), new.chars().take(400).collect());
            log_line(&format!("**Message edited** by <@{author}> in <#{channel}>\n**Before:** {b}\n**After:** {a}"));
        }
    }
}

fn on_delete(d: &Value) {
    let (Some(id), channel) = (d["id"].as_str(), d["channel_id"].as_str().unwrap_or("")) else { return };
    if d["guild_id"].as_str() != Some(&guild()) {
        return;
    }
    let (entry, by_us) = mem(|m| (m.messages.remove(id), m.removed_by_us.remove(id)));
    if by_us || private_channel(channel) {
        return;
    }
    match entry {
        Some((author, _, content)) if !content.is_empty() => {
            let shown: String = content.chars().take(500).collect();
            log_line(&format!("**Message deleted** from <@{author}> in <#{channel}>\n{shown}"));
        }
        _ => {}
    }
}

/// A channel of the server whose name ends with `name`, like "guide" for "📖┃guide".
fn channel_named(name: &str) -> Option<String> {
    let list = call(Method::GET, &format!("/guilds/{}/channels", guild()), None, None).ok()?;
    list.as_array()?.iter().find(|c| c["name"].as_str().is_some_and(|n| n.ends_with(name))).and_then(|c| c["id"].as_str().map(String::from))
}

/// Where new members are greeted: the channel set with /welcome, otherwise the "new-arrivals" channel.
fn welcome_channel() -> Option<String> {
    store::get("welcome.channel").as_str().map(String::from).or_else(|| channel_named("new-arrivals"))
}

fn on_join(d: &Value) {
    if d["guild_id"].as_str() != Some(&guild()) {
        return;
    }
    let Some(user) = d["user"]["id"].as_str() else { return };
    let count = MEMBERS.fetch_add(1, Ordering::Relaxed) + 1;
    if d["user"]["bot"] == json!(true) {
        return;
    }
    if let Some(role) = store::get("autorole").as_str() {
        let _ = call(Method::PUT, &format!("/guilds/{}/members/{user}/roles/{role}", guild()), None, Some("Auto-role"));
    }
    if let Some(channel) = welcome_channel() {
        let guide = channel_named("guide");
        let line = match store::get("welcome.message").as_str() {
            Some(template) => fill_welcome(template, user, count),
            None => {
                let start = guide.as_ref().map(|g| format!(" Start in <#{g}>.")).unwrap_or_default();
                format!("## Welcome, <@{user}>!
You are member **{count}** of Murmint.{start}")
            }
        };
        let mut buttons = vec![link("Download", &format!("{}/download", site()))];
        if let Some(g) = &guide {
            buttons.insert(0, link("Read the guide", &format!("https://discord.com/channels/{}/{g}", guild())));
        }
        let banner = json!({ "type": 12, "items": [{ "media": { "url": format!("{}/assets/welcome-banner.png", site()) }, "description": "Welcome to Murmint" }] });
        let body = json!({
            "flags": 1 << 15,
            "allowed_mentions": { "parse": [], "users": [user] },
            "components": [{ "type": 17, "accent_color": ORANGE, "components": [banner, text(&line), sep(), row(buttons)] }]
        });
        post_json(&channel, body);
    }
    let created = made_at(user).map(|t| format!("<t:{t}:R>")).unwrap_or_default();
    log_line(&format!("**Joined** <@{user}> (account created {created})"));
}

fn on_leave(d: &Value) {
    if d["guild_id"].as_str() != Some(&guild()) {
        return;
    }
    MEMBERS.store(MEMBERS.load(Ordering::Relaxed).saturating_sub(1), Ordering::Relaxed);
    if let Some(user) = d["user"]["id"].as_str() {
        if d["user"]["bot"] != json!(true) {
            log_line(&format!("**Left** <@{user}>"));
        }
    }
}

fn dispatch(kind: &str, d: &Value) {
    match kind {
        "GUILD_CREATE" if d["id"].as_str() == Some(&guild()) => {
            MEMBERS.store(d["member_count"].as_u64().unwrap_or(0), Ordering::Relaxed);
            mem(|m| {
                for c in d["channels"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(name)) = (c["id"].as_str(), c["name"].as_str()) {
                        m.channels.insert(id.into(), name.into());
                    }
                }
            });
            println!("[gateway] ready in the server");
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
            if let (Some(id), Some(name)) = (d["id"].as_str(), d["name"].as_str()) {
                mem(|m| m.channels.insert(id.into(), name.into()));
            }
        }
        "MESSAGE_CREATE" => on_message(d),
        "MESSAGE_UPDATE" => on_edit(d),
        "MESSAGE_DELETE" => on_delete(d),
        "GUILD_MEMBER_ADD" => on_join(d),
        "GUILD_MEMBER_REMOVE" => on_leave(d),
        "GUILD_MEMBER_UPDATE" => on_member_update(d),
        "GUILD_BAN_ADD" if d["guild_id"].as_str() == Some(&guild()) => {
            if let Some(user) = d["user"]["id"].as_str() {
                log_line(&format!("**Banned** <@{user}>"));
            }
        }
        _ => {}
    }
}

// ---------- the connection ----------
fn set_timeout(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    let t = Some(Duration::from_millis(1000));
    match ws.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(t);
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.sock.set_read_timeout(t);
        }
        _ => {}
    }
}

fn session(token: &str) -> Result<(), String> {
    let (mut ws, _) = tungstenite::connect(URL).map_err(|e| format!("connect: {e}"))?;
    set_timeout(&mut ws);
    let (mut interval, mut seq, mut next_beat, mut acked) = (Duration::from_secs(40), Value::Null, Instant::now() + Duration::from_secs(30), true);
    let mut identified = false;
    loop {
        if identified && Instant::now() >= next_beat {
            if !acked {
                return Err("no heartbeat reply".into());
            }
            ws.send(Message::Text(json!({ "op": 1, "d": seq }).to_string())).map_err(|e| e.to_string())?;
            acked = false;
            next_beat = Instant::now() + interval;
        }
        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e.to_string()),
        };
        let Message::Text(body) = msg else {
            if matches!(msg, Message::Close(_)) {
                return Err("closed by Discord".into());
            }
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&body) else { continue };
        if !v["s"].is_null() {
            seq = v["s"].clone();
        }
        match v["op"].as_u64() {
            Some(10) => {
                interval = Duration::from_millis(v["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250));
                next_beat = Instant::now() + interval.mul_f64(0.5);
                let id = json!({ "op": 2, "d": { "token": token, "intents": INTENTS,
                    "properties": { "os": "linux", "browser": "murmint", "device": "murmint" } } });
                ws.send(Message::Text(id.to_string())).map_err(|e| e.to_string())?;
                identified = true;
            }
            Some(11) => acked = true,
            Some(1) => {
                ws.send(Message::Text(json!({ "op": 1, "d": seq }).to_string())).map_err(|e| e.to_string())?;
            }
            Some(7) => return Err("Discord asked to reconnect".into()),
            Some(9) => return Err("session was rejected".into()),
            Some(0) => {
                if let Some(kind) = v["t"].as_str() {
                    let d = v["d"].clone();
                    let kind = kind.to_string();
                    // handled on this thread: Discord calls are quick and order matters
                    dispatch(&kind, &d);
                }
            }
            _ => {}
        }
    }
}

/// Starts the connection on its own thread, reconnecting whenever it drops.
pub fn start() {
    let (Some(token), Some(_)) = (env("DISCORD_BOT_TOKEN"), env("DISCORD_GUILD_ID")) else { return };
    let _ = log_channel; // looked up on first use
    std::thread::spawn(move || {
        let mut wait = 3;
        loop {
            let began = Instant::now();
            match session(&token) {
                Ok(()) => {}
                Err(e) => eprintln!("[gateway] {e}; reconnecting"),
            }
            wait = if began.elapsed() > Duration::from_secs(120) { 3 } else { (wait * 2).min(60) };
            std::thread::sleep(Duration::from_secs(wait));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_found_by_their_host() {
        assert_eq!(link_hosts("look https://Example.com/a?b=1 and http://sub.site.org:8080/x"), vec!["example.com", "sub.site.org"]);
        assert!(link_hosts("no links here, http alone or httpx").is_empty());
        assert!(site_allowed("murmint.app") && site_allowed("www.youtube.com") && !site_allowed("evil-murmint.app"));
        assert!(!site_allowed("murmint.app.evil.com"));
    }

    #[test]
    fn only_other_servers_invites_count() {
        assert!(has_foreign_invite("join discord.gg/abc123"));
        assert!(has_foreign_invite("https://discord.com/invite/xyz"));
        assert!(!has_foreign_invite("join https://discord.gg/MEfdzfyARV"));
        assert!(!has_foreign_invite("talk about discord, the app"));
    }

    #[test]
    fn banned_words_match_whole_words() {
        let words = vec!["darn".to_string(), "bad phrase".to_string()];
        assert!(has_banned_word("Oh DARN it", &words));
        assert!(has_banned_word("that is a Bad Phrase indeed", &words));
        assert!(!has_banned_word("darning socks", &words));
        assert!(!has_banned_word("hello", &[]));
    }

    #[test]
    fn messages_from_the_team_are_never_touched() {
        // an invite from someone with no roles and no settings on does nothing, and the cache stays tidy
        on_message(&json!({ "id": "1", "channel_id": "2", "guild_id": "nope", "author": { "id": "3" }, "content": "discord.gg/abc" }));
        assert!(mem(|m| m.messages.is_empty()), "other servers are ignored");
    }

    /// Connects and identifies with the real token to prove the intents are accepted. Handles no events.
    #[test]
    #[ignore]
    fn probe_gateway_accepts_the_intents() {
        let token = std::env::var("DISCORD_BOT_TOKEN").expect("token");
        let (mut ws, _) = tungstenite::connect(URL).expect("connect");
        set_timeout(&mut ws);
        let start = Instant::now();
        let mut outcome = String::from("no answer");
        while start.elapsed() < Duration::from_secs(25) {
            let m = match ws.read() {
                Ok(m) => m,
                Err(tungstenite::Error::Io(_)) => continue,
                Err(e) => {
                    outcome = format!("closed: {e}");
                    break;
                }
            };
            let Message::Text(body) = m else { continue };
            let v: Value = serde_json::from_str(&body).unwrap();
            match v["op"].as_u64() {
                Some(10) => {
                    let id = json!({ "op": 2, "d": { "token": token, "intents": INTENTS, "properties": { "os": "linux", "browser": "probe", "device": "probe" } } });
                    ws.send(Message::Text(id.to_string())).unwrap();
                }
                Some(0) if v["t"] == "READY" => {
                    outcome = "READY: the intents were accepted".into();
                    break;
                }
                _ => {}
            }
        }
        let _ = ws.close(None);
        println!("PROBE {outcome}");
    }
}
