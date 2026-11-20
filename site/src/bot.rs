//! The Murmint Discord bot, built into the website server (one service, no gateway connection).
//!
//! Slash commands arrive as signed HTTPS requests at /interactions. On start-up the server:
//!   - registers the commands in the Murmint server,
//!   - points the application's Interactions Endpoint URL at <PUBLIC_URL>/interactions (when the site is public),
//!   - and every two minutes gives the Alpha Tester role to anyone listed in the staff access channel.
//!
//! Commands: /help /apply /download /links /status, and for the Murmint Team: /grant /revoke.
//! Uses the same settings as Discord sign-in: DISCORD_CLIENT_ID, DISCORD_BOT_TOKEN, DISCORD_GUILD_ID, DISCORD_ACCESS_CHANNEL.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub(crate) const ORANGE: u32 = 0xff6a1f;
const FORM: &str = "https://tally.so/r/Qoe4y7";
const INVITE: &str = "https://discord.gg/MEfdzfyARV";
const EPHEMERAL: u64 = 1 << 6;
const COMPONENTS_V2: u64 = 1 << 15;

pub(crate) fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}
pub(crate) fn site() -> String {
    env("PUBLIC_URL")
        .filter(|u| !u.contains("YOUR-SITE"))
        .unwrap_or_else(|| "https://murmint.app".into())
        .trim_end_matches('/')
        .to_string()
}
fn bot_token() -> Option<String> {
    env("DISCORD_BOT_TOKEN")
}
pub(crate) fn api(method: reqwest::Method, path: &str, body: Option<Value>) -> Option<Value> {
    let token = bot_token()?;
    let mut req = crate::auth::http()
        .request(method, format!("https://discord.com/api/v10{path}"))
        .header("Authorization", format!("Bot {token}"));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().ok()?;
    if !r.status().is_success() {
        eprintln!("[bot] {path}: {}", r.status());
        return None;
    }
    Some(r.json().unwrap_or(Value::Null))
}

