//! Community features for the Murmint bot: polls, giveaways, reminders, support tickets, role buttons, custom tags,
//! welcome and auto-mod settings, and small utility commands. Settings and running polls/giveaways/reminders are kept
//! in store.rs. A background loop (`tick`) ends giveaways and sends reminders when they are due.

use crate::bot::{api, call, option, owner, role_id, ORANGE};
use crate::moderation::{actor, guild, made_at, member, now, post_json, s_opt, i_opt, who};
use crate::store;
use reqwest::Method;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

const EPHEMERAL: u64 = 1 << 6;
const COMPONENTS_V2: u64 = 1 << 15;
const PANEL: u64 = 1024 + 2048 + 65536; // view, write and read history in a ticket

// ---------- building blocks ----------
pub(crate) fn text(s: &str) -> Value {
    json!({ "type": 10, "content": s })
}
pub(crate) fn sep() -> Value {
    json!({ "type": 14, "divider": true, "spacing": 1 })
}
pub(crate) fn button(label: &str, id: &str, style: u64) -> Value {
    json!({ "type": 2, "style": style, "label": label.chars().take(80).collect::<String>(), "custom_id": id })
}
pub(crate) fn row(buttons: Vec<Value>) -> Value {
    json!({ "type": 1, "components": buttons })
}
pub(crate) fn container(parts: Vec<Value>) -> Value {
    json!({ "type": 17, "accent_color": ORANGE, "components": parts })
}
/// An interaction answer. `kind` 4 is a new message, 7 edits the message a button was on.
pub(crate) fn reply(kind: u64, parts: Vec<Value>, public: bool) -> Value {
    let flags = COMPONENTS_V2 | if public || kind == 7 { 0 } else { EPHEMERAL };
    json!({ "type": kind, "data": { "flags": flags, "allowed_mentions": { "parse": [] }, "components": [container(parts)] } })
}
pub(crate) fn say(s: &str) -> Value {
    reply(4, vec![text(s)], false)
}
fn say_public(s: &str) -> Value {
    reply(4, vec![text(s)], true)
}

pub fn rand_below(n: u64) -> u64 {
    static C: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    let mut x = nanos ^ C.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    if n == 0 { 0 } else { x % n }
}

/// "90", "10m", "2h", "1d", "1h30m" into seconds. A bare number is minutes.
pub fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase();
    if let Ok(n) = s.parse::<u64>() {
        return Some(n * 60);
    }
    let (mut total, mut num, mut any) = (0u64, String::new(), false);
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: u64 = num.parse().ok()?;
            num.clear();
            total += n * match c { 's' => 1, 'm' => 60, 'h' => 3600, 'd' => 86_400, 'w' => 604_800, _ => return None };
            any = true;
        }
    }
    (any && num.is_empty()).then_some(total)
}

pub(crate) fn short_id() -> String {
    format!("{:x}", now() * 1000 + rand_below(1000))
}

pub(crate) fn has_role(v: &Value, role: &str) -> bool {
    v["member"]["roles"].as_array().is_some_and(|r| r.iter().any(|x| x.as_str() == Some(role)))
}
pub(crate) fn is_team(v: &Value) -> bool {
    role_id("Murmint Team").is_some_and(|r| has_role(v, &r)) || actor(v) == owner()
}
fn team_only(v: &Value) -> Option<Value> {
    (!is_team(v)).then(|| say("Only the Murmint Team can do that."))
}

pub(crate) fn post_message(channel: &str, parts: Vec<Value>, users_to_ping: &[String]) -> Option<String> {
    let body = json!({
        "flags": COMPONENTS_V2,
        "allowed_mentions": { "parse": [], "users": users_to_ping },
        "components": [container(parts)],
    });
    post_json(channel, body)
}

// ---------- slash commands ----------
pub const NAMES: &[&str] = &[
    "poll", "giveaway", "remind", "ticketpanel", "rolepanel", "tag", "welcome", "autorole", "automod", "userinfo", "avatar", "serverinfo", "roll",
    "coinflip", "8ball", "choose",
];

