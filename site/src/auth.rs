//! "Sign in with Discord" for the closed alpha.
//!
//! /auth/discord           -> sends you to Discord to approve (scopes: identify, guilds.join)
//! /auth/discord/callback  -> swaps the code for a token, adds you to the Murmint server if you are not in it yet,
//!                            then shows "You're in" for Alpha Testers, or the alpha form with your Discord already filled in.
//!
//! Secrets come from the environment, never from files in the site:
//!   DISCORD_CLIENT_ID, DISCORD_CLIENT_SECRET, DISCORD_BOT_TOKEN, DISCORD_GUILD_ID
//!   PUBLIC_URL (e.g. https://murmint.example; defaults to http://localhost:<port>)
//!   ALPHA_FORM (defaults to the Tally form), ALPHA_ROLE (role name, defaults to "Alpha Tester")

use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

const SESSION_SECS: u64 = 7 * 24 * 3600;
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---- signed tokens ----
// Sign-in sessions and the OAuth `state` are signed (HMAC-SHA256) values, not entries in the server's memory. So a
// session survives every restart and deploy, and the sign-in link works even if Discord sends the person back to a
// different browser than the one they started in (Discord's own in-app browser does this).
//   token = kind.expiry.data.signature        (data is hex, so it never contains a dot)

fn secret() -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let base = env("SESSION_SECRET")
        .or_else(|| env("DISCORD_CLIENT_SECRET"))
        .or_else(|| env("DISCORD_BOT_TOKEN"))
        .unwrap_or_else(|| "murmint-local-development-secret".into());
    Sha256::digest(format!("murmint-site-sessions|{base}").as_bytes()).to_vec()
}

fn mac() -> hmac::Hmac<sha2::Sha256> {
    use hmac::Mac;
    hmac::Hmac::<sha2::Sha256>::new_from_slice(&secret()).expect("any key length works")
}

fn token_at(kind: &str, data: &str, expires: u64) -> String {
    use hmac::Mac;
    let body = format!("{kind}.{expires}.{data}");
    let mut m = mac();
    m.update(body.as_bytes());
    format!("{body}.{}", hex::encode(m.finalize().into_bytes()))
}

fn make_token(kind: &str, data: &str, lives_secs: u64) -> String {
    token_at(kind, data, now() + lives_secs)
}

/// The data inside a token, if it is the right kind, correctly signed and not expired.
fn read_token(kind: &str, token: &str) -> Option<String> {
    use hmac::Mac;
    let (body, sig) = token.rsplit_once('.')?;
    let mut m = mac();
    m.update(body.as_bytes());
    m.verify_slice(&hex::decode(sig).ok()?).ok()?;
    let mut parts = body.splitn(3, '.');
    let (k, expires, data) = (parts.next()?, parts.next()?.parse::<u64>().ok()?, parts.next()?);
    (k == kind && expires > now()).then(|| data.to_string())
}

/// Is this the sign-in state we handed out a few minutes ago? It proves itself, so no cookie is needed.
fn state_ok(state: &str) -> bool {
    read_token("s", state).is_some()
}

/// Where to go after sign-in, carried inside the signed state. Only app-linking pages are allowed.
fn state_next(state: &str) -> Option<String> {
    let text = String::from_utf8(hex::decode(read_token("s", state)?).ok()?).ok()?;
    let next = text.split_once('|')?.1;
    next.starts_with("/link").then(|| next.to_string())
}

/// An app-linking sign-in: any Discord account, for an hour. Separate from the alpha session.
fn account_cookie(id: &str, name: &str, avatar: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control() && *c != '|').take(40).collect();
    let avatar: String = avatar.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(64).collect();
    let t = make_token("a", &hex::encode(format!("{id}|{name}|{avatar}")), 3600);
    let secure = if public_url(0).starts_with("https://") { "; Secure" } else { "" };
    format!("ms_acct={t}; Path=/link; Max-Age=3600; HttpOnly; SameSite=Lax{secure}")
}