/// Like `api`, but returns the HTTP status when Discord refuses, and can attach an audit-log reason.
pub(crate) fn call(method: reqwest::Method, path: &str, body: Option<Value>, reason: Option<&str>) -> Result<Value, u16> {
    let token = bot_token().ok_or(0u16)?;
    let mut req = crate::auth::http()
        .request(method, format!("https://discord.com/api/v10{path}"))
        .header("Authorization", format!("Bot {token}"));
    if let Some(r) = reason {
        let encoded: String = r.bytes().map(|b| if b.is_ascii_alphanumeric() || b == b' ' { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        req = req.header("X-Audit-Log-Reason", encoded.replace(' ', "%20"));
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    let r = req.send().map_err(|_| 0u16)?;
    if !r.status().is_success() {
        eprintln!("[bot] {path}: {}", r.status());
        return Err(r.status().as_u16());
    }
    Ok(r.json().unwrap_or(Value::Null))
}

/// The application's public key, used to check that a request really came from Discord.
fn verify_key() -> Option<&'static VerifyingKey> {
    static KEY: OnceLock<Option<VerifyingKey>> = OnceLock::new();
    KEY.get_or_init(|| {
        let hex_key = env("DISCORD_PUBLIC_KEY").or_else(|| {
            api(reqwest::Method::GET, "/applications/@me", None)?["verify_key"].as_str().map(String::from)
        })?;
        let bytes: [u8; 32] = hex::decode(hex_key.trim()).ok()?.try_into().ok()?;
        VerifyingKey::from_bytes(&bytes).ok()
    })
    .as_ref()
}

pub fn verified(key: &VerifyingKey, body: &[u8], sig_hex: &str, stamp: &str) -> bool {
    let Ok(sig_bytes) = hex::decode(sig_hex) else { return false };
    let Ok(sig_arr): Result<[u8; 64], _> = sig_bytes.try_into() else { return false };
    let mut msg = stamp.as_bytes().to_vec();
    msg.extend_from_slice(body);
    key.verify(&msg, &Signature::from_bytes(&sig_arr)).is_ok()
}

/// Handles one interaction request. Returns (HTTP status, JSON body).
pub fn interaction(body: &[u8], sig: &str, stamp: &str) -> (&'static str, String) {
    let Some(key) = verify_key() else {
        return ("503 Service Unavailable", r#"{"error":"bot not configured"}"#.into());
    };
    if !verified(key, body, sig, stamp) {
        return ("401 Unauthorized", r#"{"error":"invalid request signature"}"#.into());
    }
    let Ok(v) = serde_json::from_slice::<Value>(body) else { return ("400 Bad Request", "{}".into()) };
    ("200 OK", respond(&v).to_string())
}

// ---- replies, in the same Components v2 card style as the server's posts ----
pub(crate) fn text(s: &str) -> Value {
    json!({ "type": 10, "content": s })
}
pub(crate) fn sep() -> Value {
    json!({ "type": 14, "divider": true, "spacing": 1 })
}
pub(crate) fn link(label: &str, url: &str) -> Value {
    json!({ "type": 2, "style": 5, "label": label, "url": url })
}
pub(crate) fn row(buttons: Vec<Value>) -> Value {
    json!({ "type": 1, "components": buttons })
}
pub(crate) fn card(parts: Vec<Value>, public: bool) -> Value {
    json!({ "type": 4, "data": { "flags": COMPONENTS_V2 | if public { 0 } else { EPHEMERAL }, "allowed_mentions": { "parse": [] }, "components": [{ "type": 17, "accent_color": ORANGE, "components": parts }] } })
}
pub(crate) fn say(s: &str) -> Value {
    card(vec![text(s)], false)
}

// ---- the site as a Components v2 link card: a section with the logo, a banner image, and link buttons ----
/// Discord builds its own preview for a pasted link, and that one can't use components. This is the card the bot can post
/// itself: the logo beside a title and text, the preview banner under it, and buttons that open the pages.
pub(crate) fn site_card(title: &str, body: &str, buttons: Vec<Value>, banner: bool, public: bool) -> Value {
    let base = site();
    let mut parts = vec![json!({
        "type": 9,
        "components": [text(&format!("## {title}\n{body}"))],
        "accessory": { "type": 11, "media": { "url": format!("{base}/assets/logo.png") }, "description": "Murmint" }
    })];
    if banner {
        parts.push(json!({ "type": 12, "items": [{ "media": { "url": format!("{base}/assets/social.png") }, "description": "Murmint: speak, and it types. AI voice typing for Windows." }] }));
    }
    parts.push(sep());
    parts.push(row(buttons));
    card(parts, public)
}

pub(crate) fn option<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    v["data"]["options"].as_array()?.iter().find(|o| o["name"].as_str() == Some(name)).map(|o| &o["value"])
}

pub fn respond(v: &Value) -> Value {
    match v["type"].as_u64() {
        Some(1) => return json!({ "type": 1 }), // PING from Discord when the endpoint is set up
        Some(2) => {}
        Some(3) if v["data"]["custom_id"].as_str().is_some_and(|c| c.starts_with("guide:")) => return crate::guide::answer(v), // the #guide menus
        Some(3) => return crate::features::component(v), // a button on a poll, giveaway, ticket or role panel
        Some(5) => {
            // a submitted form: a mission's proof (or a rejection reason), or a /bug or /feedback form
            let cid = v["data"]["custom_id"].as_str().unwrap_or("");
            return if cid.starts_with("mission_proof:") || cid.starts_with("msnno:") { crate::community::modal_submit(v) } else { crate::moderation::submitted(v) };
        }
        _ => return say("This button or menu is not supported yet."),
    }
    let s = site();
    let roles: Vec<String> = v["member"]["roles"]
        .as_array()
        .map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let is_team = || {
        role_id("Murmint Team").is_some_and(|r| roles.contains(&r))
            || v["member"]["user"]["id"].as_str() == Some(&owner())
    };
    match v["data"]["name"].as_str().unwrap_or("") {
        "help" => card(vec![
            text("## Murmint bot\nHere's what I can do:"),
            sep(),
            text("`/apply` Join the closed alpha\n`/download` Get the installer\n`/links` Every Murmint link\n`/status` The latest version and where things stand\n`/bug` Report a bug\n`/feedback` Share an idea or a problem"),
            text("`/poll` Ask a question with voting buttons\n`/remind` Get a reminder later\n`/tag` Saved answers\n`/userinfo` `/avatar` `/serverinfo`\n`/roll` `/coinflip` `/8ball` `/choose` Small games\n`/afk` · `/reminders` · `/timestamp`"),
            text("**Alpha testers:** `/bug` `/feedback` (others vote on ideas) `/rate` `/missions` (bug hunts) `/profile` `/top` `/rewards` `/testerguide` and `/guide` `/faq` `/troubleshoot` `/keys` `/changelog`"),
            text("-# Team only: `/grant` `/revoke` `/alpha` for the alpha, or right-click a person, then Apps, then **Grant alpha**. Moderation: `/warn` `/timeout` `/untimeout` `/kick` `/ban` `/unban` `/purge` `/slowmode` `/lock` `/unlock` `/whois`, and `/announce` (can ping a role or post later). Records: `/warnings` `/mission` `/rewards add` `/bugs` `/sticky` `/autoreply`. Community: `/giveaway` `/ticketpanel` `/rolepanel` `/welcome` `/autorole` `/automod`."),
        ], false),
        "apply" => site_card("Join the closed alpha", "Fill in the short form. Once you're accepted you get the **Alpha Tester** role, the alpha channels unlock, and you can download Murmint.", vec![link("Apply for the alpha", FORM), link("Sign in on the website", &format!("{s}/auth/discord"))], false, false),
        "download" => site_card("Download Murmint", "Alpha testers sign in on the website with Discord, and the download starts there. Not a tester yet? Use `/apply`.", vec![link("Sign in and download", &format!("{s}/auth/discord")), link("Download page", &format!("{s}/download"))], false, false),
        "links" | "website" => site_card(
            "Murmint",
            "**Speak. It types.** AI voice typing for Windows: hold a shortcut, talk, and clean text appears wherever your cursor is. Bring your own AI, or run a model on your own PC.",
            vec![link("Website", &format!("{s}/")), link("Features", &format!("{s}/features")), link("How it works", &format!("{s}/how-it-works")), link("FAQ", &format!("{s}/faq")), link("Join the Discord", INVITE)],
            true,
            v["data"]["name"].as_str() == Some("website"),
        ),
        "status" => {
            let version = latest_version().unwrap_or_else(|| "not published yet".into());
            site_card("Murmint status", &format!("**Stage:** closed alpha\n**Latest version:** {version}\n**Website:** {s}"), vec![link("Website", &format!("{s}/")), link("Roadmap and updates", INVITE)], false, false)
        }
        "grant" | "revoke" => {
            if !is_team() { return say("Only the Murmint Team can change alpha access."); }
            let grant = v["data"]["name"].as_str() == Some("grant");
            let users: Vec<&str> = ["user", "user2", "user3", "user4", "user5"].iter().filter_map(|n| option(v, n).and_then(|u| u.as_str())).collect();
            if users.is_empty() { return say("Pick a user."); }
            let Some(alpha) = role_id("Alpha Tester") else { return say("I can't find the **Alpha Tester** role."); };
            let guild = env("DISCORD_GUILD_ID").unwrap_or_default();
            let (mut done, mut failed) = (vec![], vec![]);
            for user in users {
                let path = format!("/guilds/{guild}/members/{user}/roles/{alpha}");
                let method = if grant { reqwest::Method::PUT } else { reqwest::Method::DELETE };
                if api(method, &path, None).is_some() {
                    if grant { record_access(user); crate::moderation::welcome(user); } else { crate::community::unmark_alpha(user); }
                    done.push(format!("<@{user}>"));
                } else {
                    failed.push(format!("<@{user}>"));
                }
            }
            let mut msg = String::new();
            if !done.is_empty() { msg += &format!("{} {} {} the closed alpha.", if grant { "Added" } else { "Removed" }, done.join(" "), if grant { "to" } else { "from" }); }
            if !failed.is_empty() { msg += &format!("{}That didn't work for {}. Is the person in the server, and is my role above Alpha Tester?", if msg.is_empty() { "" } else { "\n" }, failed.join(" ")); }
            say(&msg)
        }
        "bug" | "feedback" => crate::moderation::form(v["data"]["name"].as_str().unwrap_or("feedback")),
        "kick" | "ban" | "unban" | "timeout" | "untimeout" | "warn" | "purge" | "slowmode" | "lock" | "unlock" | "whois" | "announce" | "alpha" => {
            if !is_team() { return say("Only the Murmint Team can do that."); }
            crate::moderation::run(v["data"]["name"].as_str().unwrap_or(""), v)
        }
        n if crate::features::NAMES.contains(&n) => crate::features::run(n, v),
        n if crate::help::NAMES.contains(&n) => crate::help::run(n, v),
        n if crate::community::NAMES.contains(&n) => crate::community::run(n, v),
        "Grant alpha" | "Revoke alpha" => {
            if !is_team() { return say("Only the Murmint Team can change alpha access."); }
            crate::moderation::alpha_toggle(v, v["data"]["name"].as_str() == Some("Grant alpha"))
        }
        _ => say("I don't know that command yet."),
    }
}

pub(crate) fn owner() -> String {
    env("DISCORD_OWNER_ID").unwrap_or_else(|| "1176652779973001256".into())
}

pub(crate) fn role_id(name: &str) -> Option<String> {
    let guild = env("DISCORD_GUILD_ID")?;
    let roles = api(reqwest::Method::GET, &format!("/guilds/{guild}/roles"), None)?;
    roles.as_array()?.iter().find(|r| r["name"].as_str() == Some(name))?["id"].as_str().map(String::from)
}

/// /grant also writes the person's ID into the access channel, so website sign-in and the acknowledgements page know them.
pub(crate) fn record_access(user: &str) {
    let Some(channel) = env("DISCORD_ACCESS_CHANNEL") else { return };
    let guild = env("DISCORD_GUILD_ID").unwrap_or_default();
    let (Some(token), true) = (bot_token(), !guild.is_empty()) else { return };
    if crate::auth::access_list(&crate::auth::http(), &guild, &token).iter().any(|id| id == user) {
        return;
    }
    let _ = api(
        reqwest::Method::POST,
        &format!("/channels/{channel}/messages"),
        Some(json!({ "content": user })),
    );
}

fn latest_version() -> Option<String> {
    let token = env("GITHUB_TOKEN")?;
    let repo = env("RELEASE_REPO").unwrap_or_else(|| "proimpacts/murmint".into());
    let r: Value = crate::auth::http()
        .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .bearer_auth(token)
        .header("User-Agent", "murmint-site")
        .send()
        .ok()?
        .json()
        .ok()?;
    r["tag_name"].as_str().map(|t| t.trim_start_matches('v').to_string())
}

pub(crate) fn command_total() -> usize {
    commands().as_array().map(|a| a.len()).unwrap_or(0)
}

fn commands() -> Value {
    let people = |d: &str| {
        let mut o = vec![json!({ "type": 6, "name": "user", "description": d, "required": true })];
        for n in 2..=5 {
            o.push(json!({ "type": 6, "name": format!("user{n}"), "description": "Another person (optional)" }));
        }
        o
    };
    let mut all = vec![
        json!({ "name": "help", "description": "What the Murmint bot can do" }),
        json!({ "name": "apply", "description": "Join the Murmint closed alpha" }),
        json!({ "name": "download", "description": "Get the Murmint installer" }),
        json!({ "name": "links", "description": "Every Murmint link in one place" }),
        json!({ "name": "website", "description": "Post the Murmint website card for everyone in this channel" }),
        json!({ "name": "status", "description": "The latest Murmint version and where things stand" }),
        json!({ "name": "grant", "description": "Give up to five people alpha access (team only)", "default_member_permissions": "32", "options": people("Who gets access") }),
        json!({ "name": "revoke", "description": "Remove up to five people's alpha access (team only)", "default_member_permissions": "32", "options": people("Who loses access") }),
    ];
    all.extend(crate::moderation::definitions());
    all.extend(crate::features::definitions());
    all.extend(crate::help::definitions());
    all.extend(crate::community::definitions());
    Value::Array(all)
}

#[cfg(test)]
pub(crate) fn commands_for_test() -> Vec<Value> {
    commands().as_array().cloned().unwrap_or_default()
}

/// Start the bot's background work: register commands, set the interactions URL, and keep roles in sync.
pub fn start() {
    if bot_token().is_none() || env("DISCORD_GUILD_ID").is_none() {
        return;
    }
    std::thread::spawn(|| {
        let (Some(app), Some(guild)) = (env("DISCORD_CLIENT_ID"), env("DISCORD_GUILD_ID")) else { return };
        // commands in the Murmint server (show up instantly, unlike global commands)
        if api(
            reqwest::Method::PUT,
            &format!("/applications/{app}/guilds/{guild}/commands"),
            Some(commands()),
        )
        .is_some()
        {
            println!("[bot] slash commands registered");
        }
        // on the public site, point Discord at this server for slash commands (Discord pings the URL first, so wait until it answers)
        let url = format!("{}/interactions", site());
        if url.starts_with("https://") {
            for attempt in 0..10 {
                std::thread::sleep(Duration::from_secs(if attempt == 0 { 20 } else { 30 }));
                let current = api(reqwest::Method::GET, "/applications/@me", None);
                if current.as_ref().and_then(|a| a["interactions_endpoint_url"].as_str())
                    == Some(url.as_str())
                {
                    println!("[bot] interactions URL already set");
                    break;
                }
                if api(
                    reqwest::Method::PATCH,
                    "/applications/@me",
                    Some(json!({ "interactions_endpoint_url": url })),
                )
                .is_some()
                {
                    println!("[bot] interactions URL set to {url}");
                    break;
                }
            }
        }
    });
    // one-time tidy-up of the hidden channel the bot used to keep its data in
    std::thread::spawn(crate::store::remove_old_data_channel);
    crate::community::started();
    std::thread::spawn(crate::community::init_alpha_seen);
    // giveaways end and reminders are sent when they are due
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(5));
        crate::features::tick();
    });
    // the live connection: welcome messages, auto-mod and the log
    crate::gateway::start();
    // anyone listed in the access channel gets the Alpha Tester role, without waiting for them to sign in on the website
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(120));
        sync_roles();
    });
}