pub fn run(name: &str, v: &Value) -> Value {
    let public_ok = ["poll", "remind", "tag", "userinfo", "avatar", "serverinfo", "roll", "coinflip", "8ball", "choose"];
    let team_cmd = !public_ok.contains(&name);
    if team_cmd {
        if let Some(no) = team_only(v) {
            return no;
        }
    }
    match name {
        "poll" => poll(v),
        "giveaway" => giveaway(v),
        "remind" => remind(v),
        "ticketpanel" => ticket_panel(v),
        "rolepanel" => role_panel(v),
        "tag" => tag(v),
        "welcome" => welcome(v),
        "autorole" => autorole(v),
        "automod" => automod(v),
        "userinfo" => userinfo(v),
        "avatar" => avatar(v),
        "serverinfo" => serverinfo(),
        "roll" => {
            let sides = i_opt(v, "sides").unwrap_or(6).clamp(2, 1000) as u64;
            let count = i_opt(v, "count").unwrap_or(1).clamp(1, 20) as usize;
            let rolls: Vec<u64> = (0..count).map(|_| rand_below(sides) + 1).collect();
            let total: u64 = rolls.iter().sum();
            say_public(&if count == 1 { format!("🎲 You rolled **{total}** (d{sides}).") } else { format!("🎲 {count}d{sides}: {} = **{total}**", rolls.iter().map(u64::to_string).collect::<Vec<_>>().join(" + ")) })
        }
        "coinflip" => say_public(if rand_below(2) == 0 { "🪙 Heads." } else { "🪙 Tails." }),
        "8ball" => {
            const A: [&str; 12] = ["It is certain.", "Without a doubt.", "Yes.", "Most likely.", "Signs point to yes.", "Ask again later.", "Cannot predict now.", "Better not tell you now.", "Don't count on it.", "My sources say no.", "Very doubtful.", "Outlook not so good."];
            let q = s_opt(v, "question").unwrap_or("");
            say_public(&format!("🎱 **{}**\n{}", q.chars().take(200).collect::<String>(), A[rand_below(12) as usize]))
        }
        "choose" => {
            let options: Vec<&str> = s_opt(v, "options").unwrap_or("").split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
            if options.len() < 2 {
                return say("Give me at least two options, separated by commas.");
            }
            say_public(&format!("I pick **{}**.", options[rand_below(options.len() as u64) as usize]))
        }
        _ => say("I don't know that command yet."),
    }
}

// ---------- polls ----------
fn poll_parts(id: &str, p: &Value) -> Vec<Value> {
    let options: Vec<String> = p["options"].as_array().map(|a| a.iter().filter_map(|o| o.as_str().map(String::from)).collect()).unwrap_or_default();
    let votes = p["votes"].as_object().cloned().unwrap_or_default();
    let total = votes.len().max(1);
    let mut lines = vec![];
    let mut buttons = vec![];
    for (i, o) in options.iter().enumerate() {
        let n = votes.values().filter(|x| x.as_u64() == Some(i as u64)).count();
        let filled = n * 12 / total;
        lines.push(format!("**{o}**\n`{}{}` {n} ({}%)", "█".repeat(filled), "░".repeat(12 - filled), n * 100 / total));
        buttons.push(button(o, &format!("poll:{id}:{i}"), 2));
    }
    vec![
        text(&format!("## {}", p["q"].as_str().unwrap_or("Poll"))),
        sep(),
        text(&lines.join("\n")),
        text(&format!("-# {} vote{} · tap an answer to vote or change your vote", votes.len(), if votes.len() == 1 { "" } else { "s" })),
        row(buttons),
    ]
}

fn poll(v: &Value) -> Value {
    let Some(q) = s_opt(v, "question") else { return say("Write the question.") };
    let options: Vec<String> = (1..=4).filter_map(|i| s_opt(v, &format!("option{i}")).map(|s| s.chars().take(80).collect())).collect();
    if options.len() < 2 {
        return say("Give at least two options.");
    }
    let id = v["id"].as_str().map(|s| s.chars().rev().take(8).collect::<String>()).unwrap_or_else(short_id);
    let p = json!({ "q": q.chars().take(200).collect::<String>(), "options": options, "votes": {}, "by": actor(v) });
    let parts = poll_parts(&id, &p);
    store::write(|s| s["polls"][&id] = p);
    reply(4, parts, true)
}

// ---------- giveaways ----------
fn giveaway_parts(id: &str, g: &Value) -> Vec<Value> {
    let entrants = g["entrants"].as_array().map(|a| a.len()).unwrap_or(0);
    let prize = g["prize"].as_str().unwrap_or("a prize");
    if g["done"].as_bool() == Some(true) {
        let winners: Vec<String> = g["winners"].as_array().map(|a| a.iter().filter_map(|w| w.as_str()).map(who).collect()).unwrap_or_default();
        return vec![
            text(&format!("## Giveaway ended: {prize}")),
            text(&if winners.is_empty() { "No one entered, so there was no winner.".into() } else { format!("**Winner{}:** {}", if winners.len() == 1 { "" } else { "s" }, winners.join(" ")) }),
            text(&format!("-# {entrants} entr{}", if entrants == 1 { "y" } else { "ies" })),
        ];
    }
    vec![
        text(&format!("## Giveaway: {prize}")),
        text(&format!("Ends <t:{}:R> · **{}** winner{} · hosted by {}", g["ends"].as_u64().unwrap_or(0), g["winners_n"].as_u64().unwrap_or(1), if g["winners_n"].as_u64().unwrap_or(1) == 1 { "" } else { "s" }, who(g["by"].as_str().unwrap_or("")))),
        row(vec![button("Enter", &format!("gv:{id}"), 3)]),
        text(&format!("-# {entrants} entr{} so far · tap again to leave", if entrants == 1 { "y" } else { "ies" })),
    ]
}