/// The Discord account signed in for linking the app: (id, name, avatar).
pub fn account_user(cookies: &str) -> Option<(String, String, String)> {
    let data = read_token("a", &cookie(cookies, "ms_acct")?)?;
    let text = String::from_utf8(hex::decode(data).ok()?).ok()?;
    let mut p = text.split('|');
    let (id, name, avatar) = (p.next()?, p.next().unwrap_or(""), p.next().unwrap_or(""));
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then(|| (id.into(), name.into(), avatar.into()))
}

#[cfg(test)]
pub fn test_account() -> String {
    account_cookie("42", "Linker", "").split(';').next().unwrap().to_string()
}

/// A session for a signed-in tester: their Discord ID and the name to show.
fn session_token(id: &str, name: &str, avatar: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control() && *c != '|').take(40).collect();
    let avatar: String = avatar.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(64).collect();
    make_token("d", &hex::encode(format!("{id}|{name}|{avatar}")), SESSION_SECS)
}

/// Who is signed in as an Alpha Tester on this browser (cookie ms_dl, made at sign-in): (Discord ID, display name).
pub fn session_user(cookies: &str) -> Option<(String, String)> {
    let data = read_token("d", &cookie(cookies, "ms_dl")?)?;
    let text = String::from_utf8(hex::decode(data).ok()?).ok()?;
    let mut p = text.split('|');
    let (id, name) = (p.next()?, p.next().unwrap_or(""));
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())).then(|| (id.to_string(), name.to_string()))
}

/// Test only: a signed-in session, as sign-in would make one. Returns the cookie value.
#[cfg(test)]
pub fn test_session() -> String {
    session_token("1", "Tester", "")
}

/// Is this browser signed in as an Alpha Tester?
pub fn can_download(cookies: &str) -> bool {
    session_user(cookies).is_some()
}

/// The cookie that keeps someone signed in. Secure on the real site, which is always https.
fn session_cookie(token: &str) -> String {
    let secure = if public_url(0).starts_with("https://") { "; Secure" } else { "" };
    format!("ms_dl={token}; Path=/; Max-Age={SESSION_SECS}; HttpOnly; SameSite=Lax{secure}")
}

pub struct Reply {
    pub status: &'static str,
    pub headers: Vec<(String, String)>,
    pub html: Option<String>,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}
pub fn enabled() -> bool {
    ["DISCORD_CLIENT_ID", "DISCORD_CLIENT_SECRET", "DISCORD_BOT_TOKEN", "DISCORD_GUILD_ID"]
        .iter()
        .all(|k| env(k).is_some())
}
/// The site's public address, used for the Discord callback. Defaults to https://murmint.app on a host (and ignores the
/// placeholder); on your own PC set PUBLIC_URL=http://localhost:8080 in site/discord.env.
pub(crate) fn public_url(_port: u16) -> String {
    env("PUBLIC_URL")
        .filter(|u| !u.contains("YOUR-SITE"))
        .unwrap_or_else(|| "https://murmint.app".into())
        .trim_end_matches('/')
        .to_string()
}
fn redirect_uri(port: u16) -> String {
    format!("{}/auth/discord/callback", public_url(port))
}

pub fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
pub fn query(target: &str, key: &str) -> Option<String> {
    target.split_once('?')?.1.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then(|| dec(v))
    })
}
fn cookie(header: &str, key: &str) -> Option<String> {
    header.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}
pub(crate) fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// An unguessable value for the OAuth `state`, from the OS-seeded hasher and the clock.
fn random_state() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (0..2)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u128(t);
            h.write_u8(i);
            format!("{:016x}", h.finish())
        })
        .collect()
}

pub fn start(target: &str, port: u16) -> Reply {
    if !enabled() {
        return page("Sign-in is not set up", "<p>Discord sign-in is not configured on this server yet.</p>");
    }
    // linking the app only needs to know who you are; the alpha sign-in also adds you to the server
    let next = query(target, "next").map(|n| dec(&n)).filter(|n| n.starts_with("/link"));
    let scope = if next.is_some() { "identify" } else { "identify guilds.join" };
    let state = make_token("s", &hex::encode(format!("{}|{}", random_state(), next.unwrap_or_default())), 600);
    let url = format!(
        "https://discord.com/oauth2/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&prompt=none",
        enc(&env("DISCORD_CLIENT_ID").unwrap()), enc(&redirect_uri(port)), enc(scope), state
    );
    Reply {
        status: "302 Found",
        headers: vec![("Location".into(), url), ("Cache-Control".into(), "no-store".into())],
        html: None,
    }
}