fn sync_roles() {
    let (Some(token), Some(guild)) = (bot_token(), env("DISCORD_GUILD_ID")) else { return };
    let Some(alpha) = role_id("Alpha Tester") else { return };
    for id in crate::auth::access_list(&crate::auth::http(), &guild, &token) {
        let Some(member) = api(reqwest::Method::GET, &format!("/guilds/{guild}/members/{id}"), None) else {
            continue;
        }; // not in the server yet
        let has =
            member["roles"].as_array().is_some_and(|a| a.iter().any(|r| r.as_str() == Some(alpha.as_str())));
        if !has {
            if api(reqwest::Method::PUT, &format!("/guilds/{guild}/members/{id}/roles/{alpha}"), None)
                .is_some()
            {
                println!("[bot] gave Alpha Tester to {id}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn only_requests_signed_by_discord_are_accepted() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        let body = br#"{"type":1}"#;
        let sig = hex::encode(sk.sign(&[b"1700000000".as_slice(), body].concat()).to_bytes());
        assert!(verified(&vk, body, &sig, "1700000000"));
        assert!(!verified(&vk, body, &sig, "1700000001"), "a different timestamp must fail");
        assert!(!verified(&vk, br#"{"type":2}"#, &sig, "1700000000"), "a changed body must fail");
        assert!(!verified(&vk, body, "zz", "1700000000"));
    }

    #[test]
    fn ping_and_public_commands_answer() {
        assert_eq!(respond(&json!({ "type": 1 })), json!({ "type": 1 }));
        for name in ["help", "apply", "download", "links"] {
            let r = respond(
                &json!({ "type": 2, "data": { "name": name }, "member": { "roles": [], "user": { "id": "1" } } }),
            );
            assert_eq!(r["type"], 4, "{name}");
            assert!(
                r["data"]["components"][0]["components"].as_array().is_some_and(|a| !a.is_empty()),
                "{name}"
            );
        }
    }

    #[test]
    fn the_site_card_follows_the_components_v2_rules() {
        let ask = |name: &str| respond(&json!({ "type": 2, "data": { "name": name }, "member": { "roles": [], "user": { "id": "1" } } }));
        for name in ["links", "website", "apply", "download", "status"] {
            let r = ask(name);
            assert_eq!(r["data"]["flags"].as_u64().unwrap() & (1 << 15), 1 << 15, "{name}: components v2");
            let container = &r["data"]["components"][0];
            assert_eq!(container["type"], 17, "{name}");
            let parts = container["components"].as_array().unwrap();
            // a container only holds sections, text, galleries, separators and rows
            assert!(parts.iter().all(|p| [1u64, 9, 10, 12, 14].contains(&p["type"].as_u64().unwrap())), "{name}");
            // the first part is a section: text on one side, the logo as its thumbnail
            let section = &parts[0];
            assert_eq!(section["type"], 9, "{name}");
            assert_eq!(section["accessory"]["type"], 11);
            assert!(section["accessory"]["media"]["url"].as_str().unwrap().starts_with("https://"));
            assert!(section["components"].as_array().is_some_and(|c| !c.is_empty() && c.len() <= 3 && c[0]["type"] == 10));
            // link buttons: at most five in a row, each with an address and no custom id
            let buttons = parts.last().unwrap();
            assert_eq!(buttons["type"], 1);
            let b = buttons["components"].as_array().unwrap();
            assert!(!b.is_empty() && b.len() <= 5, "{name}");
            assert!(b.iter().all(|x| x["style"] == 5 && x["url"].as_str().unwrap().starts_with("https://") && x.get("custom_id").is_none()), "{name}");
        }
        // only /links and /website carry the banner, and only /website is shown to everyone
        let has_banner = |r: &Value| r["data"]["components"][0]["components"].as_array().unwrap().iter().any(|p| p["type"] == 12 && p["items"][0]["media"]["url"].as_str().unwrap().ends_with("/assets/social.png"));
        assert!(has_banner(&ask("links")) && has_banner(&ask("website")) && !has_banner(&ask("download")));
        assert_eq!(ask("links")["data"]["flags"].as_u64().unwrap() & 64, 64, "/links is private to whoever asked");
        assert_eq!(ask("website")["data"]["flags"].as_u64().unwrap() & 64, 0, "/website is posted for everyone");
    }

    #[test]
    fn the_help_card_fits_discords_text_limit() {
        let r = respond(&json!({ "type": 2, "data": { "name": "help" }, "member": { "roles": [], "user": { "id": "1" } } }));
        let mut parts = vec![];
        crate::help::collect_text(&r["data"]["components"], &mut parts);
        let total: usize = parts.iter().map(|p| p.chars().count()).sum();
        assert!(total <= 3900, "the help card has {total} characters");
        assert!(command_total() <= 100 && command_total() >= 60);
    }
}