fn giveaway(v: &Value) -> Value {
    let sub = v["data"]["options"][0]["name"].as_str().unwrap_or("");
    let inner = json!({ "data": { "options": v["data"]["options"][0]["options"].clone() } });
    match sub {
        "start" => {
            let Some(prize) = s_opt(&inner, "prize") else { return say("What is the prize?") };
            let Some(secs) = s_opt(&inner, "duration").and_then(parse_duration).filter(|s| (60..=30 * 86_400).contains(s)) else {
                return say("Give a duration between 1 minute and 30 days, like `30m`, `2h` or `1d`.");
            };
            let n = i_opt(&inner, "winners").unwrap_or(1).clamp(1, 20);
            let channel = v["channel_id"].as_str().unwrap_or("").to_string();
            let id = short_id();
            let g = json!({ "prize": prize.chars().take(150).collect::<String>(), "ends": now() + secs, "winners_n": n, "entrants": [], "by": actor(v), "channel": channel, "done": false });
            let Some(msg) = post_message(&channel, giveaway_parts(&id, &g), &[]) else {
                return say("I couldn't post in this channel. Can I see and write here?");
            };
            let mut g = g;
            g["message"] = json!(msg);
            store::write(|s| s["giveaways"][&id] = g);
            say(&format!("Giveaway started. It ends in {}.", human(secs)))
        }
        "end" => {
            let channel = v["channel_id"].as_str().unwrap_or("");
            let id = store::read(|s| {
                s["giveaways"].as_object().and_then(|m| m.iter().filter(|(_, g)| g["done"] != json!(true) && g["channel"].as_str() == Some(channel)).max_by_key(|(_, g)| g["ends"].as_u64()).map(|(k, _)| k.clone()))
            });
            match id {
                Some(id) => {
                    finish_giveaway(&id);
                    say("Ended it and picked the winner.")
                }
                None => say("There's no running giveaway in this channel."),
            }
        }
        _ => say("Use `/giveaway start` or `/giveaway end`."),
    }
}

fn human(secs: u64) -> String {
    match secs {
        s if s >= 86_400 => format!("{} day{}", s / 86_400, if s / 86_400 == 1 { "" } else { "s" }),
        s if s >= 3600 => format!("{} hour{}", s / 3600, if s / 3600 == 1 { "" } else { "s" }),
        s => format!("{} minute{}", (s / 60).max(1), if s / 60 <= 1 { "" } else { "s" }),
    }
}

fn finish_giveaway(id: &str) {
    let Some(mut g) = store::read(|s| s["giveaways"][id].as_object().map(|_| s["giveaways"][id].clone())) else { return };
    if g["done"] == json!(true) {
        return;
    }
    let mut pool: Vec<String> = g["entrants"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut winners = vec![];
    for _ in 0..g["winners_n"].as_u64().unwrap_or(1) {
        if pool.is_empty() {
            break;
        }
        winners.push(pool.remove(rand_below(pool.len() as u64) as usize));
    }
    g["done"] = json!(true);
    g["winners"] = json!(winners);
    let (channel, msg) = (g["channel"].as_str().unwrap_or("").to_string(), g["message"].as_str().unwrap_or("").to_string());
    let _ = call(Method::PATCH, &format!("/channels/{channel}/messages/{msg}"), Some(json!({ "flags": COMPONENTS_V2, "components": [container(giveaway_parts(id, &g))] })), None);
    if !winners.is_empty() {
        let prize = g["prize"].as_str().unwrap_or("the prize").to_string();
        post_message(&channel, vec![text(&format!("Congratulations {}! You won **{prize}**.", winners.iter().map(|w| who(w)).collect::<Vec<_>>().join(" ")))], &winners);
    }
    store::write(|s| s["giveaways"][id] = g);
}

// ---------- reminders ----------
fn remind(v: &Value) -> Value {
    let (Some(when), Some(msg)) = (s_opt(v, "in"), s_opt(v, "message")) else { return say("Say when and what to remind you about.") };
    let Some(secs) = parse_duration(when).filter(|s| (60..=30 * 86_400).contains(s)) else {
        return say("Give a time between 1 minute and 30 days, like `10m`, `2h` or `1d`.");
    };
    let me = actor(v);
    let mine = store::read(|s| s["reminders"].as_array().map(|a| a.iter().filter(|r| r["user"].as_str() == Some(&me)).count()).unwrap_or(0));
    if mine >= 10 {
        return say("You already have 10 reminders waiting.");
    }
    let at = now() + secs;
    let r = json!({ "user": me, "channel": v["channel_id"], "at": at, "text": msg.chars().take(500).collect::<String>() });
    store::write(|s| {
        if !s["reminders"].is_array() {
            s["reminders"] = json!([]);
        }
        s["reminders"].as_array_mut().unwrap().push(r);
    });
    say(&format!("I'll remind you <t:{at}:R>."))
}

// ---------- tickets ----------
fn ticket_panel(v: &Value) -> Value {
    let channel = v["channel_id"].as_str().unwrap_or("");
    let title = s_opt(v, "title").unwrap_or("Need help?");
    let body = s_opt(v, "message").unwrap_or("Press the button to open a private channel with the Murmint team.");
    if post_message(channel, vec![text(&format!("## {title}")), text(body), row(vec![button("Open a ticket", "ticket:open", 1)])], &[]).is_some() {
        say("Posted the ticket panel here.")
    } else {
        say("I couldn't post here.")
    }
}

fn open_ticket(v: &Value) -> Value {
    let me = actor(v);
    let g = guild();
    let existing = api(Method::GET, &format!("/guilds/{g}/channels"), None);
    if let Some(list) = existing.as_ref().and_then(|c| c.as_array()) {
        if let Some(c) = list.iter().find(|c| c["topic"].as_str() == Some(me.as_str()) && c["name"].as_str().is_some_and(|n| n.starts_with("ticket-"))) {
            return say(&format!("You already have a ticket open: <#{}>.", c["id"].as_str().unwrap_or("")));
        }
    }
    let name: String = v["member"]["user"]["username"].as_str().unwrap_or("member").to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).take(20).collect();
    let mut overwrites = vec![
        json!({ "id": g, "type": 0, "deny": "1024" }),
        json!({ "id": me, "type": 1, "allow": PANEL.to_string() }),
    ];
    if let Some(team) = role_id("Murmint Team") {
        overwrites.push(json!({ "id": team, "type": 0, "allow": PANEL.to_string() }));
    }
    let body = json!({ "name": format!("ticket-{}", if name.is_empty() { "member".into() } else { name }), "type": 0, "topic": me, "permission_overwrites": overwrites });
    match call(Method::POST, &format!("/guilds/{g}/channels"), Some(body), Some("Support ticket")) {
        Ok(ch) => {
            let id = ch["id"].as_str().unwrap_or("").to_string();
            post_message(&id, vec![text(&format!("## Support ticket\nHi {}, the Murmint team will be with you soon. Tell us what's going on here.", who(&me))), row(vec![button("Close ticket", "ticket:close", 4)])], &[me.clone()]);
            say(&format!("Your ticket is open: <#{id}>."))
        }
        Err(_) => say("I couldn't open a ticket right now. Please ask in the server instead."),
    }
}

