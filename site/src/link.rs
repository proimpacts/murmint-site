//! Signing the app in with a Discord account, device-code style.
//!
//! POST /api/link/start   -> the app gets a secret device code and a short code to show the person
//! GET  /link?code=XXXX   -> the person signs in with Discord (identify only) and confirms the short code
//! POST /api/link/poll    -> the app swaps its device code for an account token once confirmed
//! GET  /api/account      -> who a token belongs to          (Authorization: Bearer <token>)
//! POST /api/account/signout -> forget a token
//!
//! Pending codes live in memory for 10 minutes. Tokens are stored only as SHA-256 hashes, with the Discord ID,
//! display name and avatar they belong to.

use crate::auth::{account_user, esc, layout_page};
use crate::store;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TTL: Duration = Duration::from_secs(600);
const MAX_PENDING: usize = 2000;
// no 0/O, 1/I/L, so codes are easy to read and type
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

struct Pending {
    user_code: String,
    created: Instant,
    approved: Option<(String, String, String)>,
}

fn pending() -> &'static Mutex<HashMap<String, Pending>> {
    static P: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("OS randomness");
    b
}

fn user_code() -> String {
    let b = random_bytes::<8>();
    let c: String = b.iter().map(|x| ALPHABET[*x as usize % ALPHABET.len()] as char).collect();
    format!("{}-{}", &c[..4], &c[4..])
}

fn normalize(code: &str) -> String {
    let c: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect();
    if c.len() == 8 { format!("{}-{}", &c[..4], &c[4..]) } else { c }
}

fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn avatar_url(id: &str, avatar: &str) -> String {
    if avatar.is_empty() { String::new() } else { format!("https://cdn.discordapp.com/avatars/{id}/{avatar}.png?size=128") }
}

pub fn start(public: &str) -> (&'static str, String) {
    let mut map = pending().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|_, p| p.created.elapsed() < TTL);
    if map.len() >= MAX_PENDING {
        return ("503 Service Unavailable", r#"{"error":"busy"}"#.into());
    }
    let device = hex::encode(random_bytes::<32>());
    let code = user_code();
    map.insert(device.clone(), Pending { user_code: code.clone(), created: Instant::now(), approved: None });
    let body = json!({
        "deviceCode": device, "userCode": code, "interval": 2, "expiresIn": TTL.as_secs(),
        "verifyUrl": format!("{public}/link?code={code}"),
    });
    ("200 OK", body.to_string())
}