pub fn http() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("DiscordBot (https://github.com/proimpacts/murmint, 1.0)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap()
}

fn exchange(code: &str, port: u16) -> Result<String, String> {
    let form = [
        ("client_id", env("DISCORD_CLIENT_ID").unwrap()),
        ("client_secret", env("DISCORD_CLIENT_SECRET").unwrap()),
        ("grant_type", "authorization_code".into()),
        ("code", code.into()),
        ("redirect_uri", redirect_uri(port)),
    ];
    let r = http()
        .post("https://discord.com/api/v10/oauth2/token")
        .form(&form)
        .send()
        .map_err(|e| e.to_string())?;
    let v: Value = r.json().map_err(|e| e.to_string())?;
    v["access_token"].as_str().map(String::from).ok_or_else(|| {
        format!(
            "Discord said: {}",
            v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("no token")
        )
    })
}

pub fn callback(target: &str, _cookies: &str, port: u16) -> Reply {
    if !enabled() {
        return page("Sign-in is not set up", "<p>Discord sign-in is not configured on this server yet.</p>");
    }
    if query(target, "error").is_some() {
        return page("Sign-in cancelled", "<p>No problem. You can sign in with Discord any time.</p><p><a class=\"btn primary\" href=\"/auth/discord\">Try again</a></p>");
    }
    let (Some(code), Some(state)) = (query(target, "code"), query(target, "state")) else {
        return page("Something went wrong", "<p>Discord did not send a sign-in code.</p>");
    };
    if !state_ok(&state) {
        return page("Please try again", "<p>That sign-in took too long, so it expired. It only takes a moment.</p><p><a class=\"btn primary\" href=\"/auth/discord\">Sign in with Discord</a></p>");
    }
    if let Some(next) = state_next(&state) {
        return match who(&code, port) {
            Ok((id, name, avatar)) => Reply {
                status: "302 Found",
                headers: vec![
                    ("Location".into(), next),
                    ("Set-Cookie".into(), account_cookie(&id, &name, &avatar)),
                    ("Cache-Control".into(), "no-store".into()),
                ],
                html: None,
            },
            Err(e) => page("Something went wrong", &format!("<p>{}</p>", esc(&e))),
        };
    }
    match finish(&code, port) {
        Ok((html, session)) => {
            let mut headers = vec![("Cache-Control".into(), "no-store".into())];
            if let Some(t) = session {
                headers.push(("Set-Cookie".into(), session_cookie(&t)));
            }
            Reply { status: "200 OK", headers, html: Some(html) }
        }
        Err(e) => page(
            "Something went wrong",
            &format!(
                "<p>{}</p><p><a class=\"btn primary\" href=\"/auth/discord\">Try again</a></p>",
                esc(&e)
            ),
        ),
    }
}

/// The Discord account behind a sign-in code: (id, display name, avatar hash).
fn who(code: &str, port: u16) -> Result<(String, String, String), String> {
    let token = exchange(code, port)?;
    let me: Value = http()
        .get("https://discord.com/api/v10/users/@me")
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let id = me["id"].as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return Err("Could not read your Discord account.".into());
    }
    let name = me["global_name"].as_str().or(me["username"].as_str()).unwrap_or("there").to_string();
    Ok((id, name, me["avatar"].as_str().unwrap_or("").to_string()))
}