fn close_ticket(v: &Value) -> Value {
    let channel = v["channel_id"].as_str().unwrap_or("");
    let Ok(info) = call(Method::GET, &format!("/channels/{channel}"), None, None) else { return say("I couldn't read this channel.") };
    if !info["name"].as_str().is_some_and(|n| n.starts_with("ticket-")) {
        return say("This isn't a ticket.");
    }
    if info["topic"].as_str() != Some(actor(v).as_str()) && !is_team(v) {
        return say("Only the person who opened this ticket or the team can close it.");
    }
    crate::community::transcript(channel, info["name"].as_str().unwrap_or("ticket"), &actor(v));
    let _ = call(Method::DELETE, &format!("/channels/{channel}"), None, Some("Ticket closed"));
    say("Closing this ticket.")
}

// ---------- role buttons ----------
pub(crate) fn self_assignable_pub(role: &str) -> bool {
    self_assignable(role)
}

/// A role anyone may give themselves: no permissions, not managed, not the alpha or team role.
fn self_assignable(role: &str) -> bool {
    let Some(roles) = api(Method::GET, &format!("/guilds/{}/roles", guild()), None) else { return false };
    roles.as_array().into_iter().flatten().find(|r| r["id"].as_str() == Some(role)).is_some_and(|r| {
        let perms: u64 = r["permissions"].as_str().and_then(|p| p.parse().ok()).unwrap_or(1);
        let name = r["name"].as_str().unwrap_or("");
        perms == 0 && r["managed"] != json!(true) && role != guild() && name != "Alpha Tester" && name != "Murmint Team"
    })
}

fn role_panel(v: &Value) -> Value {
    let roles: Vec<&str> = (1..=4).filter_map(|i| s_opt(v, &format!("role{i}"))).collect();
    if roles.is_empty() {
        return say("Pick at least one role.");
    }
    for r in &roles {
        if !self_assignable(r) {
            return say(&format!("<@&{r}> can't be a button role. It has permissions, belongs to an integration, or is the alpha or team role."));
        }
    }
    let title = s_opt(v, "title").unwrap_or("Pick your roles");
    let names: Vec<String> = roles.iter().map(|r| format!("<@&{r}>")).collect();
    let buttons: Vec<Value> = roles
        .iter()
        .map(|r| {
            let label = api(Method::GET, &format!("/guilds/{}/roles", guild()), None)
                .and_then(|rs| rs.as_array().and_then(|a| a.iter().find(|x| x["id"].as_str() == Some(r)).and_then(|x| x["name"].as_str().map(String::from))))
                .unwrap_or_else(|| "Role".into());
            button(&label, &format!("role:{r}"), 2)
        })
        .collect();
    let channel = v["channel_id"].as_str().unwrap_or("");
    if post_message(channel, vec![text(&format!("## {title}\nTap a button to add or remove the role: {}", names.join(" "))), row(buttons)], &[]).is_some() {
        say("Posted the role panel here.")
    } else {
        say("I couldn't post here.")
    }
}

fn toggle_role(v: &Value, role: &str) -> Value {
    if !self_assignable(role) {
        return say("That role can't be changed with a button.");
    }
    let me = actor(v);
    let path = format!("/guilds/{}/members/{me}/roles/{role}", guild());
    if has_role(v, role) {
        match call(Method::DELETE, &path, None, Some("Role button")) {
            Ok(_) => say(&format!("Removed <@&{role}>.")),
            Err(_) => say("I couldn't change that role."),
        }
    } else {
        match call(Method::PUT, &path, None, Some("Role button")) {
            Ok(_) => say(&format!("You now have <@&{role}>.")),
            Err(_) => say("I couldn't change that role."),
        }
    }
}

