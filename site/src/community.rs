//! Records and community tools for the Murmint bot, kept in store.rs: warnings, the tester programme (confirmed bugs,
//! points, levels, reward tiers, bug-hunt missions), voting on feature ideas, a bug list for the team, AFK, sticky
//! messages, auto-replies, reminders you can list and cancel, scheduled announcements, ticket transcripts, and a few
//! small utilities.
//!
//! The tester programme only counts bugs the team has confirmed, so reports can't be farmed for rewards. The bot never
//! sends anything of value itself: when a tester reaches a level with a reward, it tells them, and tells the team to
//! deliver it, with a button to mark it done.

use crate::bot::{api, call, env, owner, role_id};
use crate::features::{button, container, is_team, parse_duration, post_message, rand_below, reply, row, say, sep, short_id, text};
use crate::moderation::{actor, channel_named, dm, i_opt, log_channel, now, s_opt, who};
use crate::store;
use reqwest::Method;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Instant;

pub const NAMES: &[&str] = &[
    "warnings", "profile", "top", "rewards", "mission", "missions", "bugs", "afk", "sticky", "autoreply", "reminders", "timestamp", "botinfo",
];

fn guild() -> String {
    crate::moderation::guild()
}
fn sub_of(v: &Value) -> (String, Value) {
    let sub = &v["data"]["options"][0];
    (sub["name"].as_str().unwrap_or("").to_string(), json!({ "data": { "options": sub["options"].clone() } }))
}
fn target(v: &Value) -> String {
    s_opt(v, "user").map(String::from).unwrap_or_else(|| actor(v))
}
fn bar(done: u64, total: u64, width: usize) -> String {
    let filled = if total == 0 { 0 } else { ((done * width as u64) / total).min(width as u64) as usize };
    format!("`{}{}`", "█".repeat(filled), "░".repeat(width - filled))
}

// ---------- tester levels ----------
/// (level, name, points needed)
pub const LADDER: &[(u64, &str, u64)] = &[
    (1, "Rookie", 1),
    (2, "Tester", 4),
    (3, "Bug Hunter", 10),
    (4, "Exterminator", 20),
    (5, "Debug Master", 40),
    (6, "Legend", 75),
];

pub fn level_for(points: u64) -> u64 {
    LADDER.iter().filter(|(_, _, p)| points >= *p).map(|(l, _, _)| *l).max().unwrap_or(0)
}
fn level_name(level: u64) -> &'static str {
    LADDER.iter().find(|(l, _, _)| *l == level).map(|(_, n, _)| *n).unwrap_or("Newcomer")
}
/// Points a bug earns once the team confirms it.
pub fn severity_points(kind: &str) -> u64 {
    match kind {
        "minor" => 1,
        "major" => 3,
        "critical" => 6,
        _ => 0,
    }
}
const MISSION_BONUS: u64 = 2;

fn points_of(user: &str) -> u64 {
    store::get(&format!("contrib.{user}.points")).as_u64().unwrap_or(0)
}

/// Changes a tester's points and counters, and handles any level they reach.
fn adjust(user: &str, points: i64, field: &str, count: i64) {
    let (before, after) = store::write(|s| {
        let c = &mut s["contrib"][user];
        let old = c["points"].as_u64().unwrap_or(0);
        let new = (old as i64 + points).max(0) as u64;
        c["points"] = json!(new);
        if !field.is_empty() {
            c[field] = json!((c[field].as_u64().unwrap_or(0) as i64 + count).max(0));
        }
        (old, new)
    });
    let (from, to) = (level_for(before), level_for(after));
    if to > from {
        level_up(user, from, to);
    }
}

fn level_up(user: &str, from: u64, to: u64) {
    for l in (from + 1)..=to {
        let name = level_name(l);
        let tier = store::get(&format!("rewards.tiers.{l}"));
        let reward = tier["text"].as_str().map(String::from);
        // a free incentive: the role for this level (Bug Hunter is the default for level 3)
        let role = tier["role"].as_str().map(String::from).or_else(|| if l == 3 { role_id("Bug Hunter") } else { None });
        if let Some(r) = &role {
            let _ = call(Method::PUT, &format!("/guilds/{}/members/{user}/roles/{r}", guild()), None, Some("Tester level reward"));
        }
        let mut msg = format!("**You reached level {l}: {name}.** Thank you for finding bugs. Every confirmed bug makes Murmint better.");
        if let Some(r) = &role {
            msg += &format!("\nYou earned the <@&{r}> role.");
        }
        if let Some(r) = &reward {
            msg += &format!("\n**You unlocked a reward:** {r}\nThe Murmint team will contact you about it.");
            store::write(|s| {
                if !s["rewards"]["claims"].is_array() {
                    s["rewards"]["claims"] = json!([]);
                }
                s["rewards"]["claims"].as_array_mut().unwrap().push(json!({ "user": user, "level": l, "text": r, "at": now(), "done": false }));
            });
            notify_team_of_reward(user, l, name, r);
        }
        dm(user, &msg);
    }
}

fn notify_team_of_reward(user: &str, level: u64, name: &str, reward: &str) {
    let parts = vec![
        text(&format!("## Reward to send\n{} reached **level {level}: {name}**.\n**Reward:** {reward}", who(user))),
        row(vec![button("Mark delivered", &format!("reward:{user}:{level}"), 3)]),
    ];
    let channel = env("DISCORD_REWARDS_CHANNEL").or_else(log_channel);
    match channel {
        Some(c) => {
            post_message(&c, parts, &[]);
        }
        None => dm(&owner(), &format!("**Reward to send:** {} reached level {level} ({name}). Reward: {reward}", who(user))),
    }
}

pub fn reward_delivered(v: &Value, user: &str, level: &str) -> Value {
    if !is_team(v) {
        return say("Only the Murmint Team can mark a reward delivered.");
    }
    let level_n: u64 = level.parse().unwrap_or(0);
    store::write(|s| {
        for c in s["rewards"]["claims"].as_array_mut().into_iter().flatten() {
            if c["user"].as_str() == Some(user) && c["level"].as_u64() == Some(level_n) {
                c["done"] = json!(true);
            }
        }
    });
    let mut card = v["message"]["components"][0].clone();
    if let Some(parts) = card["components"].as_array_mut() {
        parts.retain(|p| p["type"].as_u64() != Some(1));
        if let Some(t) = parts.first_mut() {
            let old = t["content"].as_str().unwrap_or("").to_string();
            t["content"] = json!(format!("{old}\n**Delivered** by {}", who(&actor(v))));
        }
    }
    json!({ "type": 7, "data": { "flags": 1 << 15, "allowed_mentions": { "parse": [] }, "components": [card] } })
}

// ---------- confirmed bugs ----------
/// A bug report was posted: remembers who sent it and which mission it belongs to. Returns the mission's title.
pub fn note_bug(bug: &str, user: &str, mission: Option<&str>) -> Option<String> {
    let found = mission.and_then(|m| {
        let rec = store::get(&format!("missions.{m}"));
        (rec.is_object() && rec["closed"] != json!(true)).then(|| (m.to_string(), rec["title"].as_str().unwrap_or("").to_string()))
    });
    store::write(|s| {
        s["bugs"][bug] = json!({ "user": user, "mission": found.as_ref().map(|f| f.0.clone()) });
        if let Some((m, _)) = &found {
            if !s["missions"][m]["bugs"].is_array() {
                s["missions"][m]["bugs"] = json!([]);
            }
            s["missions"][m]["bugs"].as_array_mut().unwrap().push(json!(bug));
        }
        let list = &mut s["reports"][user];
        if !list.is_array() {
            *list = json!([]);
        }
        list.as_array_mut().unwrap().push(json!(now()));
    });
    found.map(|f| f.1)
}

/// Five reports a day per person, so the bug list stays useful.
pub fn can_report(user: &str) -> bool {
    let cutoff = now().saturating_sub(86_400);
    store::get(&format!("reports.{user}")).as_array().map(|a| a.iter().filter(|t| t.as_u64().is_some_and(|t| t > cutoff)).count()).unwrap_or(0) < 5
}

/// The team changed a bug's status. Awards, adjusts or removes the reporter's points. Returns a note to send them.
pub fn triage(bug: &str, reporter: &str, kind: &str) -> Option<String> {
    let mission_bonus = if store::get(&format!("bugs.{bug}.mission")).is_string() { MISSION_BONUS } else { 0 };
    let credit = store::get(&format!("credit.{bug}"));
    let old = credit["points"].as_u64();
    match kind {
        "minor" | "major" | "critical" => {
            let new = severity_points(kind) + mission_bonus;
            match old {
                Some(o) if o == new => return None,
                Some(o) => adjust(reporter, new as i64 - o as i64, "", 0),
                None => adjust(reporter, new as i64, "confirmed", 1),
            }
            let fixed = credit["fixed"] == json!(true);
            store::set(&format!("credit.{bug}"), json!({ "user": reporter, "points": new, "sev": kind, "fixed": fixed }));
            Some(format!("Your bug **{bug}** was confirmed as **{kind}** and earned **{new} point{}**.", if new == 1 { "" } else { "s" }))
        }
        "duplicate" | "notabug" => {
            let o = old?;
            adjust(reporter, -(o as i64), "confirmed", -1);
            if credit["fixed"] == json!(true) {
                adjust(reporter, -1, "fixed", -1); // the fix bonus goes too
            }
            store::write(|s| {
                if let Some(m) = s["credit"].as_object_mut() {
                    m.remove(bug);
                }
            });
            Some(format!("Your bug report **{bug}** was marked as {}, so its points were removed.", if kind == "duplicate" { "a duplicate" } else { "not a bug" }))
        }
        "fixed" => {
            // fixing a bug nobody triaged counts it as a minor one
            if old.is_none() {
                let base = severity_points("minor") + mission_bonus;
                adjust(reporter, base as i64, "confirmed", 1);
                store::set(&format!("credit.{bug}"), json!({ "user": reporter, "points": base, "sev": "minor", "fixed": false }));
            }
            if store::get(&format!("credit.{bug}.fixed")) == json!(true) {
                return None;
            }
            adjust(reporter, 1, "fixed", 1);
            store::set(&format!("credit.{bug}.fixed"), json!(true));
            Some(format!("Your bug **{bug}** was fixed, and you earned **1 bonus point**. The fix arrives in a future build. Thank you."))
        }
        _ => None,
    }
}

