//! /acknowledgements: thanks the staff and the alpha testers, with their pictures and names (contributors have their own page).
//! Built live from Discord (roles and the staff access channel), refreshed every ten minutes.

use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("DiscordBot (https://murmint.app, 1.0)")
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

struct Person {
    name: String,
    avatar: String,
    link: String,
    detail: String,
}

fn discord_avatar(user: &Value) -> String {
    let id = user["id"].as_str().unwrap_or("0");
    match user["avatar"].as_str() {
        Some(h) => format!(
            "https://cdn.discordapp.com/avatars/{id}/{h}.{}?size=128",
            if h.starts_with("a_") { "gif" } else { "png" }
        ),
        None => format!(
            "https://cdn.discordapp.com/embed/avatars/{}.png",
            id.parse::<u64>().map(|n| (n >> 22) % 6).unwrap_or(0)
        ),
    }
}
fn discord_person(user: &Value, nick: Option<&str>, detail: &str) -> Person {
    let name =
        nick.or(user["global_name"].as_str()).or(user["username"].as_str()).unwrap_or("Someone").to_string();
    Person {
        name,
        avatar: discord_avatar(user),
        link: format!("https://discord.com/users/{}", user["id"].as_str().unwrap_or("")),
        detail: detail.to_string(),
    }
}

/// (staff, alpha testers) from the Discord server
fn from_discord() -> (Vec<Person>, Vec<Person>) {
    let (Some(bot), Some(guild)) = (env("DISCORD_BOT_TOKEN"), env("DISCORD_GUILD_ID")) else {
        return (vec![], vec![]);
    };
    let c = client();
    let get = |path: &str| -> Option<Value> {
        c.get(format!("https://discord.com/api/v10{path}"))
            .header("Authorization", format!("Bot {bot}"))
            .send()
            .ok()?
            .json()
            .ok()
    };
    let roles = get(&format!("/guilds/{guild}/roles")).unwrap_or_default();
    let role_id = |n: &str| {
        roles
            .as_array()
            .and_then(|a| a.iter().find(|r| r["name"].as_str() == Some(n)))
            .and_then(|r| r["id"].as_str())
            .map(String::from)
    };
    let (team, alpha) =
        (role_id("Murmint Team"), role_id(&env("ALPHA_ROLE").unwrap_or_else(|| "Alpha Tester".into())));
    let has = |m: &Value, r: &Option<String>| {
        r.as_ref().is_some_and(|r| {
            m["roles"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(r.as_str())))
        })
    };

    // everyone in the server (up to a few thousand)
    let mut members: Vec<Value> = vec![];
    let mut after = "0".to_string();
    for _ in 0..5 {
        let Some(page) = get(&format!("/guilds/{guild}/members?limit=1000&after={after}")) else { break };
        let Some(arr) = page.as_array() else { break };
        if arr.is_empty() {
            break;
        }
        after = arr.last().and_then(|m| m["user"]["id"].as_str()).unwrap_or("0").to_string();
        members.extend(arr.iter().cloned());
        if arr.len() < 1000 {
            break;
        }
    }
    let owner = crate::auth::owner();
    let listed = crate::auth::listed_ids();

    let (mut staff, mut testers) = (BTreeMap::new(), BTreeMap::new());
    for m in &members {
        let u = &m["user"];
        if u["bot"].as_bool() == Some(true) {
            continue;
        }
        let id = u["id"].as_str().unwrap_or("").to_string();
        let nick = m["nick"].as_str();
        if id == owner || has(m, &team) {
            staff.insert(
                id.clone(),
                discord_person(u, nick, if id == owner { "Creator" } else { "Murmint Team" }),
            );
        } else if has(m, &alpha) || listed.contains(&id) {
            testers.insert(id.clone(), discord_person(u, nick, "Alpha Tester"));
        }
    }
    // people listed in the access channel who have not joined the server yet
    let missing: Vec<String> =
        listed.iter().filter(|id| !staff.contains_key(*id) && !testers.contains_key(*id)).cloned().collect();
    for id in missing {
        if let Some(u) = get(&format!("/users/{id}")) {
            if u["id"].is_string() {
                testers.insert(id.clone(), discord_person(&u, None, "Alpha Tester"));
            }
        }
    }
    (staff.into_values().collect(), testers.into_values().collect())
}

fn cards(people: &[Person], empty: &str) -> String {
    if people.is_empty() {
        return format!("<div class=\"ack-empty\">{}</div>", esc(empty));
    }
    let items: String = people.iter().map(|p| format!(
        "<a class=\"ack-card\" href=\"{}\" rel=\"noopener\"><img src=\"{}\" alt=\"\" width=\"40\" height=\"40\"><span class=\"who\"><b>{}</b><small>{}</small></span></a>",
        esc(&p.link), esc(&p.avatar), esc(&p.name), esc(&p.detail)
    )).collect();
    format!("<div class=\"ack-grid\">{items}</div>")
}

/// The page body (cached for ten minutes).
pub fn body() -> String {
    if let Some((at, html)) = CACHE.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return html.clone();
        }
    }
    let (staff, testers) = from_discord();
    let html = format!(
        r#"<section class="page-top wrap">
  <h1><span class="dim">Thank you</span> for building this with us.</h1>
  <p>Murmint exists because of the people who test it, run it and help write it.</p>
</section>

<section class="block wrap ack" style="padding-top:60px">
  <div class="ack-group"><h2>Staff <span>{ns}</span></h2><p>The people who build Murmint and run the community.</p>{staff}</div>
  <div class="ack-group"><h2>Alpha testers <span>{nt}</span></h2><p>Early testers who try every build, find the bugs and tell us what to fix.</p>{testers}</div>
</section>

<section class="wrap">
  <div class="final">
    <h2>Want to <span class="dim">join them?</span></h2>
    <p>Apply for the closed alpha and help shape Murmint.</p>
    <div class="actions"><a class="btn primary" href="/auth/discord">Join with Discord</a><a class="btn ghost" href="https://discord.gg/MEfdzfyARV">Join the Discord</a></div>
  </div>
</section>
"#,
        ns = staff.len(),
        nt = testers.len(),
        staff = cards(&staff, "Names for this section will be listed here."),
        testers = cards(&testers, "Names for this section will be listed here."),
    );
    *CACHE.lock().unwrap() = Some((Instant::now(), html.clone()));
    html
}