// ---------- tags ----------
fn tag(v: &Value) -> Value {
    let sub = v["data"]["options"][0]["name"].as_str().unwrap_or("");
    let inner = json!({ "data": { "options": v["data"]["options"][0]["options"].clone() } });
    let name = s_opt(&inner, "name").map(|n| n.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(30).collect::<String>()).unwrap_or_default();
    match sub {
        "get" => match store::get(&format!("tags.{name}")).as_str() {
            Some(t) => say_public(t),
            None => say("There's no tag with that name. `/tag list` shows them."),
        },
        "list" => {
            let names: Vec<String> = store::read(|s| s["tags"].as_object().map(|m| m.keys().map(|k| format!("`{k}`")).collect()).unwrap_or_default());
            say(&if names.is_empty() { "No tags yet.".to_string() } else { format!("**Tags:** {}", names.join(" ")) })
        }
        "create" | "delete" => {
            if let Some(no) = team_only(v) {
                return no;
            }
            if name.is_empty() {
                return say("Give the tag a name using letters, numbers or dashes.");
            }
            if sub == "delete" {
                let had = store::write(|s| s["tags"].as_object_mut().and_then(|m| m.remove(&name)).is_some());
                return say(if had { "Deleted." } else { "There's no tag with that name." });
            }
            let Some(body) = s_opt(&inner, "text") else { return say("What should the tag say?") };
            store::write(|s| {
                if !s["tags"].is_object() {
                    s["tags"] = json!({});
                }
                s["tags"][&name] = json!(body.replace("\\n", "\n").chars().take(1500).collect::<String>());
            });
            say(&format!("Saved. Anyone can use it with `/tag get {name}`."))
        }
        _ => say("Use `/tag get`, `/tag list`, `/tag create` or `/tag delete`."),
    }
}

// ---------- welcome, autorole, automod settings ----------
fn welcome(v: &Value) -> Value {
    let sub = v["data"]["options"][0]["name"].as_str().unwrap_or("");
    let inner = json!({ "data": { "options": v["data"]["options"][0]["options"].clone() } });
    match sub {
        "set" => {
            let (Some(ch), Some(msg)) = (s_opt(&inner, "channel"), s_opt(&inner, "message")) else { return say("Pick a channel and write the message.") };
            store::set("welcome.channel", json!(ch));
            store::set("welcome.message", json!(msg.replace("\\n", "\n").chars().take(1500).collect::<String>()));
            say(&format!("Welcome messages will go to <#{ch}>. Use `/welcome preview` to see one."))
        }
        "off" => {
            store::set("welcome.channel", Value::Null);
            say("Welcome messages are off.")
        }
        "preview" => match (store::get("welcome.channel").as_str(), store::get("welcome.message").as_str()) {
            (Some(_), Some(m)) => say(&fill_welcome(m, &actor(v), 0)),
            _ => say("No welcome message is set yet. Use `/welcome set`."),
        },
        _ => say("Use `/welcome set`, `/welcome off` or `/welcome preview`."),
    }
}

pub fn fill_welcome(template: &str, user: &str, count: u64) -> String {
    template.replace("{user}", &who(user)).replace("{server}", "Murmint").replace("{count}", &count.to_string())
}

fn autorole(v: &Value) -> Value {
    match s_opt(v, "role") {
        Some(r) => {
            if !self_assignable(r) {
                return say(&format!("<@&{r}> can't be given automatically. It has permissions, belongs to an integration, or is the alpha or team role."));
            }
            store::set("autorole", json!(r));
            say(&format!("New members will get <@&{r}> when they join."))
        }
        None => {
            store::set("autorole", Value::Null);
            say("New members no longer get a role automatically.")
        }
    }
}

fn automod(v: &Value) -> Value {
    let sub = v["data"]["options"][0]["name"].as_str().unwrap_or("");
    let inner = json!({ "data": { "options": v["data"]["options"][0]["options"].clone() } });
    match sub {
        "status" => {
            let on = |k: &str| if store::get(&format!("automod.{k}")) == json!(true) { "on" } else { "off" };
            let words = store::get("automod.words").as_array().map(|a| a.len()).unwrap_or(0);
            say(&format!("## Auto-mod\n**Invite links:** {}\n**All other links:** {}\n**Spam:** {}\n**Mass mentions:** {}\n**Banned words:** {words}\n-# Murmint Team members are never checked.", on("invites"), on("links"), on("spam"), on("mentions")))
        }
        "toggle" => {
            let (Some(what), Some(state)) = (s_opt(&inner, "rule"), option(&inner, "on").and_then(|o| o.as_bool())) else { return say("Pick a rule and on or off.") };
            store::set(&format!("automod.{what}"), json!(state));
            say(&format!("{what} is now {}.", if state { "on" } else { "off" }))
        }
        "word" => {
            let (Some(action), Some(word)) = (s_opt(&inner, "action"), s_opt(&inner, "word")) else { return say("Say add or remove, and the word.") };
            let w = word.to_lowercase();
            store::write(|s| {
                if !s["automod"]["words"].is_array() {
                    s["automod"]["words"] = json!([]);
                }
                let list = s["automod"]["words"].as_array_mut().unwrap();
                list.retain(|x| x.as_str() != Some(w.as_str()));
                if action == "add" {
                    list.push(json!(w));
                }
            });
            say(&format!("{} `{w}`. The message is deleted when someone says it.", if action == "add" { "Added" } else { "Removed" }))
        }
        _ => say("Use `/automod status`, `/automod toggle` or `/automod word`."),
    }
}