// ---------- profile, top and rewards ----------
fn profile(v: &Value) -> Value {
    let id = target(v);
    let c = store::get(&format!("contrib.{id}"));
    let n = |k: &str| c[k].as_u64().unwrap_or(0);
    let points = points_of(&id);
    let level = level_for(points);
    let next = LADDER.iter().find(|(l, _, _)| *l == level + 1);
    let progress = match next {
        Some((l, name, need)) => {
            let base = LADDER.iter().find(|(x, _, _)| *x == level).map(|(_, _, p)| *p).unwrap_or(0);
            format!("{} {points}/{need} points to level {l}: {name}", bar(points - base, need - base, 12))
        }
        None => "You've reached the top level.".to_string(),
    };
    let mut all: Vec<(String, u64)> = store::read(|s| s["contrib"].as_object().map(|m| m.iter().map(|(k, c)| (k.clone(), c["points"].as_u64().unwrap_or(0))).collect()).unwrap_or_default());
    all.sort_by(|a, b| b.1.cmp(&a.1));
    let place = all.iter().position(|(u, _)| *u == id).map(|p| format!("#{}", p + 1)).unwrap_or_else(|| "unranked".into());
    reply(4, vec![text(&format!(
        "## {}\n**Level {level}: {}** · {points} points · rank {place}\n{progress}\n**Bugs confirmed:** {} ({} fixed)\n**Missions approved:** {}\n**Ideas the team planned:** {}\n-# Only bugs the team confirms earn points: minor 1 · major 3 · critical 6 · +2 for a mission bug · +1 when it's fixed · +2 for a mission staff approve.",
        who(&id), level_name(level), n("confirmed"), n("fixed"), n("missions"), n("ideas")))], true)
}

fn top() -> Value {
    let mut all: Vec<(String, u64)> = store::read(|s| s["contrib"].as_object().map(|m| m.iter().map(|(k, c)| (k.clone(), c["points"].as_u64().unwrap_or(0))).collect()).unwrap_or_default());
    all.sort_by(|a, b| b.1.cmp(&a.1));
    let lines: Vec<String> = all.iter().filter(|(_, p)| *p > 0).take(10).enumerate().map(|(i, (u, p))| format!("**{}.** {} · {p} points · {}", i + 1, who(u), level_name(level_for(*p)))).collect();
    reply(4, vec![text("## Top bug hunters"), sep(), text(&if lines.is_empty() { "No one has a confirmed bug yet. Be the first with `/bug`.".to_string() } else { lines.join("\n") })], true)
}

fn rewards(v: &Value) -> Value {
    let (sub, inner) = sub_of(v);
    let team_cmds = ["add", "remove", "pending", "delivered"];
    if team_cmds.contains(&sub.as_str()) && !is_team(v) {
        return say("Only the Murmint Team can do that.");
    }
    match sub.as_str() {
        "list" | "" => {
            let me = points_of(&actor(v));
            let mut lines = vec![];
            for (l, name, need) in LADDER {
                let tier = store::get(&format!("rewards.tiers.{l}"));
                let mut perks = vec![];
                if let Some(t) = tier["text"].as_str() {
                    perks.push(t.to_string());
                }
                if let Some(r) = tier["role"].as_str() {
                    perks.push(format!("<@&{r}> role"));
                } else if *l == 3 {
                    perks.push("Bug Hunter role".to_string());
                }
                let mark = if me >= *need { "\u{2714}" } else { "\u{25CB}" };
                lines.push(format!("{mark} **Level {l}: {name}** · {need} points{}", if perks.is_empty() { String::new() } else { format!("\n   {}", perks.join(" · ")) }));
            }
            reply(4, vec![
                text("## Tester levels and rewards"),
                sep(),
                text(&lines.join("\n")),
                text(&format!("You have **{me} points**. Only bugs the team confirms earn points (minor 1, major 3, critical 6, and +2 for a bug found on a mission). Rewards are given by the Murmint team for confirmed, original bugs, and the team decides what each one is. `/profile` shows your progress.")),
            ], false)
        }
        "add" => {
            let (Some(level), Some(body)) = (i_opt(&inner, "level"), s_opt(&inner, "reward")) else { return say("Pick a level and write the reward.") };
            if !(1..=LADDER.len() as i64).contains(&level) {
                return say("Pick a level from 1 to 6.");
            }
            let mut tier = json!({ "text": body.chars().take(200).collect::<String>() });
            if let Some(role) = s_opt(&inner, "role") {
                if !crate::features::self_assignable_pub(role) {
                    return say(&format!("<@&{role}> can't be a reward role. It has permissions, belongs to an integration, or is the alpha or team role."));
                }
                tier["role"] = json!(role);
            }
            store::set(&format!("rewards.tiers.{level}"), tier);
            say(&format!("Level {level} ({}) now rewards: **{body}**. When a tester gets there, they're told, and you get a note with a Mark delivered button.", level_name(level as u64)))
        }
        "remove" => {
            let Some(level) = i_opt(&inner, "level") else { return say("Which level?") };
            store::write(|s| {
                if let Some(m) = s["rewards"]["tiers"].as_object_mut() {
                    m.remove(&level.to_string());
                }
            });
            say(&format!("Removed the reward for level {level}."))
        }
        "pending" => {
            let lines: Vec<String> = store::read(|s| s["rewards"]["claims"].as_array().map(|a| a.iter().filter(|c| c["done"] != json!(true)).map(|c| format!("{} · level {} · {}", who(c["user"].as_str().unwrap_or("")), c["level"].as_u64().unwrap_or(0), c["text"].as_str().unwrap_or(""))).collect()).unwrap_or_default());
            say(&if lines.is_empty() { "No rewards are waiting to be sent.".to_string() } else { format!("## Rewards to send\n{}", lines.join("\n")) })
        }
        "delivered" => {
            let Some(user) = s_opt(&inner, "user") else { return say("Pick a user.") };
            let n = store::write(|s| {
                let mut n = 0;
                for c in s["rewards"]["claims"].as_array_mut().into_iter().flatten() {
                    if c["user"].as_str() == Some(user) && c["done"] != json!(true) {
                        c["done"] = json!(true);
                        n += 1;
                    }
                }
                n
            });
            say(&format!("Marked {n} reward{} for {} as delivered.", if n == 1 { "" } else { "s" }, who(user)))
        }
        _ => say("Use `/rewards list`."),
    }
}

// ---------- bug-hunt missions ----------
// A mission is finished by sending proof, and it only counts once a staff member approves it. The tester presses the button,
// fills in a form (what they did, from a menu; an image or recording; an optional note), the proof is posted for staff with
// Approve and Reject buttons, and the tester is told the result. Points are given only on approval.
const MISSION_IDEAS: &[(&str, &str)] = &[
    ("Dictate a list", "Hold Ctrl + Win and say three or more things in a row, like \"I need milk eggs bread and coffee\", in Slack, Discord or Notepad. Check it becomes a list. Also try saying \"numbered list\"."),
    ("Correct yourself out loud", "Say things like \"meet at 8 p.m., no wait, 9 p.m.\" and \"send it to Sam, no wait, Maria\". Only the fix should be typed. Look for cases where the wrong half stays or the meaning changes."),
    ("Message context", "Turn on Message context in Settings under Dictation. Type half a sentence in a text box, then dictate the rest. Check punctuation and capitals carry on naturally. Try a password field: nothing should be read."),
    ("The apps picker", "Open Apps, press Add, and check the popular apps on your PC are listed first, then everything else installed. Add two apps with different styles and dictate in each. Note any missing app, wrong icon or wrong style."),
    ("Hands-free for five minutes", "Press Ctrl + Alt + D and dictate hands-free for five minutes. Look for missed words, repeated text, delays, or it not stopping when you ask."),
    ("Emoji and symbols", "Say \"smiley face\", \"thumbs up emoji\", \"slash\" and \"five times\". They should type as characters and numerals. Then say \"give it a thumbs up\": that should stay words."),
    ("Different text boxes", "Dictate into Notepad, Word, a browser text box and a chat app. Report any app where nothing is typed, the text lands in the wrong place, or your clipboard isn't restored."),
    ("A two-minute dictation", "Speak for two minutes with natural pauses. Check punctuation follows your pauses, nothing is dropped, and the meaning is never changed."),
    ("Vocabulary", "Add a hard name under Vocabulary, record it five ways, then use it in sentences. Check it's written correctly, and that other words aren't changed by mistake."),
    ("A model on your PC", "Connect Ollama or LM Studio under Models and dictate with it. Check the result, the speed, and what happens if the model isn't running."),
    ("Install and update", "Install Murmint over an existing install, then open Settings and press Check now for updates. Look for broken settings, wrong messages, or anything lost."),
    ("Writing styles", "Dictate the same sentence in Natural, Casual, Professional, Formal, Concise and Technical. Each should change the tone and never the meaning."),
];

const MISSION_POINTS: i64 = 2;

fn clip(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n').take(n).collect::<String>().trim().to_string()
}

/// What a tester can tick off in the form: the things the mission asks for, one per sentence or line, or the ones the staff typed.
pub fn mission_tasks(steps: &str, given: Option<&str>) -> Vec<String> {
    let pieces: Vec<String> = match given.map(str::trim).filter(|g| !g.is_empty()) {
        Some(g) => g.split(|c| c == ';' || c == '\n').map(|s| clip(s, 100)).filter(|s| !s.is_empty()).take(9).collect(),
        None => steps
            .replace('\n', ". ")
            .split(". ")
            .map(|s| clip(s.trim().trim_end_matches('.'), 100))
            .filter(|s| s.chars().count() >= 8)
            .take(9)
            .collect(),
    };
    if pieces.is_empty() { vec!["The whole mission".to_string()] } else { pieces }
}

fn tasks_of(m: &Value) -> Vec<String> {
    match m["tasks"].as_array() {
        Some(a) if !a.is_empty() => a.iter().filter_map(|t| t.as_str().map(String::from)).collect(),
        _ => mission_tasks(m["steps"].as_str().unwrap_or(""), None),
    }
}

fn counts(m: &Value) -> (usize, usize) {
    let (mut ok, mut waiting) = (m["done"].as_array().map(|a| a.len()).unwrap_or(0), 0);
    for s in m["subs"].as_object().into_iter().flat_map(|o| o.values()) {
        match s["status"].as_str() {
            Some("approved") => ok += 1,
            Some("pending") => waiting += 1,
            _ => {}
        }
    }
    (ok, waiting)
}