pub fn poll(body: &[u8]) -> (&'static str, String) {
    let v: Value = serde_json::from_slice(body).unwrap_or_default();
    let device = v["deviceCode"].as_str().unwrap_or("");
    let mut map = pending().lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = map.get(device).filter(|p| p.created.elapsed() < TTL) else {
        map.remove(device);
        return ("410 Gone", r#"{"error":"expired"}"#.into());
    };
    let Some((id, name, avatar)) = p.approved.clone() else {
        return ("202 Accepted", r#"{"pending":true}"#.into());
    };
    map.remove(device);
    drop(map);
    let token = hex::encode(random_bytes::<32>());
    store::write(|s| {
        s["appAccounts"][hash(&token)] = json!({ "id": id, "name": name, "avatar": avatar, "created": now() });
    });
    let user = json!({ "id": id, "name": name, "avatar": avatar_url(&id, &avatar) });
    ("200 OK", json!({ "token": token, "user": user }).to_string())
}

fn bearer(auth_header: &str) -> Option<&str> {
    auth_header.strip_prefix("Bearer ").map(str::trim).filter(|t| t.len() == 64)
}

pub fn account(auth_header: &str) -> (&'static str, String) {
    let Some(t) = bearer(auth_header) else { return ("401 Unauthorized", r#"{"error":"signed out"}"#.into()) };
    let a = store::read(|s| s["appAccounts"][hash(t)].clone());
    match a["id"].as_str() {
        Some(id) => (
            "200 OK",
            json!({ "id": id, "name": a["name"], "avatar": avatar_url(id, a["avatar"].as_str().unwrap_or("")) }).to_string(),
        ),
        None => ("401 Unauthorized", r#"{"error":"signed out"}"#.into()),
    }
}

pub fn sign_out(auth_header: &str) -> (&'static str, String) {
    if let Some(t) = bearer(auth_header) {
        store::write(|s| {
            if let Some(m) = s["appAccounts"].as_object_mut() {
                m.remove(&hash(t));
            }
        });
    }
    ("200 OK", r#"{"ok":true}"#.into())
}

/// GET /link (shows the confirm form) and POST /link (confirms). Returns (status, extra headers, html).
pub fn page(method: &str, target: &str, cookies: &str, body: &[u8]) -> (&'static str, Vec<(String, String)>, String) {
    let code = if method == "POST" {
        std::str::from_utf8(body).ok().and_then(|b| crate::auth::query(&format!("?{b}"), "code")).unwrap_or_default()
    } else {
        crate::auth::query(target, "code").unwrap_or_default()
    };
    let code = normalize(&code);
    let Some((id, name, avatar)) = account_user(cookies) else {
        let next = crate::auth::enc(&format!("/link?code={code}"));
        return ("302 Found", vec![("Location".into(), format!("/auth/discord?next={next}"))], String::new());
    };
    if method == "POST" {
        let mut map = pending().lock().unwrap_or_else(|e| e.into_inner());
        let found = map.values_mut().find(|p| p.user_code == code && p.created.elapsed() < TTL && p.approved.is_none());
        return match found {
            Some(p) => {
                p.approved = Some((id, name.clone(), avatar));
                ("200 OK", vec![], layout_page("Murmint is signed in", &format!("<p class=\"lead-in\">You're signed in as <b>{}</b>.</p><p>You can close this tab and go back to Murmint.</p>", esc(&name))))
            }
            None => ("200 OK", vec![], layout_page("That code didn't work", "<p>It may have expired. In Murmint, press <b>Sign in</b> again to get a new code.</p>")),
        };
    }
    let form = format!(
        "<p class=\"lead-in\">Signed in to Discord as <b>{}</b>.</p><p>Check that this code matches the one in Murmint, then confirm. Only continue if you started this yourself.</p>\
         <form method=\"post\" action=\"/link\"><p><input name=\"code\" value=\"{}\" maxlength=\"9\" autocomplete=\"off\" style=\"font:700 22px/1 Inter,sans-serif;letter-spacing:.12em;text-align:center;width:220px;padding:12px;border-radius:12px;border:1px solid rgba(0,0,0,.15)\"></p>\
         <div class=\"actions\"><button class=\"btn primary\" type=\"submit\">Sign in to Murmint</button></div></form>",
        esc(&name),
        esc(&code)
    );
    ("200 OK", vec![], layout_page("Sign in to Murmint", &form))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_readable_and_normalized() {
        let c = user_code();
        assert_eq!(c.len(), 9);
        assert!(c.chars().all(|ch| ch == '-' || ALPHABET.contains(&(ch as u8))));
        assert_eq!(normalize("abcd efgh"), "ABCD-EFGH");
        assert_eq!(normalize("ABCD-EFGH"), "ABCD-EFGH");
    }

    #[test]
    fn a_device_is_pending_until_confirmed_then_gets_a_token_once() {
        let (_, started) = start("https://murmint.app");
        let v: Value = serde_json::from_str(&started).unwrap();
        let (device, code) = (v["deviceCode"].as_str().unwrap().to_string(), v["userCode"].as_str().unwrap().to_string());
        let poll_body = json!({ "deviceCode": device }).to_string();
        assert_eq!(poll(poll_body.as_bytes()).0, "202 Accepted");

        let cookie = crate::auth::test_account();
        assert_eq!(page("GET", "/link?code=x", "", b"").0, "302 Found", "must sign in with Discord first");
        let (st, _, html) = page("POST", "/link", &cookie, format!("code={code}").as_bytes());
        assert!(st == "200 OK" && html.contains("signed in"), "{html}");

        let (st, body) = poll(poll_body.as_bytes());
        assert_eq!(st, "200 OK");
        let token = serde_json::from_str::<Value>(&body).unwrap()["token"].as_str().unwrap().to_string();
        assert_eq!(poll(poll_body.as_bytes()).0, "410 Gone", "the device code is used up");
        assert!(account(&format!("Bearer {token}")).1.contains("Linker"));
        sign_out(&format!("Bearer {token}"));
        assert_eq!(account(&format!("Bearer {token}")).0, "401 Unauthorized");
    }

    #[test]
    fn a_wrong_code_does_nothing() {
        let (_, html) = { let r = page("POST", "/link", &crate::auth::test_account(), b"code=ZZZZ-ZZZZ"); (r.0, r.2) };
        assert!(html.contains("didn"));
    }
}