// ---------- utility ----------
fn target(v: &Value) -> String {
    s_opt(v, "user").map(String::from).unwrap_or_else(|| actor(v))
}

fn avatar(v: &Value) -> Value {
    let id = target(v);
    let user = v["data"]["resolved"]["users"][&id].clone();
    let url = match user["avatar"].as_str() {
        Some(h) => format!("https://cdn.discordapp.com/avatars/{id}/{h}.png?size=1024"),
        None => format!("https://cdn.discordapp.com/embed/avatars/{}.png", (id.parse::<u64>().unwrap_or(0) >> 22) % 6),
    };
    reply(4, vec![text(&format!("## {}'s avatar", who(&id))), json!({ "type": 12, "items": [{ "media": { "url": url } }] })], true)
}

fn userinfo(v: &Value) -> Value {
    let id = target(v);
    let created = made_at(&id).map(|t| format!("<t:{t}:D>")).unwrap_or_else(|| "unknown".into());
    let (joined, roles) = match member(&id) {
        Ok(m) => (
            m["joined_at"].as_str().and_then(|s| s.get(..10)).unwrap_or("unknown").to_string(),
            m["roles"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).map(|r| format!("<@&{r}>")).collect::<Vec<_>>(),
        ),
        Err(_) => ("not in the server".into(), vec![]),
    };
    say_public(&format!("## {}\n**Account created:** {created}\n**Joined the server:** {joined}\n**Roles:** {}", who(&id), if roles.is_empty() { "none".into() } else { roles.join(" ") }))
}

fn serverinfo() -> Value {
    let Some(g) = api(Method::GET, &format!("/guilds/{}?with_counts=true", guild()), None) else { return say("I couldn't read the server.") };
    let created = made_at(&guild()).map(|t| format!("<t:{t}:D>")).unwrap_or_default();
    say_public(&format!(
        "## {}\n**Members:** {}\n**Roles:** {}\n**Boosts:** {}\n**Created:** {created}",
        g["name"].as_str().unwrap_or("Murmint"),
        g["approximate_member_count"].as_u64().unwrap_or(0),
        g["roles"].as_array().map(|r| r.len()).unwrap_or(0),
        g["premium_subscription_count"].as_u64().unwrap_or(0)
    ))
}

// ---------- buttons ----------
/// A press on one of the bot's buttons: polls, giveaways, tickets and role buttons.
pub fn component(v: &Value) -> Value {
    let id = v["data"]["custom_id"].as_str().unwrap_or("");
    let me = actor(v);
    let mut parts = id.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("poll"), Some(pid), Some(i)) => {
            let Ok(i) = i.parse::<u64>() else { return say("That button isn't valid.") };
            let updated = store::write(|s| {
                if s["polls"][pid].is_object() {
                    s["polls"][pid]["votes"][&me] = json!(i);
                    Some(s["polls"][pid].clone())
                } else {
                    None
                }
            });
            match updated {
                Some(p) => reply(7, poll_parts(pid, &p), true),
                None => say("This poll has closed."),
            }
        }
        (Some("gv"), Some(gid), _) => {
            let updated = store::write(|s| {
                if s["giveaways"][gid]["done"] == json!(false) {
                    let list = s["giveaways"][gid]["entrants"].as_array_mut()?;
                    if let Some(pos) = list.iter().position(|x| x.as_str() == Some(me.as_str())) {
                        list.remove(pos);
                    } else {
                        list.push(json!(me));
                    }
                    Some(s["giveaways"][gid].clone())
                } else {
                    None
                }
            });
            match updated {
                Some(g) => reply(7, giveaway_parts(gid, &g), true),
                None => say("This giveaway has ended."),
            }
        }
        (Some(kind @ ("faq" | "guide" | "trouble")), None, _) => crate::help::select(kind, v["data"]["values"][0].as_str().unwrap_or("")),
        (Some("bug"), Some(kind), _) => crate::help::bug_status(v, kind),
        (Some("mission"), Some(mid), _) => crate::community::mission_form(v, mid),
        (Some("msn"), Some(kind @ ("ok" | "no")), Some(rest)) => crate::community::review_click(v, kind, rest),
        (Some("reward"), Some(user), Some(level)) => crate::community::reward_delivered(v, user, level),
        (Some("idea"), Some(dir @ ("up" | "down")), Some(iid)) => crate::community::idea_vote(v, dir, iid),
        (Some("idea"), Some(kind), _) => crate::community::idea_status(v, kind),
        (Some("ticket"), Some("open"), _) => open_ticket(v),
        (Some("ticket"), Some("close"), _) => close_ticket(v),
        (Some("role"), Some(r), _) => toggle_role(v, r),
        _ => say("This button isn't used anymore."),
    }
}