fn finish(code: &str, port: u16) -> Result<(String, Option<String>), String> {
    let token = exchange(code, port)?;
    let c = http();
    let me: Value = c
        .get("https://discord.com/api/v10/users/@me")
        .bearer_auth(&token)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let (id, name) = (
        me["id"].as_str().unwrap_or("").to_string(),
        me["global_name"].as_str().or(me["username"].as_str()).unwrap_or("there").to_string(),
    );
    let username = me["username"].as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return Err("Could not read your Discord account.".into());
    }
    let (guild, bot) = (env("DISCORD_GUILD_ID").unwrap(), env("DISCORD_BOT_TOKEN").unwrap());
    // join the server (201 = added just now, 204 = already a member)
    let joined = c
        .put(format!("https://discord.com/api/v10/guilds/{guild}/members/{id}"))
        .header("Authorization", format!("Bot {bot}"))
        .json(&serde_json::json!({ "access_token": token }))
        .send()
        .map_err(|e| e.to_string())?;
    let just_joined = joined.status().as_u16() == 201;
    let member: Value = c
        .get(format!("https://discord.com/api/v10/guilds/{guild}/members/{id}"))
        .header("Authorization", format!("Bot {bot}"))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .unwrap_or_default();
    let roles: Value = c
        .get(format!("https://discord.com/api/v10/guilds/{guild}/roles"))
        .header("Authorization", format!("Bot {bot}"))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .unwrap_or_default();
    let role_name = env("ALPHA_ROLE").unwrap_or_else(|| "Alpha Tester".into());
    let role_id = |n: &str| {
        roles
            .as_array()
            .and_then(|a| a.iter().find(|r| r["name"].as_str() == Some(n)))
            .and_then(|r| r["id"].as_str())
            .map(String::from)
    };
    let alpha_id = role_id(&role_name);
    let has = |rid: &Option<String>| {
        rid.as_ref().is_some_and(|rid| {
            member["roles"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(rid.as_str())))
        })
    };
    // who gets in: the owner, anyone with the Alpha Tester or Murmint Team role, or anyone listed in the staff access channel
    let owner = crate::bot::owner();
    let listed = access_list(&c, &guild, &bot).contains(&id);
    let is_alpha = id == owner || has(&alpha_id) || has(&role_id("Murmint Team")) || listed;
    // listed people get the Alpha Tester role too, so the alpha channels open for them in Discord
    if listed && !has(&alpha_id) {
        if let Some(rid) = &alpha_id {
            let _ = c
                .put(format!("https://discord.com/api/v10/guilds/{guild}/members/{id}/roles/{rid}"))
                .header("Authorization", format!("Bot {bot}"))
                .header("X-Audit-Log-Reason", "Listed in alpha-access")
                .send();
        }
    }
    let server = format!("https://discord.com/channels/{guild}");
    let hi = esc(&name);
    let session = is_alpha.then(|| session_token(&id, &name, me["avatar"].as_str().unwrap_or("")));
    Ok((
        if is_alpha {
            layout_page("You're in", &format!(
            "<p class=\"lead-in\">Welcome back, {hi}. You are an <b>Alpha Tester</b>.</p><p>Alpha builds, the tester chat and bug reports are waiting in the Murmint Discord.</p><div class=\"actions\"><a class=\"btn primary\" href=\"{server}\">Open the Discord</a><a class=\"btn ghost\" href=\"/download/installer\">Download Murmint</a></div>"
        ))
        } else {
            let form = env("ALPHA_FORM").unwrap_or_else(|| "https://tally.so/r/Qoe4y7".into());
            let sep = if form.contains('?') { '&' } else { '?' };
            let apply = format!("{form}{sep}discord_id={}&discord_username={}", enc(&id), enc(&username));
            layout_page("One more step", &format!(
            "<p class=\"lead-in\">Hi {hi}, {}</p><p>Murmint is in a closed alpha. Fill in the short form and, once you are accepted, you get the <b>Alpha Tester</b> role and the alpha channels unlock.</p><div class=\"actions\"><a class=\"btn primary\" href=\"{}\">Apply for the alpha</a><a class=\"btn ghost\" href=\"{server}\">Open the Discord</a></div>",
            if just_joined { "you have joined the Murmint Discord." } else { "you are signed in." },
            esc(&apply)
        ))
        },
        session,
    ))
}

fn page(title: &str, body: &str) -> Reply {
    Reply { status: "200 OK", headers: vec![], html: Some(layout_page(title, body)) }
}

/// A plain page in the site's look (same stylesheet and alpha bar), without the build step.
pub(crate) fn layout_page(title: &str, body: &str) -> String {
    let p = crate::Page {
        file: "auth",
        title: "Murmint alpha",
        desc: "Sign in with Discord for the Murmint closed alpha.",
        nav: "",
    };
    crate::layout(&p, &format!("<section class=\"page-top wrap\"><h1>{}</h1></section><section class=\"block wrap\" style=\"padding-top:40px\"><div class=\"prose auth-card\">{body}</div></section>", esc(title))).replacen("<head>", "<head><base href=\"/\">", 1)
}