fn mission_parts(id: &str, m: &Value) -> Vec<Value> {
    let (approved, waiting) = counts(m);
    let found = m["bugs"].as_array().map(|a| a.len()).unwrap_or(0);
    let closed = m["closed"] == json!(true);
    let mut tail = format!("{approved} approved · {waiting} waiting for review · {found} bug{} reported", if found == 1 { "" } else { "s" });
    if let Some(end) = m["ends"].as_u64() {
        tail = format!("{} <t:{end}:R> · {tail}", if closed { "Ended" } else { "Ends" });
    }
    let mut parts = vec![
        text(&format!("## Bug hunt: {}\n{}", m["title"].as_str().unwrap_or("Mission"), m["steps"].as_str().unwrap_or(""))),
        text(&format!("**When you're done**, press the button, say what you completed and upload a screenshot or recording. A staff member checks it, and you earn **{MISSION_POINTS} points** once they approve it.\n**Found a bug?** Use `/bug` and enter the mission code **{id}**. Each bug the team confirms earns **+{MISSION_BONUS} bonus points**.")),
        text(&format!("-# {tail}")),
    ];
    if !closed {
        parts.push(row(vec![button("I finished it", &format!("mission:{id}"), 3)]));
    }
    parts
}

fn mission(v: &Value) -> Value {
    let (sub, inner) = sub_of(v);
    let channel = v["channel_id"].as_str().unwrap_or("").to_string();
    let (title, steps, days, tasks) = match sub.as_str() {
        "new" => {
            let (Some(t), Some(s)) = (s_opt(&inner, "title"), s_opt(&inner, "steps")) else { return say("Give the mission a title and say what to test.") };
            let steps: String = s.replace("\\n", "\n").chars().take(1000).collect();
            (t.chars().take(80).collect::<String>(), steps.clone(), i_opt(&inner, "days"), mission_tasks(&steps, s_opt(&inner, "tasks")))
        }
        "random" => {
            let (t, s) = MISSION_IDEAS[rand_below(MISSION_IDEAS.len() as u64) as usize];
            (t.to_string(), s.to_string(), i_opt(&inner, "days"), mission_tasks(s, None))
        }
        "end" => {
            let Some(id) = s_opt(&inner, "code") else { return say("Give the mission code.") };
            return if close_mission(id) { say("Mission ended.") } else { say("I couldn't find a running mission with that code.") };
        }
        _ => return say("Use `/mission new`, `/mission random` or `/mission end`."),
    };
    let id = short_id();
    let ends = days.filter(|d| (1..=30).contains(d)).map(|d| now() + d as u64 * 86_400);
    let mut m = json!({ "title": title, "steps": steps, "tasks": tasks, "subs": {}, "bugs": [], "closed": false, "channel": channel, "by": actor(v) });
    if let Some(e) = ends {
        m["ends"] = json!(e);
    }
    let Some(msg) = post_message(&channel, mission_parts(&id, &m), &[]) else { return say("I couldn't post here.") };
    m["message"] = json!(msg);
    store::write(|s| s["missions"][&id] = m);
    say(&format!("Mission posted. Its code is **{id}**. Testers who finish it send proof, and you approve it in the review channel."))
}

fn close_mission(id: &str) -> bool {
    let Some(mut m) = store::read(|s| s["missions"][id].is_object().then(|| s["missions"][id].clone())) else { return false };
    if m["closed"] == json!(true) {
        return false;
    }
    m["closed"] = json!(true);
    let (channel, msg) = (m["channel"].as_str().unwrap_or("").to_string(), m["message"].as_str().unwrap_or("").to_string());
    if !msg.is_empty() {
        let _ = call(Method::PATCH, &format!("/channels/{channel}/messages/{msg}"), Some(json!({ "flags": 1 << 15, "components": [container(mission_parts(id, &m))] })), None);
    }
    store::write(|s| s["missions"][id] = m);
    true
}

fn missions() -> Value {
    let list: Vec<String> = store::read(|s| {
        s["missions"].as_object().map(|m| m.iter().filter(|(_, x)| x["closed"] != json!(true)).map(|(id, x)| {
            let link = match (x["channel"].as_str(), x["message"].as_str()) {
                (Some(c), Some(msg)) => format!("https://discord.com/channels/{}/{c}/{msg}", crate::moderation::guild()),
                _ => String::new(),
            };
            format!("**{}** (code `{id}`){}", x["title"].as_str().unwrap_or(""), if link.is_empty() { String::new() } else { format!(" · [open]({link})") })
        }).collect()).unwrap_or_default()
    });
    reply(4, vec![text("## Open bug hunts"), sep(), text(&if list.is_empty() { "No missions right now. Report any bug you find with `/bug`.".to_string() } else { list.join("\n") })], false)
}

/// Staff: the Murmint Team, the owner, or anyone with the Administrator permission.
pub(crate) fn is_staff(v: &Value) -> bool {
    is_team(v) || v["member"]["permissions"].as_str().and_then(|p| p.parse::<u64>().ok()).is_some_and(|p| p & 8 != 0)
}

// ---- the form ----
fn proof_form(id: &str, m: &Value) -> Value {
    let mut options: Vec<Value> = tasks_of(m).iter().enumerate().map(|(i, t)| json!({ "label": clip(t, 100), "value": format!("t{i}") })).collect();
    options.push(json!({ "label": "Something else (say what in the note)", "value": "other" }));
    let n = options.len().min(10);
    json!({ "type": 9, "data": {
        "custom_id": format!("mission_proof:{id}"),
        "title": clip(&format!("Finished: {}", m["title"].as_str().unwrap_or("mission")), 45),
        "components": [
            { "type": 18, "label": "What did you complete?", "description": "Pick everything you did.",
              "component": { "type": 3, "custom_id": "done", "placeholder": "Choose what you finished", "min_values": 1, "max_values": n, "required": true, "options": options } },
            { "type": 18, "label": "Proof", "description": "A screenshot or a short screen recording.",
              "component": { "type": 19, "custom_id": "proof", "min_values": 1, "max_values": 3, "required": true } },
            { "type": 18, "label": "Anything to add? (optional)",
              "component": { "type": 4, "custom_id": "notes", "style": 2, "required": false, "max_length": 500 } },
        ] } })
}

/// The button on a mission card: opens the form, unless proof is already in or the mission has ended.
pub fn mission_form(v: &Value, id: &str) -> Value {
    let Some(m) = store::read(|s| s["missions"][id].is_object().then(|| s["missions"][id].clone())) else { return say("This mission isn't available anymore.") };
    if m["closed"] == json!(true) {
        return say("This mission has ended.");
    }
    let me = actor(v);
    for s in m["subs"].as_object().into_iter().flat_map(|o| o.values()) {
        if s["user"].as_str() == Some(me.as_str()) {
            match s["status"].as_str() {
                Some("approved") => return say("Staff already approved your proof for this one. Thank you!"),
                Some("pending") => return say("Your proof is waiting for a staff member to check it. You'll get a message when it's done."),
                _ => {}
            }
        }
    }
    if m["done"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(me.as_str()))) {
        return say("You already finished this one. Thank you.");
    }
    proof_form(id, &m)
}