// ---------- background loop ----------
/// Ends giveaways and sends reminders that have come due. Called every few seconds.
pub fn tick() {
    crate::help::watch_release();
    crate::community::tick();
    let t = now();
    let due: Vec<String> = store::read(|s| s["giveaways"].as_object().map(|m| m.iter().filter(|(_, g)| g["done"] == json!(false) && g["ends"].as_u64().is_some_and(|e| e <= t)).map(|(k, _)| k.clone()).collect()).unwrap_or_default());
    for id in due {
        finish_giveaway(&id);
    }
    let reminders: Vec<Value> = store::write(|s| {
        let Some(list) = s["reminders"].as_array_mut() else { return vec![] };
        let (due, rest): (Vec<Value>, Vec<Value>) = list.drain(..).partition(|r| r["at"].as_u64().is_some_and(|a| a <= t));
        *list = rest;
        due
    });
    for r in reminders {
        let (user, channel) = (r["user"].as_str().unwrap_or(""), r["channel"].as_str().unwrap_or(""));
        post_message(channel, vec![text(&format!("{} Reminder: {}", who(user), r["text"].as_str().unwrap_or("")))], &[user.to_string()]);
    }
}

/// The slash commands for these features.
pub fn definitions() -> Vec<Value> {
    let team = |mut c: Value| {
        c["default_member_permissions"] = json!("32");
        c
    };
    let mut poll_opts = vec![json!({ "type": 3, "name": "question", "description": "What are you asking?", "required": true, "max_length": 200 })];
    for i in 1..=4 {
        poll_opts.push(json!({ "type": 3, "name": format!("option{i}"), "description": format!("Answer {i}"), "required": i <= 2, "max_length": 80 }));
    }
    let channel_opt = json!({ "type": 7, "name": "channel", "description": "Which channel", "required": true, "channel_types": [0, 5] });
    let user_opt = json!({ "type": 6, "name": "user", "description": "Who (you, if left empty)" });
    let mut role_opts = vec![json!({ "type": 8, "name": "role1", "description": "A role people can give themselves", "required": true })];
    for i in 2..=4 {
        role_opts.push(json!({ "type": 8, "name": format!("role{i}"), "description": "Another role" }));
    }
    role_opts.push(json!({ "type": 3, "name": "title", "description": "A heading for the panel", "max_length": 100 }));
    vec![
        json!({ "name": "poll", "description": "Ask the server a question with buttons to vote", "options": poll_opts }),
        team(json!({ "name": "giveaway", "description": "Run a giveaway (team only)", "options": [
            { "type": 1, "name": "start", "description": "Start a giveaway in this channel", "options": [
                { "type": 3, "name": "prize", "description": "What is being given away", "required": true, "max_length": 150 },
                { "type": 3, "name": "duration", "description": "How long, like 30m, 2h or 1d", "required": true },
                { "type": 4, "name": "winners", "description": "How many winners (default 1)", "min_value": 1, "max_value": 20 } ] },
            { "type": 1, "name": "end", "description": "End the running giveaway in this channel now" } ] })),
        json!({ "name": "remind", "description": "Get a reminder later", "options": [
            { "type": 3, "name": "in", "description": "When, like 10m, 2h or 1d", "required": true },
            { "type": 3, "name": "message", "description": "What to remind you about", "required": true, "max_length": 500 } ] }),
        team(json!({ "name": "ticketpanel", "description": "Post a button that opens private support tickets (team only)", "options": [
            { "type": 3, "name": "title", "description": "Heading", "max_length": 100 },
            { "type": 3, "name": "message", "description": "Text above the button", "max_length": 500 } ] })),
        team(json!({ "name": "rolepanel", "description": "Post buttons people use to pick their own roles (team only)", "options": role_opts })),
        json!({ "name": "tag", "description": "Saved answers for common questions", "options": [
            { "type": 1, "name": "get", "description": "Show a tag", "options": [{ "type": 3, "name": "name", "description": "The tag's name", "required": true }] },
            { "type": 1, "name": "list", "description": "All the tags" },
            { "type": 1, "name": "create", "description": "Save a tag (team only)", "options": [
                { "type": 3, "name": "name", "description": "A short name", "required": true, "max_length": 30 },
                { "type": 3, "name": "text", "description": "What it says. Use \\n for a new line", "required": true, "max_length": 1500 } ] },
            { "type": 1, "name": "delete", "description": "Delete a tag (team only)", "options": [{ "type": 3, "name": "name", "description": "The tag's name", "required": true }] } ] }),
        team(json!({ "name": "welcome", "description": "Greet new members (team only)", "options": [
            { "type": 1, "name": "set", "description": "Set the channel and message", "options": [channel_opt.clone(),
                { "type": 3, "name": "message", "description": "Use {user}, {server} and {count}. Use \\n for a new line", "required": true, "max_length": 1500 }] },
            { "type": 1, "name": "off", "description": "Stop welcome messages" },
            { "type": 1, "name": "preview", "description": "See the welcome message" } ] })),
        team(json!({ "name": "autorole", "description": "Give new members a role automatically (team only)", "options": [{ "type": 8, "name": "role", "description": "The role (leave empty to turn it off)" }] })),
        team(json!({ "name": "automod", "description": "Automatic moderation (team only)", "options": [
            { "type": 1, "name": "status", "description": "What is switched on" },
            { "type": 1, "name": "toggle", "description": "Turn a rule on or off", "options": [
                { "type": 3, "name": "rule", "description": "Which rule", "required": true, "choices": [
                    { "name": "Invite links to other servers", "value": "invites" }, { "name": "All other links", "value": "links" },
                    { "name": "Spam (many messages fast)", "value": "spam" }, { "name": "Mass mentions (5 or more)", "value": "mentions" } ] },
                { "type": 5, "name": "on", "description": "On or off", "required": true } ] },
            { "type": 1, "name": "word", "description": "Add or remove a banned word", "options": [
                { "type": 3, "name": "action", "description": "Add or remove", "required": true, "choices": [{ "name": "Add", "value": "add" }, { "name": "Remove", "value": "remove" }] },
                { "type": 3, "name": "word", "description": "The word", "required": true, "max_length": 40 } ] } ] })),
        json!({ "name": "userinfo", "description": "Account and server info about someone", "options": [user_opt.clone()] }),
        json!({ "name": "avatar", "description": "Show someone's profile picture", "options": [user_opt] }),
        json!({ "name": "serverinfo", "description": "About this server" }),
        json!({ "name": "roll", "description": "Roll dice", "options": [
            { "type": 4, "name": "sides", "description": "Sides on each die (default 6)", "min_value": 2, "max_value": 1000 },
            { "type": 4, "name": "count", "description": "How many dice (default 1)", "min_value": 1, "max_value": 20 } ] }),
        json!({ "name": "coinflip", "description": "Flip a coin" }),
        json!({ "name": "8ball", "description": "Ask the magic 8-ball", "options": [{ "type": 3, "name": "question", "description": "Your question", "required": true, "max_length": 200 }] }),
        json!({ "name": "choose", "description": "Pick one option at random", "options": [{ "type": 3, "name": "options", "description": "Options separated by commas", "required": true, "max_length": 500 }] }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_are_understood() {
        assert_eq!(parse_duration("90"), Some(5400));
        assert_eq!(parse_duration("10m"), Some(600));
        assert_eq!(parse_duration("2h"), Some(7200));
        assert_eq!(parse_duration("1d"), Some(86_400));
        assert_eq!(parse_duration("1h30m"), Some(5400));
        assert_eq!(parse_duration("soon"), None);
        assert_eq!(parse_duration("5x"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn random_numbers_stay_in_range_and_vary() {
        let rolls: Vec<u64> = (0..200).map(|_| rand_below(6)).collect();
        assert!(rolls.iter().all(|r| *r < 6));
        assert!((0..6).all(|n| rolls.contains(&n)), "every face comes up");
        assert_eq!(rand_below(0), 0);
    }

    #[test]
    fn a_poll_counts_votes_and_a_changed_vote_moves() {
        let mut p = json!({ "q": "Pizza?", "options": ["Yes", "No"], "votes": {} });
        p["votes"]["a"] = json!(0);
        p["votes"]["b"] = json!(1);
        p["votes"]["b"] = json!(0);
        let shown = poll_parts("x", &p)[2]["content"].as_str().unwrap().to_string();
        assert!(shown.contains("**Yes**") && shown.contains("2 (100%)") && shown.contains("0 (0%)"), "{shown}");
    }

    #[test]
    fn giveaways_show_entries_and_winners() {
        let g = json!({ "prize": "A prize", "ends": 1, "winners_n": 1, "entrants": ["1", "2"], "by": "9", "done": false });
        assert!(giveaway_parts("g", &g).iter().any(|p| p["content"].as_str().is_some_and(|c| c.contains("2 entries so far"))));
        let done = json!({ "prize": "A prize", "entrants": ["1"], "winners": ["1"], "done": true });
        assert!(giveaway_parts("g", &done)[1]["content"].as_str().unwrap().contains("<@1>"));
    }

    #[test]
    fn the_welcome_message_fills_in_its_placeholders() {
        assert_eq!(fill_welcome("Hi {user}, you are member {count} of {server}", "5", 42), "Hi <@5>, you are member 42 of Murmint");
    }

    #[test]
    fn utility_commands_answer_without_the_network() {
        let r = run("roll", &json!({ "data": { "options": [{ "name": "sides", "value": 20 }] }, "member": { "user": { "id": "1" } } }));
        assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().contains("rolled"));
        let r = run("choose", &json!({ "data": { "options": [{ "name": "options", "value": "a" }] }, "member": { "user": { "id": "1" } } }));
        assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().contains("two options"));
    }

    #[test]
    fn team_settings_are_refused_for_everyone_else() {
        for name in ["giveaway", "ticketpanel", "rolepanel", "welcome", "autorole", "automod"] {
            let r = run(name, &json!({ "data": { "options": [] }, "member": { "roles": [], "user": { "id": "42" } } }));
            assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().starts_with("Only the Murmint Team"), "{name}");
        }
    }
}