/// Discord user IDs pasted into the staff alpha-access channel (DISCORD_ACCESS_CHANNEL, or the channel whose name contains "alpha-access").
pub fn access_list(c: &reqwest::blocking::Client, guild: &str, bot: &str) -> Vec<String> {
    let auth = format!("Bot {bot}");
    let channel = env("DISCORD_ACCESS_CHANNEL").or_else(|| {
        let chans: Value = c
            .get(format!("https://discord.com/api/v10/guilds/{guild}/channels"))
            .header("Authorization", &auth)
            .send()
            .ok()?
            .json()
            .ok()?;
        chans
            .as_array()?
            .iter()
            .find(|ch| ch["name"].as_str().is_some_and(|n| n.contains("alpha-access")))
            .and_then(|ch| ch["id"].as_str())
            .map(String::from)
    });
    let Some(channel) = channel else { return vec![] };
    let msgs: Value = match c
        .get(format!("https://discord.com/api/v10/channels/{channel}/messages?limit=100"))
        .header("Authorization", &auth)
        .send()
        .and_then(|r| r.json())
    {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut ids = Vec::new();
    for m in msgs.as_array().into_iter().flatten() {
        let text = m["content"].as_str().unwrap_or("");
        let mut cur = String::new();
        for ch in text.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_digit() {
                cur.push(ch);
            } else {
                if (17..=20).contains(&cur.len()) {
                    ids.push(cur.clone());
                }
                cur.clear();
            }
        }
    }
    ids
}

/// The Discord IDs listed in the staff access channel (for the acknowledgements page).
pub fn listed_ids() -> Vec<String> {
    match (env("DISCORD_GUILD_ID"), env("DISCORD_BOT_TOKEN")) {
        (Some(g), Some(b)) => access_list(&http(), &g, &b),
        _ => vec![],
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;

    #[test]
    fn a_token_is_valid_only_if_it_is_signed_unexpired_and_the_right_kind() {
        let t = make_token("d", "12345", 600);
        assert_eq!(read_token("d", &t).as_deref(), Some("12345"));
        assert!(read_token("s", &t).is_none(), "a session can't be used as a sign-in state");
        assert!(read_token("d", &t.replacen("12345", "99999", 1)).is_none());
        let (body, _) = t.rsplit_once('.').unwrap();
        assert!(read_token("d", &format!("{body}.{}", "0".repeat(64))).is_none());
        assert!(read_token("d", &token_at("d", "1", now().saturating_sub(5))).is_none());
        assert!(read_token("d", "not a token").is_none() && read_token("d", "").is_none() && read_token("d", "a.b.c.zz").is_none());
    }

    #[test]
    fn who_is_signed_in_is_read_from_the_cookie() {
        let t = session_token("123456789012345678", "Sam | the <b>tester</b>", "abc_123");
        let (id, name) = session_user(&format!("theme=dark; ms_dl={t}; other=1")).unwrap();
        assert_eq!(id, "123456789012345678");
        assert_eq!(name, "Sam  the <b>tester</b>", "the separator is stripped; showing the name safely is the page's job");
        assert!(can_download(&format!("ms_dl={t}")));
        assert!(session_user("theme=dark").is_none());
        assert!(session_user(&format!("ms_dl={}", make_token("s", "7", 600))).is_none(), "a sign-in state isn't a session");
        assert!(session_user("ms_dl=abc").is_none());
        assert_eq!(session_user(&format!("ms_dl={}", session_token("1", &"x".repeat(200), ""))).unwrap().1.len(), 40);
    }

    #[test]
    fn the_sign_in_link_carries_its_own_proof_and_needs_no_cookie() {
        let state = make_token("s", &random_state(), 600);
        assert!(state_ok(&state), "a state we made is accepted, in any browser");
        assert!(!state_ok("forged") && !state_ok(&make_token("d", "1", 600)) && !state_ok(&token_at("s", "x", now().saturating_sub(1))));
        let c = session_cookie("t");
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Lax") && c.contains("Secure"), "{c}");
    }
}