/// Every answer in a submitted form, by the name of its field, however deeply the form nests them.
pub(crate) fn answers(v: &Value, out: &mut std::collections::HashMap<String, Vec<String>>) {
    match v {
        Value::Object(o) => {
            if let Some(cid) = o.get("custom_id").and_then(|c| c.as_str()) {
                let vals: Vec<String> = match (o.get("values"), o.get("value")) {
                    (Some(Value::Array(a)), _) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                    (_, Some(Value::String(s))) => vec![s.clone()],
                    _ => vec![],
                };
                out.insert(cid.to_string(), vals);
            }
            for x in o.values() {
                answers(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| answers(x, out)),
        _ => {}
    }
}

#[derive(Debug)]
pub struct Proof {
    pub tasks: Vec<String>,
    pub files: Vec<(String, String, String)>, // name, url, content type
    pub notes: String,
}

pub fn parse_proof(v: &Value, m: &Value) -> Result<Proof, String> {
    let mut a = std::collections::HashMap::new();
    answers(&v["data"]["components"], &mut a);
    let tasks_all = tasks_of(m);
    let tasks: Vec<String> = a
        .get("done")
        .map(|picked| {
            picked
                .iter()
                .filter_map(|p| if p == "other" { Some("Something else".to_string()) } else { p.strip_prefix('t').and_then(|n| n.parse::<usize>().ok()).and_then(|i| tasks_all.get(i).cloned()) })
                .collect()
        })
        .unwrap_or_default();
    if tasks.is_empty() {
        return Err("Pick what you completed.".into());
    }
    let mut files = vec![];
    for id in a.get("proof").cloned().unwrap_or_default() {
        let f = &v["data"]["resolved"]["attachments"][&id];
        let (name, url, ctype) = (f["filename"].as_str().unwrap_or("proof"), f["url"].as_str().unwrap_or(""), f["content_type"].as_str().unwrap_or(""));
        let lower = name.to_lowercase();
        let is_media = ctype.starts_with("image/") || ctype.starts_with("video/") || [".png", ".jpg", ".jpeg", ".gif", ".webp", ".mp4", ".webm", ".mov"].iter().any(|e| lower.ends_with(e));
        if url.is_empty() || !is_media {
            return Err("The proof has to be an image or a screen recording.".into());
        }
        files.push((clip(name, 80), url.to_string(), ctype.to_string()));
    }
    if files.is_empty() {
        return Err("Add a screenshot or a recording as proof.".into());
    }
    Ok(Proof { tasks, files: files.into_iter().take(3).collect(), notes: clip(a.get("notes").and_then(|n| n.first()).map(|s| s.as_str()).unwrap_or(""), 500) })
}

/// A submitted form, either a tester's proof or a staff member's reason for rejecting one.
pub fn modal_submit(v: &Value) -> Value {
    let cid = v["data"]["custom_id"].as_str().unwrap_or("");
    if let Some(rest) = cid.strip_prefix("msnno:") {
        let mut a = std::collections::HashMap::new();
        answers(&v["data"]["components"], &mut a);
        let reason = a.get("reason").and_then(|r| r.first()).cloned().unwrap_or_default();
        return finish_review(v, rest, false, &reason);
    }
    let Some(id) = cid.strip_prefix("mission_proof:") else { return say("That form isn't used anymore.") };
    let Some(m) = store::read(|s| s["missions"][id].is_object().then(|| s["missions"][id].clone())) else { return say("This mission isn't available anymore.") };
    if m["closed"] == json!(true) {
        return say("This mission ended before your proof arrived.");
    }
    let me = actor(v);
    if m["subs"].as_object().into_iter().flat_map(|o| o.values()).any(|s| s["user"].as_str() == Some(me.as_str()) && matches!(s["status"].as_str(), Some("pending") | Some("approved"))) {
        return say("You already sent proof for this mission.");
    }
    let proof = match parse_proof(v, &m) {
        Ok(p) => p,
        Err(e) => return say(&e),
    };
    let sid = short_id();
    let sub = json!({
        "user": me, "tasks": proof.tasks, "notes": proof.notes, "status": "pending", "at": now(),
        "files": proof.files.iter().map(|(n, u, c)| json!({ "name": n, "url": u, "type": c })).collect::<Vec<_>>(),
    });
    store::write(|s| s["missions"][id]["subs"][&sid] = sub);
    // fetching the files and posting for review can take a few seconds, so it happens after this answer goes back
    let (mid, sid2) = (id.to_string(), sid);
    std::thread::spawn(move || post_for_review(&mid, &sid2));
    say("Thanks! Your proof is with the team. A staff member will check it, and you'll get a message when it's approved.")
}

fn review_channel() -> Option<String> {
    env("DISCORD_MISSION_REVIEW_CHANNEL").or_else(|| channel_named("mission-review")).or_else(|| channel_named("staff-chat")).or_else(log_channel)
}

/// Fetches an uploaded file from Discord's own servers (and only from there).
fn fetch_file(url: &str) -> Option<Vec<u8>> {
    let host = url.strip_prefix("https://")?.split('/').next()?;
    if !(host == "cdn.discordapp.com" || host == "media.discordapp.net") {
        return None;
    }
    let r = crate::auth::http().get(url).send().ok()?;
    if !r.status().is_success() || r.content_length().is_some_and(|n| n > 10_000_000) {
        return None;
    }
    let bytes = r.bytes().ok()?;
    (bytes.len() <= 10_000_000).then(|| bytes.to_vec())
}

/// Posts a tester's proof for staff, with the files copied in so they stay available.
fn post_for_review(mid: &str, sid: &str) {
    let (Some(m), Some(sub)) = (
        store::read(|s| s["missions"][mid].is_object().then(|| s["missions"][mid].clone())),
        store::read(|s| s["missions"][mid]["subs"][sid].is_object().then(|| s["missions"][mid]["subs"][sid].clone())),
    ) else { return };
    let user = sub["user"].as_str().unwrap_or("");
    let tasks: Vec<String> = sub["tasks"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(|t| format!("• {t}"))).collect()).unwrap_or_default();
    let mut files: Vec<(String, Vec<u8>, String)> = vec![];
    let mut missed: Vec<String> = vec![];
    for (i, f) in sub["files"].as_array().into_iter().flatten().enumerate() {
        let (name, url, ctype) = (f["name"].as_str().unwrap_or("proof"), f["url"].as_str().unwrap_or(""), f["type"].as_str().unwrap_or("application/octet-stream"));
        match fetch_file(url) {
            Some(bytes) => files.push((format!("proof{}-{}", i + 1, name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_').collect::<String>()), bytes, if ctype.is_empty() { "application/octet-stream".into() } else { ctype.to_string() })),
            None => missed.push(url.to_string()),
        }
    }
    let mut parts = vec![text(&format!(
        "## Mission proof: {}\n{} says they finished:\n{}{}{}\n-# Mission {mid} · proof {sid}",
        m["title"].as_str().unwrap_or("mission"), who(user), tasks.join("\n"),
        if sub["notes"].as_str().is_some_and(|n| !n.is_empty()) { format!("\n**Note:** {}", sub["notes"].as_str().unwrap_or("")) } else { String::new() },
        if missed.is_empty() { String::new() } else { format!("\n**Files that couldn't be copied** (these links stop working after a day): {}", missed.join(" ")) }
    ))];
    if !files.is_empty() {
        parts.push(json!({ "type": 12, "items": files.iter().map(|(n, _, _)| json!({ "media": { "url": format!("attachment://{n}") } })).collect::<Vec<_>>() }));
    }
    parts.push(row(vec![button("Approve", &format!("msn:ok:{mid}:{sid}"), 3), button("Reject", &format!("msn:no:{mid}:{sid}"), 4)]));
    let payload = json!({
        "flags": 1 << 15, "allowed_mentions": { "parse": [] },
        "components": [container(parts.clone())],
        "attachments": files.iter().enumerate().map(|(i, (n, _, _))| json!({ "id": i, "filename": n })).collect::<Vec<_>>(),
    });
    let Some(channel) = review_channel() else {
        dm(&owner(), &format!("A mission proof needs review but there's no review channel: {} · {}. Create a channel called mission-review.", who(user), m["title"].as_str().unwrap_or("mission")));
        return;
    };
    let posted = if files.is_empty() { post_message(&channel, parts, &[]) } else { crate::store::post_files(&channel, payload, files) };
    if let Some(msg) = posted {
        store::set(&format!("missions.{mid}.subs.{sid}.review"), json!({ "channel": channel, "message": msg }));
    }
}

/// Staff pressed Approve or Reject on a proof.
pub fn review_click(v: &Value, kind: &str, rest: &str) -> Value {
    if !is_staff(v) {
        return say("Only staff can review mission proof.");
    }
    if kind == "no" {
        return json!({ "type": 9, "data": {
            "custom_id": format!("msnno:{rest}"), "title": "Reject this proof",
            "components": [{ "type": 18, "label": "Why? (the tester sees this)", "component": { "type": 4, "custom_id": "reason", "style": 2, "required": false, "max_length": 300 } }] } });
    }
    finish_review(v, rest, true, "")
}

/// Records the decision, tells the tester, awards the points, and updates the review post.
fn finish_review(v: &Value, rest: &str, approve: bool, reason: &str) -> Value {
    if !is_staff(v) {
        return say("Only staff can review mission proof.");
    }
    let Some((mid, sid)) = rest.split_once(':') else { return say("That review isn't valid.") };
    let Some(sub) = store::read(|s| s["missions"][mid]["subs"][sid].is_object().then(|| s["missions"][mid]["subs"][sid].clone())) else { return say("That proof isn't available anymore.") };
    if sub["status"].as_str() != Some("pending") {
        return say("That proof was already reviewed.");
    }
    let (user, title) = (sub["user"].as_str().unwrap_or("").to_string(), store::get(&format!("missions.{mid}.title")).as_str().unwrap_or("the mission").to_string());
    let by = actor(v);
    store::write(|s| {
        let rec = &mut s["missions"][mid]["subs"][sid];
        rec["status"] = json!(if approve { "approved" } else { "rejected" });
        rec["by"] = json!(by);
        rec["reviewed"] = json!(now());
        if !reason.is_empty() {
            rec["reason"] = json!(reason);
        }
    });
    if approve {
        adjust(&user, MISSION_POINTS, "missions", 1);
        dm(&user, &format!("**Your mission proof was approved.** {title}\nYou earned **{MISSION_POINTS} points**. Thank you for testing!"));
    } else {
        dm(&user, &format!("**Your mission proof wasn't approved.** {title}{}\nYou're welcome to try again and send new proof.", if reason.is_empty() { String::new() } else { format!("\nStaff said: {reason}") }));
    }
    // update the review post: drop the buttons and say what was decided
    let decided = format!("**{}** by {}{}", if approve { "Approved" } else { "Rejected" }, who(&by), if reason.is_empty() { String::new() } else { format!(": {reason}") });
    if v["type"].as_u64() == Some(3) {
        let mut card = v["message"]["components"][0].clone();
        if let Some(parts) = card["components"].as_array_mut() {
            parts.retain(|p| p["type"].as_u64() != Some(1));
            parts.push(text(&decided));
        }
        return json!({ "type": 7, "data": { "flags": 1 << 15, "allowed_mentions": { "parse": [] }, "components": [card] } });
    }
    // a rejection comes back through the reason form, so the post is edited directly
    let (chan, msg) = (store::get(&format!("missions.{mid}.subs.{sid}.review.channel")), store::get(&format!("missions.{mid}.subs.{sid}.review.message")));
    if let (Some(chan), Some(msg)) = (chan.as_str(), msg.as_str()) {
        if let Ok(cur) = call(Method::GET, &format!("/channels/{chan}/messages/{msg}"), None, None) {
            let mut card = cur["components"][0].clone();
            if let Some(parts) = card["components"].as_array_mut() {
                parts.retain(|p| p["type"].as_u64() != Some(1));
                parts.push(text(&decided));
            }
            let _ = call(Method::PATCH, &format!("/channels/{chan}/messages/{msg}"), Some(json!({ "flags": 1 << 15, "components": [card] })), None);
        }
    }
    say(&if approve { "Approved. They've been told.".to_string() } else { "Rejected. They've been told.".to_string() })
}

// ---------- ideas: votes and status ----------
pub fn idea_vote_row(id: &str, up: usize, down: usize) -> Value {
    row(vec![button(&format!("\u{25B2} {up}"), &format!("idea:up:{id}"), 3), button(&format!("\u{25BC} {down}"), &format!("idea:down:{id}"), 4)])
}

pub fn idea_status_row() -> Value {
    row(vec![
        button("Considering", "idea:considering", 2),
        button("Planned", "idea:planned", 2),
        button("Done", "idea:done", 3),
        button("Declined", "idea:declined", 2),
        button("Reopen", "idea:reopen", 2),
    ])
}

/// The card with its two vote buttons relabelled.
pub fn relabel_votes(card: &Value, id: &str, up: usize, down: usize) -> Value {
    let mut c = card.clone();
    if let Some(parts) = c["components"].as_array_mut() {
        for p in parts.iter_mut() {
            if p["components"].as_array().is_some_and(|b| b.first().and_then(|x| x["custom_id"].as_str()).is_some_and(|i| i.starts_with("idea:up:"))) {
                *p = idea_vote_row(id, up, down);
            }
        }
    }
    c
}

pub fn idea_vote(v: &Value, dir: &str, id: &str) -> Value {
    let me = actor(v);
    let counts = store::write(|s| {
        let rec = &mut s["ideas"][id];
        for side in ["up", "down"] {
            if !rec[side].is_array() {
                rec[side] = json!([]);
            }
        }
        let (mine, other) = if dir == "up" { ("up", "down") } else { ("down", "up") };
        rec[other].as_array_mut().unwrap().retain(|x| x.as_str() != Some(me.as_str()));
        let list = rec[mine].as_array_mut().unwrap();
        if let Some(pos) = list.iter().position(|x| x.as_str() == Some(me.as_str())) {
            list.remove(pos);
        } else {
            list.push(json!(me));
        }
        (rec["up"].as_array().map(|a| a.len()).unwrap_or(0), rec["down"].as_array().map(|a| a.len()).unwrap_or(0))
    });
    let card = relabel_votes(&v["message"]["components"][0], id, counts.0, counts.1);
    json!({ "type": 7, "data": { "flags": 1 << 15, "allowed_mentions": { "parse": [] }, "components": [card] } })
}

pub fn idea_status(v: &Value, kind: &str) -> Value {
    if !is_team(v) {
        return say("Only the Murmint Team can change an idea's status.");
    }
    let label = match kind {
        "considering" => "Considering",
        "planned" => "Planned",
        "done" => "Done",
        "declined" => "Declined",
        "reopen" => "New",
        _ => return say("That button isn't used anymore."),
    };
    let card = &v["message"]["components"][0];
    let Some((updated, reporter)) = crate::help::set_status(card, if label == "New" { "Open" } else { label }, &actor(v)) else { return say("I couldn't read that idea.") };
    // the status line says "Open" for a reopened idea; show "New" instead
    let mut updated = updated;
    if label == "New" {
        if let Some(parts) = updated["components"].as_array_mut() {
            for p in parts.iter_mut() {
                if let Some(t) = p["content"].as_str().filter(|t| t.starts_with("**Status:** Open")) {
                    p["content"] = json!(t.replacen("Open", "New", 1));
                }
            }
        }
    }
    if matches!(kind, "planned" | "done") {
        let title = updated["components"][0]["content"].as_str().unwrap_or("your idea").trim_start_matches("## ").to_string();
        let id = title.split(':').next().unwrap_or("").to_string();
        if store::get(&format!("ideacredit.{id}")) != json!(true) {
            store::set(&format!("ideacredit.{id}"), json!(true));
            adjust(&reporter, 1, "ideas", 1);
        }
        dm(&reporter, &format!("**Your idea is now marked {label}.** {title}\nThank you for helping shape Murmint."));
    }
    json!({ "type": 7, "data": { "flags": 1 << 15, "allowed_mentions": { "parse": [] }, "components": [updated] } })
}

// ---------- warnings ----------
/// Records a warning and returns how many the person has now.
pub fn record_warning(user: &str, by: &str, reason: &str) -> usize {
    let entry = json!({ "at": now(), "by": by, "reason": reason });
    store::write(|s| {
        if !s["warns"][user].is_array() {
            s["warns"][user] = json!([]);
        }
        let list = s["warns"][user].as_array_mut().unwrap();
        list.push(entry);
        list.len()
    })
}

fn warnings(v: &Value) -> Value {
    let (sub, inner) = sub_of(v);
    let Some(user) = s_opt(&inner, "user") else { return say("Pick a user.") };
    match sub.as_str() {
        "list" => {
            let list: Vec<Value> = store::get(&format!("warns.{user}")).as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                return say(&format!("{} has no warnings.", who(user)));
            }
            let lines: Vec<String> = list.iter().enumerate().map(|(i, w)| format!("**{}.** <t:{}:d> by {}: {}", i + 1, w["at"].as_u64().unwrap_or(0), who(w["by"].as_str().unwrap_or("")), w["reason"].as_str().unwrap_or(""))).collect();
            say(&format!("## Warnings for {}\n{}", who(user), lines.join("\n")))
        }
        "clear" => {
            store::write(|s| {
                if let Some(m) = s["warns"].as_object_mut() {
                    m.remove(user);
                }
            });
            say(&format!("Cleared {}'s warnings.", who(user)))
        }
        _ => say("Use `/warnings list` or `/warnings clear`."),
    }
}

// ---------- bug list for the team ----------
fn bugs(v: &Value) -> Value {
    let want = s_opt(v, "show").unwrap_or("open");
    let Some(channel) = channel_named("bugs") else { return say("I couldn't find the bugs channel.") };
    let Some(msgs) = api(Method::GET, &format!("/channels/{channel}/messages?limit=100"), None) else { return say("I couldn't read the bugs channel.") };
    let mut lines = vec![];
    let (mut open, mut fixed) = (0, 0);
    for m in msgs.as_array().into_iter().flatten() {
        let mut parts = vec![];
        crate::help::collect_text(&m["components"], &mut parts);
        let (Some(head), Some(status)) = (parts.iter().find(|p| p.starts_with("## BUG-")), parts.iter().find(|p| p.starts_with("**Status:**"))) else { continue };
        let state = status.trim_start_matches("**Status:** ").split(" · ").next().unwrap_or("").to_string();
        let is_open = matches!(state.as_str(), "Open" | "In progress") || state.starts_with("Confirmed");
        if is_open {
            open += 1;
        }
        if state == "Fixed" {
            fixed += 1;
        }
        let wanted = match want {
            "all" => true,
            "fixed" => state == "Fixed",
            "new" => state == "Open",
            _ => is_open,
        };
        if wanted && lines.len() < 15 {
            let id = m["id"].as_str().unwrap_or("");
            lines.push(format!("[{}](https://discord.com/channels/{}/{channel}/{id}) · {state}", head.trim_start_matches("## "), guild()));
        }
    }
    reply(4, vec![text(&format!("## Bugs\n**{open}** open · **{fixed}** fixed (of the last 100 posts)")), sep(), text(&if lines.is_empty() { "Nothing to show.".to_string() } else { lines.join("\n") })], false)
}

// ---------- AFK ----------
fn afk(v: &Value) -> Value {
    let me = actor(v);
    let reason: String = s_opt(v, "reason").unwrap_or("away").chars().take(100).collect();
    store::set(&format!("afk.{me}"), json!({ "reason": reason, "since": now() }));
    say(&format!("You're AFK: {reason}. I'll tell people who mention you, and clear it when you next speak."))
}

// ---------- sticky messages ----------
fn sticky(v: &Value) -> Value {
    let (sub, inner) = sub_of(v);
    let channel = v["channel_id"].as_str().unwrap_or("").to_string();
    match sub.as_str() {
        "set" => {
            let Some(body) = s_opt(&inner, "text") else { return say("What should it say?") };
            let body: String = body.replace("\\n", "\n").chars().take(1500).collect();
            let msg = post_message(&channel, vec![text(&body)], &[]).unwrap_or_default();
            store::set(&format!("sticky.{channel}"), json!({ "text": body, "msg": msg, "at": now() }));
            say("Pinned to the bottom of this channel. It reposts itself after new messages.")
        }
        "clear" => {
            let old = store::get(&format!("sticky.{channel}.msg"));
            if let Some(id) = old.as_str() {
                let _ = call(Method::DELETE, &format!("/channels/{channel}/messages/{id}"), None, None);
            }
            store::write(|s| {
                if let Some(m) = s["sticky"].as_object_mut() {
                    m.remove(&channel);
                }
            });
            say("Removed the sticky message from this channel.")
        }
        _ => say("Use `/sticky set` or `/sticky clear`."),
    }
}

// ---------- auto-replies ----------
fn autoreply(v: &Value) -> Value {
    let (sub, inner) = sub_of(v);
    match sub.as_str() {
        "add" => {
            let Some(trigger) = s_opt(&inner, "trigger").map(|t| t.to_lowercase()) else { return say("What word or phrase should trigger it?") };
            let body = match (s_opt(&inner, "reply"), s_opt(&inner, "faq")) {
                (Some(r), _) => r.replace("\\n", "\n").chars().take(1500).collect::<String>(),
                (None, Some(k)) => match crate::help::faq_answer(k) {
                    Some(a) => a,
                    None => return say("I don't know that FAQ topic."),
                },
                _ => return say("Write a reply, or pick an FAQ answer to use."),
            };
            store::set(&format!("autoreply.{}", trigger.replace('.', "_")), json!(body));
            say(&format!("When someone says `{trigger}` I'll answer. (Not for Murmint Team members, and at most once every two minutes per channel.)"))
        }
        "remove" => {
            let Some(trigger) = s_opt(&inner, "trigger").map(|t| t.to_lowercase().replace('.', "_")) else { return say("Which trigger?") };
            let had = store::write(|s| s["autoreply"].as_object_mut().and_then(|m| m.remove(&trigger)).is_some());
            say(if had { "Removed." } else { "There's no auto-reply with that trigger." })
        }
        "list" => {
            let list: Vec<String> = store::read(|s| s["autoreply"].as_object().map(|m| m.keys().map(|k| format!("`{k}`")).collect()).unwrap_or_default());
            say(&if list.is_empty() { "No auto-replies yet.".to_string() } else { format!("**Auto-replies:** {}", list.join(" ")) })
        }
        _ => say("Use `/autoreply add`, `remove` or `list`."),
    }
}

fn matches_trigger(content: &str, trigger: &str) -> bool {
    let l = content.to_lowercase();
    let trigger = trigger.replace('_', ".");
    if trigger.contains(' ') {
        return l.contains(&trigger);
    }
    l.split(|c: char| !c.is_alphanumeric() && c != '.').any(|t| t.trim_matches('.') == trigger)
}

// ---------- gateway hooks ----------
/// Everything that reacts to a normal message: AFK, sticky messages and auto-replies.
pub fn on_message(d: &Value, team: bool, private: bool) {
    let (Some(id), Some(channel), Some(author)) = (d["id"].as_str(), d["channel_id"].as_str(), d["author"]["id"].as_str()) else { return };
    let content = d["content"].as_str().unwrap_or("");
    let t = now();
    // AFK: coming back, and people who mention someone who is away
    if let Some(a) = store::get(&format!("afk.{author}")).as_object() {
        if t > a.get("since").and_then(|s| s.as_u64()).unwrap_or(0) + 10 {
            store::write(|s| {
                if let Some(m) = s["afk"].as_object_mut() {
                    m.remove(author);
                }
            });
            crate::gateway::notice(channel, &format!("Welcome back, <@{author}>. I've cleared your AFK."));
        }
    }
    for m in d["mentions"].as_array().into_iter().flatten() {
        let Some(uid) = m["id"].as_str() else { continue };
        let rec = store::get(&format!("afk.{uid}"));
        if rec.is_object() && uid != author {
            crate::gateway::notice(channel, &format!("<@{uid}> is AFK: {} (since <t:{}:R>)", rec["reason"].as_str().unwrap_or("away"), rec["since"].as_u64().unwrap_or(t)));
        }
    }
    // auto-replies (not to the team, and not too often)
    if !team && !private {
        let found: Option<(String, String)> = store::read(|s| s["autoreply"].as_object().and_then(|m| m.iter().find(|(k, _)| matches_trigger(content, k)).map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))));
        if let Some((trigger, answer)) = found {
            static LAST: std::sync::Mutex<Option<std::collections::HashMap<String, u64>>> = std::sync::Mutex::new(None);
            let key = format!("{channel}:{trigger}");
            let ok = {
                let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
                let map = g.get_or_insert_with(Default::default);
                if map.get(&key).is_some_and(|last| t < last + 120) {
                    false
                } else {
                    map.insert(key, t);
                    true
                }
            };
            if ok {
                let body = json!({ "flags": 1 << 15, "allowed_mentions": { "parse": [], "replied_user": false }, "message_reference": { "message_id": id, "fail_if_not_exists": false },
                    "components": [container(vec![text(&answer)])] });
                crate::moderation::post_json(channel, body);
            }
        }
    }
    // sticky message: move it back to the bottom
    if let Some(s) = store::get(&format!("sticky.{channel}")).as_object() {
        if t >= s.get("at").and_then(|a| a.as_u64()).unwrap_or(0) + 4 {
            if let Some(old) = s.get("msg").and_then(|m| m.as_str()).filter(|m| !m.is_empty()) {
                let _ = call(Method::DELETE, &format!("/channels/{channel}/messages/{old}"), None, None);
            }
            let body = s.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let msg = post_message(channel, vec![text(&body)], &[]).unwrap_or_default();
            store::set(&format!("sticky.{channel}"), json!({ "text": body, "msg": msg, "at": t }));
        }
    }
}

// ---------- alpha tester welcome on any role change ----------
/// Remembers that this person has the Alpha Tester role. True the first time.
pub fn mark_alpha(user: &str) -> bool {
    store::write(|s| {
        if !s["alpha"]["seen"].is_array() {
            s["alpha"]["seen"] = json!([]);
        }
        let list = s["alpha"]["seen"].as_array_mut().unwrap();
        if list.iter().any(|x| x.as_str() == Some(user)) {
            false
        } else {
            list.push(json!(user));
            true
        }
    })
}

pub fn unmark_alpha(user: &str) {
    store::write(|s| {
        if let Some(l) = s["alpha"]["seen"].as_array_mut() {
            l.retain(|x| x.as_str() != Some(user));
        }
    });
}

/// The first time the bot runs, it notes who already has the role, so no one gets a welcome they don't need.
pub fn init_alpha_seen() {
    if store::get("alpha.seen").is_array() {
        return;
    }
    let Some(role) = role_id("Alpha Tester") else { return };
    let mut seen = vec![];
    let mut after = String::from("0");
    for _ in 0..10 {
        let Some(page) = api(Method::GET, &format!("/guilds/{}/members?limit=1000&after={after}", guild()), None) else { return };
        let list = page.as_array().cloned().unwrap_or_default();
        for m in &list {
            if m["roles"].as_array().is_some_and(|r| r.iter().any(|x| x.as_str() == Some(role.as_str()))) {
                if let Some(id) = m["user"]["id"].as_str() {
                    seen.push(json!(id));
                }
            }
        }
        match list.last().and_then(|m| m["user"]["id"].as_str()) {
            Some(last) if list.len() == 1000 => after = last.to_string(),
            _ => break,
        }
    }
    store::set("alpha.seen", json!(seen));
}

// ---------- reminders you can list and cancel ----------
fn reminders(v: &Value) -> Value {
    let (sub, inner) = sub_of(v);
    let me = actor(v);
    let mine: Vec<(usize, Value)> = store::read(|s| s["reminders"].as_array().map(|a| a.iter().cloned().enumerate().filter(|(_, r)| r["user"].as_str() == Some(me.as_str())).collect()).unwrap_or_default());
    match sub.as_str() {
        "list" => say(&if mine.is_empty() { "You have no reminders waiting.".to_string() } else { format!("## Your reminders\n{}", mine.iter().enumerate().map(|(n, (_, r))| format!("**{}.** <t:{}:R>: {}", n + 1, r["at"].as_u64().unwrap_or(0), r["text"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n")) }),
        "cancel" => {
            let n = i_opt(&inner, "number").unwrap_or(0) as usize;
            let Some((index, _)) = n.checked_sub(1).and_then(|i| mine.get(i)).cloned() else { return say("There's no reminder with that number. `/reminders list` shows them.") };
            store::write(|s| {
                if let Some(a) = s["reminders"].as_array_mut() {
                    if index < a.len() {
                        a.remove(index);
                    }
                }
            });
            say("Cancelled.")
        }
        _ => say("Use `/reminders list` or `/reminders cancel`."),
    }
}

// ---------- utilities ----------
fn timestamp(v: &Value) -> Value {
    let Some(when) = s_opt(v, "when") else { return say("Say when, like `in 2h` or `2026-12-01 18:00`.") };
    let secs = if let Some(rest) = when.strip_prefix("in ") {
        parse_duration(rest).map(|d| now() + d)
    } else {
        let w = when.replace(' ', "T");
        let w = if w.matches(':').count() == 1 { format!("{w}:00Z") } else { format!("{w}Z") };
        crate::help::parse_iso(&w)
    };
    let Some(t) = secs else { return say("I couldn't read that. Try `in 2h`, `in 3d`, or `2026-12-01 18:00` (UTC).") };
    let lines: Vec<String> = ["t", "T", "d", "D", "f", "F", "R"].iter().map(|f| format!("`<t:{t}:{f}>` shows <t:{t}:{f}>")).collect();
    say(&format!("## Timestamp\nEveryone sees this in their own time zone.\n{}", lines.join("\n")))
}

fn botinfo() -> Value {
    let up = started_at().elapsed().as_secs();
    say(&format!("## Murmint bot\n**Commands:** {}\n**Uptime:** {}h {}m since the last update\n**Saved data:** on the server's disk", crate::bot::command_total(), up / 3600, up % 3600 / 60))
}

fn started_at() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

pub fn started() {
    let _ = started_at();
}

// ---------- ticket transcript ----------
/// Saves a ticket's messages as a text file in the mod-log, if there is one. Called just before a ticket closes.
pub fn transcript(channel: &str, name: &str, closed_by: &str) {
    let Some(log) = log_channel() else { return };
    let Some(msgs) = api(Method::GET, &format!("/channels/{channel}/messages?limit=100"), None) else { return };
    let mut lines = vec![];
    for m in msgs.as_array().into_iter().flatten().rev() {
        let mut parts = vec![];
        if let Some(c) = m["content"].as_str().filter(|c| !c.is_empty()) {
            parts.push(c.to_string());
        }
        crate::help::collect_text(&m["components"], &mut parts);
        if parts.is_empty() {
            continue;
        }
        let who_said = m["author"]["username"].as_str().unwrap_or("unknown");
        let at = m["timestamp"].as_str().unwrap_or("").get(..16).unwrap_or("").replace('T', " ");
        lines.push(format!("[{at}] {who_said}: {}", parts.join(" ").replace('\n', " ")));
    }
    if lines.is_empty() {
        return;
    }
    crate::store::upload(&log, &format!("{name}.txt"), lines.join("\n").into_bytes(), &format!("Transcript of **{name}**, closed by {}", who(closed_by)));
}

// ---------- background work ----------
/// Posts announcements whose time has come and closes missions that have ended. Called every few seconds.
pub fn tick() {
    let t = now();
    let due: Vec<Value> = store::write(|s| {
        let Some(list) = s["scheduled"].as_array_mut() else { return vec![] };
        let (due, rest): (Vec<Value>, Vec<Value>) = list.drain(..).partition(|a| a["at"].as_u64().is_some_and(|x| x <= t));
        *list = rest;
        due
    });
    for a in due {
        crate::moderation::send_announcement(a["channel"].as_str().unwrap_or(""), a["title"].as_str(), a["message"].as_str().unwrap_or(""), a["ping"].as_str());
    }
    let ended: Vec<String> = store::read(|s| s["missions"].as_object().map(|m| m.iter().filter(|(_, x)| x["closed"] != json!(true) && x["ends"].as_u64().is_some_and(|e| e <= t)).map(|(k, _)| k.clone()).collect()).unwrap_or_default());
    for id in ended {
        close_mission(&id);
    }
}

// ---------- command entry ----------
pub fn run(name: &str, v: &Value) -> Value {
    let team_only = ["warnings", "mission", "bugs", "sticky", "autoreply"];
    if team_only.contains(&name) && !is_team(v) {
        return say("Only the Murmint Team can do that.");
    }
    match name {
        "warnings" => warnings(v),
        "profile" => profile(v),
        "top" => top(),
        "rewards" => rewards(v),
        "mission" => mission(v),
        "missions" => missions(),
        "bugs" => bugs(v),
        "afk" => afk(v),
        "sticky" => sticky(v),
        "autoreply" => autoreply(v),
        "reminders" => reminders(v),
        "timestamp" => timestamp(v),
        "botinfo" => botinfo(),
        _ => say("I don't know that command yet."),
    }
}

pub fn definitions() -> Vec<Value> {
    let team = |mut c: Value| {
        c["default_member_permissions"] = json!("32");
        c
    };
    let user_opt = json!({ "type": 6, "name": "user", "description": "Who (you, if left empty)" });
    let user_req = json!({ "type": 6, "name": "user", "description": "Who", "required": true });
    let level_req = json!({ "type": 4, "name": "level", "description": "The level (1 to 6)", "required": true, "min_value": 1, "max_value": 6 });
    let faq_choices: Vec<Value> = crate::help::faq_keys().into_iter().map(|(k, l)| json!({ "name": l, "value": k })).collect();
    vec![
        team(json!({ "name": "warnings", "description": "A person's warning history (team only)", "options": [
            { "type": 1, "name": "list", "description": "Show their warnings", "options": [user_req.clone()] },
            { "type": 1, "name": "clear", "description": "Clear their warnings", "options": [user_req.clone()] } ] })),
        json!({ "name": "profile", "description": "A tester's level, points and confirmed bugs", "options": [user_opt] }),
        json!({ "name": "top", "description": "The top bug hunters" }),
        json!({ "name": "rewards", "description": "Tester levels and what each one earns", "options": [
            { "type": 1, "name": "list", "description": "The levels, rewards and your progress" },
            { "type": 1, "name": "add", "description": "Set the reward for a level (team only)", "options": [
                level_req.clone(),
                { "type": 3, "name": "reward", "description": "What the tester gets, like a gift card", "required": true, "max_length": 200 },
                { "type": 8, "name": "role", "description": "A role they also get" }] },
            { "type": 1, "name": "remove", "description": "Remove a level's reward (team only)", "options": [level_req] },
            { "type": 1, "name": "pending", "description": "Rewards waiting to be sent (team only)" },
            { "type": 1, "name": "delivered", "description": "Mark someone's rewards as sent (team only)", "options": [user_req.clone()] } ] }),
        team(json!({ "name": "mission", "description": "Post a bug hunt for testers (team only)", "options": [
            { "type": 1, "name": "new", "description": "Post a bug hunt in this channel", "options": [
                { "type": 3, "name": "title", "description": "A short name", "required": true, "max_length": 80 },
                { "type": 3, "name": "steps", "description": "What to test. Use \\n for a new line", "required": true, "max_length": 1000 },
                { "type": 4, "name": "days", "description": "Close it after this many days", "min_value": 1, "max_value": 30 },
                { "type": 3, "name": "tasks", "description": "What testers can tick off, separated by semicolons (default: one per sentence)", "max_length": 900 }] },
            { "type": 1, "name": "random", "description": "Post one of the ready-made bug hunts here", "options": [
                { "type": 4, "name": "days", "description": "Close it after this many days", "min_value": 1, "max_value": 30 }] },
            { "type": 1, "name": "end", "description": "End a bug hunt now", "options": [{ "type": 3, "name": "code", "description": "Its mission code", "required": true, "max_length": 20 }] } ] })),
        json!({ "name": "missions", "description": "The bug hunts running now" }),
        team(json!({ "name": "bugs", "description": "The bug list from the bugs channel (team only)", "options": [
            { "type": 3, "name": "show", "description": "Which bugs", "choices": [{ "name": "Open and confirmed", "value": "open" }, { "name": "New, not yet looked at", "value": "new" }, { "name": "Fixed", "value": "fixed" }, { "name": "All", "value": "all" }] }] })),
        json!({ "name": "afk", "description": "Tell people you're away", "options": [{ "type": 3, "name": "reason", "description": "Why", "max_length": 100 }] }),
        team(json!({ "name": "sticky", "description": "Keep a message at the bottom of a channel (team only)", "options": [
            { "type": 1, "name": "set", "description": "Pin a message to the bottom of this channel", "options": [{ "type": 3, "name": "text", "description": "The message. Use \\n for a new line", "required": true, "max_length": 1500 }] },
            { "type": 1, "name": "clear", "description": "Remove it" } ] })),
        team(json!({ "name": "autoreply", "description": "Answer common questions automatically (team only)", "options": [
            { "type": 1, "name": "add", "description": "Reply when someone says a word or phrase", "options": [
                { "type": 3, "name": "trigger", "description": "The word or phrase", "required": true, "max_length": 40 },
                { "type": 3, "name": "reply", "description": "What to say. Use \\n for a new line", "max_length": 1500 },
                { "type": 3, "name": "faq", "description": "Or use one of the FAQ answers", "choices": faq_choices }] },
            { "type": 1, "name": "remove", "description": "Remove an auto-reply", "options": [{ "type": 3, "name": "trigger", "description": "The word or phrase", "required": true, "max_length": 40 }] },
            { "type": 1, "name": "list", "description": "Show the auto-replies" } ] })),
        json!({ "name": "reminders", "description": "See or cancel your reminders", "options": [
            { "type": 1, "name": "list", "description": "Your waiting reminders" },
            { "type": 1, "name": "cancel", "description": "Cancel one", "options": [{ "type": 4, "name": "number", "description": "Its number in /reminders list", "required": true, "min_value": 1, "max_value": 10 }] } ] }),
        json!({ "name": "timestamp", "description": "Make a time that shows in everyone's own time zone", "options": [{ "type": 3, "name": "when", "description": "Like in 2h, in 3d or 2026-12-01 18:00 (UTC)", "required": true, "max_length": 40 }] }),
        json!({ "name": "botinfo", "description": "About this bot" }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_points() {
        assert_eq!(level_for(0), 0);
        assert_eq!(level_for(1), 1);
        assert_eq!(level_for(3), 1);
        assert_eq!(level_for(4), 2);
        assert_eq!(level_for(10), 3);
        assert_eq!(level_for(74), 5);
        assert_eq!(level_for(75), 6);
        assert_eq!(level_for(10_000), 6);
        assert_eq!(level_name(3), "Bug Hunter");
        assert!(LADDER.windows(2).all(|w| w[0].2 < w[1].2 && w[0].0 + 1 == w[1].0), "the ladder only climbs");
    }

    #[test]
    fn severity_decides_the_points() {
        assert_eq!((severity_points("minor"), severity_points("major"), severity_points("critical")), (1, 3, 6));
        assert_eq!(severity_points("duplicate"), 0);
    }

    #[test]
    fn only_confirmed_bugs_earn_points_and_changes_are_fair() {
        // a report earns nothing until the team confirms it
        note_bug("T-1", "t_u1", None);
        assert_eq!(points_of("t_u1"), 0);
        assert!(triage("T-1", "t_u1", "progress").is_none());
        assert_eq!(points_of("t_u1"), 0);
        // confirmed as major: 3 points; confirming again changes nothing
        assert!(triage("T-1", "t_u1", "major").unwrap().contains("3 points"));
        assert_eq!(points_of("t_u1"), 3);
        assert!(triage("T-1", "t_u1", "major").is_none());
        // upgraded to critical: pays the difference only
        triage("T-1", "t_u1", "critical");
        assert_eq!(points_of("t_u1"), 6);
        // fixed: one bonus point, and only once
        assert!(triage("T-1", "t_u1", "fixed").unwrap().contains("bonus"));
        assert!(triage("T-1", "t_u1", "fixed").is_none());
        assert_eq!(points_of("t_u1"), 7);
        // a duplicate takes the points back
        assert!(triage("T-1", "t_u1", "duplicate").is_some());
        assert_eq!(points_of("t_u1"), 0);
        assert_eq!(store::get("contrib.t_u1.confirmed").as_u64(), Some(0));
    }

    #[test]
    fn a_mission_bug_earns_a_bonus_and_an_untriaged_fix_counts_as_minor() {
        store::set("missions.t_m", json!({ "title": "Try lists", "closed": false, "done": [], "bugs": [] }));
        assert_eq!(note_bug("T-2", "t_u2", Some("t_m")).as_deref(), Some("Try lists"));
        assert!(triage("T-2", "t_u2", "minor").unwrap().contains("3 points"), "1 for minor plus 2 for the mission");
        assert_eq!(store::get("missions.t_m.bugs").as_array().map(|a| a.len()), Some(1));
        // an unknown or closed mission code is ignored
        assert!(note_bug("T-3", "t_u3", Some("nope")).is_none());
        // fixed without triage: counted as a minor bug plus the fix bonus
        assert!(triage("T-4", "t_u4", "fixed").is_some());
        assert_eq!(points_of("t_u4"), 2);
    }

    #[test]
    fn reports_are_limited_to_five_a_day() {
        for i in 0..5 {
            assert!(can_report("t_spam"), "report {i}");
            note_bug(&format!("T-S{i}"), "t_spam", None);
        }
        assert!(!can_report("t_spam"));
        assert!(can_report("t_other"));
    }

    #[test]
    fn mission_cards_show_progress_and_close() {
        let m = json!({ "title": "Try lists", "steps": "Say a list", "subs": { "a": { "user": "1", "status": "approved" }, "b": { "user": "2", "status": "pending" }, "c": { "user": "3", "status": "rejected" } }, "bugs": ["a"], "closed": false, "ends": 1_900_000_000u64 });
        let parts = mission_parts("abc", &m);
        assert!(parts[1]["content"].as_str().unwrap().contains("mission code **abc**") && parts[1]["content"].as_str().unwrap().contains("once they approve"));
        assert!(parts[2]["content"].as_str().unwrap().contains("1 approved · 1 waiting for review · 1 bug reported"), "{}", parts[2]["content"]);
        assert_eq!(parts[3]["components"][0]["custom_id"], "mission:abc");
        assert_eq!(parts[3]["components"][0]["label"], "I finished it");
        let closed = json!({ "title": "x", "steps": "y", "subs": {}, "bugs": [], "closed": true });
        assert_eq!(mission_parts("abc", &closed).len(), 3, "no button once it has ended");
        assert!(MISSION_IDEAS.len() >= 10 && MISSION_IDEAS.iter().all(|(t, s)| t.len() <= 80 && s.len() <= 1000));
    }

    #[test]
    fn what_testers_can_tick_off_comes_from_the_steps_or_the_staff() {
        let derived = mission_tasks("Hold Ctrl + Win and say a list. Check it becomes a list. Also try saying numbered list.\nTry it in Slack", None);
        assert_eq!(derived, vec!["Hold Ctrl + Win and say a list", "Check it becomes a list", "Also try saying numbered list", "Try it in Slack"]);
        assert_eq!(mission_tasks("x", None), vec!["The whole mission"], "too little to split");
        assert_eq!(mission_tasks("ignored", Some("Open Apps; Add Discord ;; Dictate in it")), vec!["Open Apps", "Add Discord", "Dictate in it"]);
        assert!(mission_tasks(&"Do a thing here. ".repeat(40), None).len() <= 9);
        for (_, steps) in MISSION_IDEAS {
            let t = mission_tasks(steps, None);
            assert!(!t.is_empty() && t.len() <= 9 && t.iter().all(|x| x.chars().count() <= 100), "{steps}");
        }
    }

    #[test]
    fn the_form_has_a_menu_an_upload_and_a_note_and_fits_discords_limits() {
        let m = json!({ "title": "A mission with a rather long title that goes past the limit", "steps": "Open Apps. Add Discord. Dictate in it.", "subs": {}, "closed": false });
        let f = proof_form("abc", &m);
        assert_eq!(f["type"], 9);
        assert_eq!(f["data"]["custom_id"], "mission_proof:abc");
        assert!(f["data"]["title"].as_str().unwrap().chars().count() <= 45);
        let rows = f["data"]["components"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r["type"] == 18 && r["label"].as_str().unwrap().chars().count() <= 45 && r["description"].as_str().map_or(true, |d| d.chars().count() <= 100)));
        let menu = &rows[0]["component"];
        assert_eq!(menu["type"], 3);
        let opts = menu["options"].as_array().unwrap();
        assert_eq!(opts.len(), 4, "three tasks and 'something else'");
        assert_eq!(opts[0]["value"], "t0");
        assert_eq!(opts[3]["value"], "other");
        assert_eq!(menu["min_values"], 1);
        assert_eq!(menu["max_values"], 4);
        assert_eq!(rows[1]["component"]["type"], 19, "an upload");
        assert_eq!(rows[1]["component"]["min_values"], 1);
        assert_eq!(rows[2]["component"]["type"], 4);
        assert_eq!(rows[2]["component"]["required"], false);
    }

    fn sent(tasks: Value, files: Value, notes: &str) -> Value {
        json!({ "type": 5, "member": { "roles": [], "user": { "id": "55" } }, "data": {
            "custom_id": "mission_proof:M1",
            "components": [
                { "type": 18, "id": 1, "component": { "type": 3, "id": 2, "custom_id": "done", "values": tasks } },
                { "type": 18, "id": 3, "component": { "type": 19, "id": 4, "custom_id": "proof", "values": files.as_array().map(|a| a.iter().map(|f| f["id"].clone()).collect::<Vec<_>>()).unwrap_or_default() } },
                { "type": 18, "id": 5, "component": { "type": 4, "id": 6, "custom_id": "notes", "value": notes } },
            ],
            "resolved": { "attachments": files.as_array().map(|a| a.iter().map(|f| (f["id"].as_str().unwrap().to_string(), f.clone())).collect::<serde_json::Map<_, _>>()).unwrap_or_default() },
        } })
    }

    #[test]
    fn a_filled_in_form_is_read_into_a_proof_and_only_media_counts() {
        let m = json!({ "title": "T", "steps": "Open Apps. Add Discord. Dictate in it.", "closed": false });
        let png = json!({ "id": "9", "filename": "shot.png", "content_type": "image/png", "size": 1000, "url": "https://cdn.discordapp.com/attachments/1/2/shot.png?ex=1" });
        let p = parse_proof(&sent(json!(["t0", "t2", "other"]), json!([png]), "it worked"), &m).unwrap();
        assert_eq!(p.tasks, vec!["Open Apps", "Dictate in it", "Something else"]);
        assert_eq!(p.files[0].0, "shot.png");
        assert_eq!(p.notes, "it worked");
        // no menu choice, no file, or a file that isn't an image or recording are all turned away
        assert!(parse_proof(&sent(json!([]), json!([png]), ""), &m).unwrap_err().contains("what you completed"));
        assert!(parse_proof(&sent(json!(["t0"]), json!([]), ""), &m).unwrap_err().contains("screenshot"));
        let exe = json!({ "id": "8", "filename": "virus.exe", "content_type": "application/x-msdownload", "url": "https://cdn.discordapp.com/attachments/1/2/virus.exe" });
        assert!(parse_proof(&sent(json!(["t0"]), json!([exe]), ""), &m).unwrap_err().contains("image or a screen recording"));
        let mp4 = json!({ "id": "7", "filename": "clip.mp4", "content_type": "video/mp4", "url": "https://cdn.discordapp.com/attachments/1/2/clip.mp4" });
        assert!(parse_proof(&sent(json!(["t1"]), json!([mp4]), ""), &m).is_ok());
        // a choice that doesn't exist on the menu is ignored, not trusted
        assert!(parse_proof(&sent(json!(["t99", "zzz"]), json!([png]), ""), &m).is_err());
        // files are only fetched from Discord's own servers
        assert!(fetch_file("https://evil.example/shot.png").is_none() && fetch_file("http://cdn.discordapp.com/x").is_none());
    }

    fn staff(action_type: u64) -> Value {
        json!({ "type": action_type, "member": { "roles": [], "user": { "id": crate::bot::owner() } }, "data": {},
            "message": { "components": [{ "type": 17, "components": [{ "type": 10, "content": "## Mission proof" }, { "type": 1, "components": [{ "type": 2 }] }] }] } })
    }

    #[test]
    fn proof_is_reviewed_by_staff_and_points_follow_only_approval() {
        store::set("missions.MR1", json!({ "title": "Review me", "steps": "Do a thing here.", "closed": false, "subs": {
            "S1": { "user": "t_rev1", "status": "pending", "tasks": ["x"] }, "S2": { "user": "t_rev2", "status": "pending", "tasks": ["x"] } } }));
        // a tester can't approve their own proof
        let tester = json!({ "type": 3, "member": { "roles": [], "user": { "id": "t_rev1" } }, "data": {}, "message": { "components": [{}] } });
        assert!(review_click(&tester, "ok", "MR1:S1")["data"]["components"][0]["components"][0]["content"].as_str().unwrap().contains("Only staff"));
        assert_eq!(points_of("t_rev1"), 0);
        assert_eq!(store::get("missions.MR1.subs.S1.status"), "pending");
        // staff approve: the card loses its buttons, the tester gets the points once
        let done = review_click(&staff(3), "ok", "MR1:S1");
        assert_eq!(done["type"], 7);
        let parts = done["data"]["components"][0]["components"].as_array().unwrap();
        assert!(!parts.iter().any(|p| p["type"] == 1), "no buttons left");
        assert!(parts.last().unwrap()["content"].as_str().unwrap().starts_with("**Approved** by"));
        assert_eq!(points_of("t_rev1"), 2);
        assert_eq!(store::get("missions.MR1.subs.S1.status"), "approved");
        assert!(review_click(&staff(3), "ok", "MR1:S1")["data"]["components"][0]["components"][0]["content"].as_str().unwrap().contains("already reviewed"));
        assert_eq!(points_of("t_rev1"), 2, "a second click doesn't pay again");
        // staff reject: Reject first asks for a reason, then records it, and no points are given
        let ask = review_click(&staff(3), "no", "MR1:S2");
        assert_eq!(ask["type"], 9);
        assert_eq!(ask["data"]["custom_id"], "msnno:MR1:S2");
        let mut submit = staff(5);
        submit["data"] = json!({ "custom_id": "msnno:MR1:S2", "components": [{ "type": 18, "component": { "type": 4, "custom_id": "reason", "value": "The screenshot is blurry" } }] });
        assert!(modal_submit(&submit)["data"]["components"][0]["components"][0]["content"].as_str().unwrap().contains("Rejected"));
        assert_eq!(store::get("missions.MR1.subs.S2.status"), "rejected");
        assert_eq!(store::get("missions.MR1.subs.S2.reason"), "The screenshot is blurry");
        assert_eq!(points_of("t_rev2"), 0);
    }

    #[test]
    fn administrators_count_as_staff_and_one_proof_is_allowed_at_a_time() {
        let admin = json!({ "member": { "roles": [], "permissions": "8", "user": { "id": "somebody" } } });
        assert!(is_staff(&admin));
        assert!(!is_staff(&json!({ "member": { "roles": [], "permissions": "2048", "user": { "id": "somebody" } } })));
        store::set("missions.MR2", json!({ "title": "Once", "steps": "Do a thing here.", "closed": false, "subs": {} }));
        let who_sent = |status: &str| {
            store::set("missions.MR2.subs", json!({ "S9": { "user": "55", "status": status } }));
            mission_form(&json!({ "member": { "user": { "id": "55" } } }), "MR2")
        };
        assert_eq!(who_sent("pending")["type"], 4, "waiting for review: no second form");
        assert_eq!(who_sent("approved")["type"], 4, "approved: nothing more to send");
        assert_eq!(who_sent("rejected")["type"], 9, "rejected: they may send new proof");
        store::set("missions.MR2.closed", json!(true));
        assert_eq!(mission_form(&json!({ "member": { "user": { "id": "56" } } }), "MR2")["type"], 4, "an ended mission has no form");
    }

    #[test]
    fn rewards_show_the_ladder_to_everyone_and_settings_only_to_the_team() {
        let r = run("rewards", &json!({ "data": { "options": [{ "name": "list" }] }, "member": { "roles": [], "user": { "id": "7" } } }));
        let mut parts = vec![];
        crate::help::collect_text(&r["data"]["components"], &mut parts);
        let shown = parts.join("\n");
        assert!(shown.contains("Level 3: Bug Hunter") && shown.contains("Only bugs the team confirms"), "{shown}");
        for sub in ["add", "remove", "pending", "delivered"] {
            let r = run("rewards", &json!({ "data": { "options": [{ "name": sub, "options": [] }] }, "member": { "roles": [], "user": { "id": "7" } } }));
            assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().starts_with("Only the Murmint Team"), "{sub}");
        }
    }

    #[test]
    fn reaching_a_level_with_a_reward_creates_a_pending_claim() {
        store::set("rewards.tiers.2", json!({ "text": "A test reward" }));
        adjust("t_winner", 4, "confirmed", 1);
        let claims = store::get("rewards.claims");
        assert!(claims.as_array().unwrap().iter().any(|c| c["user"] == "t_winner" && c["level"] == 2 && c["text"] == "A test reward" && c["done"] == false));
    }

    #[test]
    fn triaging_and_marking_delivered_are_team_only() {
        let click = json!({ "type": 3, "data": { "custom_id": "reward:1:2" }, "member": { "roles": [], "user": { "id": "5" } }, "message": { "components": [{}] } });
        let r = reward_delivered(&click, "1", "2");
        assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().starts_with("Only the Murmint Team"));
    }

    #[test]
    fn triggers_match_words_not_parts_of_words() {
        assert!(matches_trigger("Why does SmartScreen block it?", "smartscreen"));
        assert!(matches_trigger("windows protected your pc!", "windows protected your pc"));
        assert!(matches_trigger("is it murmint.app?", "murmint_app"));
        assert!(!matches_trigger("smartscreens are annoying", "smartscreen"));
        assert!(!matches_trigger("hello", "bye"));
    }

    #[test]
    fn warnings_are_counted_per_person() {
        assert_eq!(record_warning("t_a", "9", "spam"), 1);
        assert_eq!(record_warning("t_a", "9", "spam again"), 2);
        assert_eq!(record_warning("t_b", "9", "once"), 1);
    }

    #[test]
    fn idea_votes_relabel_the_buttons() {
        let card = json!({ "type": 17, "components": [{ "type": 10, "content": "x" }, idea_vote_row("abc", 0, 0), idea_status_row()] });
        let c = relabel_votes(&card, "abc", 3, 1);
        assert_eq!(c["components"][1]["components"][0]["label"], "\u{25B2} 3");
        assert_eq!(c["components"][1]["components"][1]["label"], "\u{25BC} 1");
        assert_eq!(c["components"][2]["components"][0]["custom_id"], "idea:considering", "the status row is untouched");
        assert_eq!(bar(5, 10, 10), "`█████░░░░░`");
        assert_eq!(bar(0, 0, 4), "`░░░░`");
    }

    #[test]
    fn timestamps_are_read_from_plain_words_and_dates() {
        let r = timestamp(&json!({ "data": { "options": [{ "name": "when", "value": "2026-12-01 18:00" }] } }));
        let said = r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().to_string();
        assert!(said.contains("<t:1796148000:F>"), "{said}");
        let r = timestamp(&json!({ "data": { "options": [{ "name": "when", "value": "someday" }] } }));
        assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().contains("couldn't read"));
    }

    #[test]
    fn team_tools_are_refused_for_everyone_else() {
        for name in ["warnings", "mission", "bugs", "sticky", "autoreply"] {
            let r = run(name, &json!({ "data": { "options": [] }, "member": { "roles": [], "user": { "id": "42" } } }));
            assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().starts_with("Only the Murmint Team"), "{name}");
        }
    }
}
