//! Help for Murmint users and alpha testers: FAQ, guides, shortcuts, API key links, troubleshooting,
//! tips, the latest posts from the server's updates and roadmap channels, release notes, ratings, and automatic
//! announcements when a new version is published. The answers come from the website's own FAQ and how-it-works pages.

use crate::bot::{api, env, role_id};
use crate::features::{button, container, has_role, is_team, post_message, rand_below, reply, row, say, sep, text};
use crate::moderation::{actor, channel_named, dm, i_opt, now, s_opt, who};
use crate::store;
use reqwest::Method;
use serde_json::{json, Value};

const DOWNLOAD: &str = "https://murmint.app/download";

pub const NAMES: &[&str] = &[
    "faq", "guide", "shortcuts", "keys", "troubleshoot", "tip", "requirements", "privacy", "updates", "roadmap", "changelog", "rate", "testerguide",
];

// (key, label, answer)
const FAQ: &[(&str, &str, &str)] = &[
    ("free", "Is it free?", "Yes. Murmint is free, with no subscription and no word limits. It uses an AI provider you choose (such as Groq, Cloudflare Workers AI or OpenAI) with your own API key, and you pay that provider directly for what you use, usually fractions of a cent per dictation. Groq and Cloudflare have free tiers, and a model on your own PC through Ollama or LM Studio costs nothing."),
    ("privacy", "Is my data safe?", "Murmint has no servers and no accounts, so there is nowhere for us to send your data. Your voice goes from your PC to the speech provider you picked, and your text to the formatting provider you picked. Audio is never stored. API keys are encrypted with Windows and only sent to their own provider. History is optional and stays on your PC."),
    ("smartscreen", "Why a security warning?", "Windows shows \"Windows protected your PC\" for every app that is not code-signed, and Murmint is not code-signed yet because the certificate is expensive. It doesn't mean the file is dangerous. Click **More info**, then **Run anyway**. You can also read the source and build the installer yourself."),
    ("install", "How do I install it?", "Download the installer and run it. If Windows warns you, click More info, then Run anyway. Choose where to install and click Install. It takes a few seconds and needs no administrator rights. Then open **Models**, add an API key (or connect Ollama or LM Studio), click in any text box, hold **Ctrl + Win**, talk, and let go."),
    ("provider", "Which AI provider?", "For the cheapest start, Groq or Cloudflare Workers AI. For the best formatting, add Claude or a GPT model for text alongside a speech model. Claude can't transcribe audio, so pair it with OpenAI, Groq or Cloudflare for the speech part. A ChatGPT subscription does not include API access. `/keys` shows where to get each key."),
    ("apps", "Does it work in every app?", "It types wherever your cursor is, using paste, and puts your clipboard back right after. If no text box is selected it looks for the app's message box, and otherwise offers Copy. Apps running as administrator and some games ignore pasted text."),
    ("speed", "Will it slow my PC?", "No. One small engine written in Rust does the work, and Murmint sits quietly in your tray until you hold the shortcut."),
    ("platforms", "Mac or Linux?", "Yes, in the future we plan to add Linux and Mac, but for alpha testing we do not plan to. Murmint runs on Windows 10 and 11 for now."),
    ("uninstall", "How do I uninstall it?", "Use Installed apps in Windows Settings, or the uninstaller in the Murmint folder. It asks whether to delete your settings, history and saved keys too."),
    ("contact", "How do I reach you?", "Email proimpacts@auralink.dev, or ask in this server. Open a ticket if you need a private chat with the team."),
    ("who", "Who made it?", "One developer, with Claude as an AI pair programmer. The mission and contributors are on the website."),
];

const GUIDE: &[(&str, &str, &str, &str)] = &[
    ("start", "Getting started", "Install, add a key, and talk", "**1.** Run the installer. It needs no administrator rights, and Murmint lives in the tray.\n**2.** Open **Models** and add a key for a provider (Groq and Cloudflare Workers AI have free tiers), or connect Ollama or LM Studio.\n**3.** Click in any text box, hold **Ctrl + Win**, talk, and let go."),
    ("modes", "Three ways to dictate", "Hold, click, or hands-free", "**Hold to talk:** hold the shortcut and let go when you're done.\n**Click to start:** click once and Murmint stops after a few seconds of quiet.\n**Hands-free:** press **Ctrl + Alt + D** and it types after every pause, until you stop it.\nChange any shortcut in Settings."),
    ("spoken", "Just talk", "No commands to learn", "There are no commands to remember. Name three or more things and they become a list, count off first, second, third for steps, and say \"no wait\" to correct yourself. Talking to an AI? **Better prompts** turns what you say into a clear, numbered prompt."),
    ("corrections", "Correct yourself out loud", "No wait, I meant", "Change your mind while speaking and only the fix is typed. \"Meet at 8 p.m., no wait, 9 p.m.\" becomes **9 p.m.** \"Snapshot, I mean screenshot\" becomes **screenshot**."),
    ("styles", "Writing styles and rules", "Natural, Casual, Professional…", "Six built-in styles plus your own, and a list of rules the AI follows every time (like \"Use British spelling\"). Open **Writing** to turn one on. You can also list words Murmint should never write."),
    ("apps", "A style for each app", "Slack professional, Discord casual", "Open **Apps**, press **Add**, pick an app, and choose its style. Popular apps found on your PC are listed first, then everything else installed. Apps you haven't added use your default style. Say **Discord format** first for Discord styling."),
    ("vocabulary", "Teach it your words", "Names and terms by voice", "Open **Vocabulary**, type a word, and record yourself saying it five ways: normal, quiet, fast, slow and loud. You can also say \"add Kubernetes to my vocabulary\"."),
    ("context", "Message context", "Match what you already wrote", "Turn on **Message context** in Settings, under Dictation. Murmint reads up to the last 800 characters before your cursor and gives them to the formatting model, so punctuation and capital letters carry on from what's there. It's off by default, needs AI formatting, and never reads password fields."),
    ("emoji", "Say an emoji", "Smiley face, thumbs up emoji", "Say **smiley face** for \u{1F642}, or a name plus the word emoji: **thumbs up emoji** \u{1F44D}, **heart emoji** \u{2764}\u{FE0F}, **fire emoji** \u{1F525}. Only a few clear faces (smiley, frowny, winking, laughing, crying) work without the word emoji."),
    ("history", "History and cost", "See what each dictation cost", "Open **History** to see what each dictation sent, which model ran it and roughly what it cost. Keeping history is optional and it stays on your PC."),
];

const TROUBLE: &[(&str, &str, &str, &str)] = &[
    ("typed", "Nothing gets typed", "The text doesn't appear", "Murmint types by pasting, so check these in order:\n**1.** Is a text box selected? If not, the pill offers **Copy**.\n**2.** Apps running as administrator and some games ignore pasted text. Try a normal app.\n**3.** Open **Models** and check you added a working key for both speech and formatting.\n**4.** Hold the shortcut a little longer before you talk."),
    ("mic", "It doesn't hear me", "No transcription or the wrong mic", "Open **Settings → Dictation** and check the **Microphone** section picks the mic you're using. Make sure Windows lets desktop apps use the microphone (Settings → Privacy → Microphone), and that your speech provider has a working key under **Models**."),
    ("shortcut", "The shortcut does nothing", "Ctrl + Win isn't responding", "Another program may already use the keys. Change the shortcut in **Settings → Shortcuts** to a combination you don't use elsewhere. **Esc** cancels a dictation, and **Ctrl + Alt + Z** undoes the last insertion."),
    ("smartscreen", "Windows blocks the installer", "\"Windows protected your PC\"", "That appears for every app that isn't code-signed, and Murmint isn't yet. Click **More info**, then **Run anyway**. It doesn't mean the file is dangerous."),
    ("style", "Wrong writing style in an app", "It's too formal or too casual", "Open **Apps**, add the app, and pick the style you want for it. Apps you haven't added use your default style, which you set under **Writing**."),
    ("misheard", "It mishears a word", "Names and technical terms", "Open **Vocabulary** and record the word five ways. Murmint then swaps misheard phrases for your word and changes nothing else."),
    ("clipboard", "My clipboard changed", "Copied text went missing", "Murmint pastes your text and puts your clipboard back right after. If another program grabs the clipboard at the exact same moment it can be missed, so copy again and tell us in `/bug` if it keeps happening."),
    ("formatting", "The text came out wrong", "Punctuation, style or meaning", "Try a different formatting model under **Models**, check your **Writing** style and rules, and turn on **Message context** in Settings so Murmint can see what you already wrote. If it still changes the meaning, report it with `/bug` and include what you said."),
];

const KEYS: &[(&str, &str, &str)] = &[
    ("groq", "Groq", "**Groq** is a cheap, fast start with a free tier, and handles speech and text.\nGet a key: https://console.groq.com/keys"),
    ("cloudflare", "Cloudflare Workers AI", "**Cloudflare Workers AI** has a free tier and handles speech and text. You need your **Account ID** and an **API token** with Workers AI access.\nGet them: https://dash.cloudflare.com/ (open AI, then Workers AI)"),
    ("openai", "OpenAI", "**OpenAI** gives the best live transcription (Live transcription needs an OpenAI key) plus GPT models for text. It needs API billing, and a ChatGPT subscription does **not** include API access.\nGet a key: https://platform.openai.com/api-keys"),
    ("anthropic", "Anthropic (Claude)", "**Claude** gives excellent formatting, but it can't transcribe audio, so pair it with OpenAI, Groq or Cloudflare for speech.\nGet a key: https://console.anthropic.com/settings/keys"),
    ("gemini", "Google Gemini", "**Gemini** works for formatting text.\nGet a key: https://aistudio.google.com/apikey"),
    ("mistral", "Mistral", "**Mistral** works for formatting text.\nGet a key: https://console.mistral.ai/api-keys"),
    ("openrouter", "OpenRouter", "**OpenRouter** gives one key for many models.\nGet a key: https://openrouter.ai/keys"),
    ("local", "A model on your PC", "Run a model on your own PC with **Ollama** (https://ollama.com) or **LM Studio** (https://lmstudio.ai) and connect it under **Models**. It costs nothing and nothing leaves your computer."),
];


const TIPS: &[&str] = &[
    "Highlight text you typed and press **Ctrl + Alt + F** to format it like a dictation.",
    "Change your mind mid-sentence: \"at 8 p.m., no wait, 9 p.m.\" types only 9 p.m.",
    "Name three or more things in a row and Murmint turns them into a list for you.",
    "Hold **Ctrl + Win** to talk, or press **Ctrl + Alt + D** to go hands-free.",
    "Press **Esc** to cancel a dictation, or **Ctrl + Alt + Z** to undo the last insertion.",
    "Open **Apps** and add Slack as professional and Discord as casual, so each app gets the right tone.",
    "Record a tricky name five ways under **Vocabulary** and Murmint will stop mishearing it.",
    "Turn on **Message context** in Settings so Murmint can match what you already wrote.",
    "Say **smiley face** or **thumbs up emoji** to type the emoji.",
    "Say \"five times\" and you get 5 times. Say \"slash\" and you get /.",
    "Add words like \"utilize\" to **words to avoid** and Murmint will never write them.",
    "History shows what each dictation cost, usually a fraction of a cent.",
    "Groq and Cloudflare Workers AI both have free tiers, and Ollama on your PC costs nothing.",
    "Say **Discord format** before a message to get Discord styling.",
    "Use **/bug** if something misbehaves. Include what you said and which app you were in.",
];

fn menu(id: &str, placeholder: &str, options: Vec<Value>) -> Value {
    row(vec![json!({ "type": 3, "custom_id": id, "placeholder": placeholder, "options": options })])
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn faq_card(key: &str) -> Value {
    let (_, label, body) = FAQ.iter().find(|(k, _, _)| *k == key).unwrap_or(&FAQ[0]);
    reply(4, vec![text(&format!("## {label}\n{body}")), sep(), faq_menu()], false)
}
fn faq_menu() -> Value {
    menu("faq", "More questions", FAQ.iter().map(|(k, l, _)| json!({ "label": l, "value": k })).collect())
}
fn guide_menu() -> Value {
    menu("guide", "Pick a topic", GUIDE.iter().map(|(k, l, d, _)| json!({ "label": l, "value": k, "description": d })).collect())
}
fn trouble_menu() -> Value {
    menu("trouble", "What's going wrong?", TROUBLE.iter().map(|(k, l, d, _)| json!({ "label": l, "value": k, "description": d })).collect())
}

/// A choice made in one of the bot's drop-down menus.
pub fn select(menu_id: &str, value: &str) -> Value {
    match menu_id {
        "faq" => {
            let (_, label, body) = FAQ.iter().find(|(k, _, _)| *k == value).unwrap_or(&FAQ[0]);
            reply(7, vec![text(&format!("## {label}\n{body}")), sep(), faq_menu()], false)
        }
        "guide" => {
            let (_, label, _, body) = GUIDE.iter().find(|(k, ..)| *k == value).unwrap_or(&GUIDE[0]);
            reply(7, vec![text(&format!("## {label}\n{body}")), sep(), guide_menu()], false)
        }
        "trouble" => {
            let (_, label, _, body) = TROUBLE.iter().find(|(k, ..)| *k == value).unwrap_or(&TROUBLE[0]);
            reply(7, vec![text(&format!("## {label}\n{body}")), sep(), trouble_menu(), text("-# Still stuck? Use `/bug` or open a ticket and tell us what you tried.")], false)
        }
        _ => say("That menu isn't used anymore."),
    }
}

/// A FAQ answer by its topic, for auto-replies.
pub fn faq_answer(key: &str) -> Option<String> {
    FAQ.iter().find(|(k, _, _)| *k == key).map(|(_, l, b)| format!("**{l}**\n{b}"))
}

pub fn faq_keys() -> Vec<(&'static str, &'static str)> {
    FAQ.iter().map(|(k, l, _)| (*k, *l)).collect()
}

pub fn run(name: &str, v: &Value) -> Value {
    match name {
        "faq" => match s_opt(v, "topic") {
            Some(t) => faq_card(t),
            None => reply(4, vec![text("## Questions, answered\nPick a question."), faq_menu()], false),
        },
        "guide" => reply(4, vec![text("## Murmint guide\nPick what you want to learn."), guide_menu()], false),
        "troubleshoot" => reply(4, vec![text("## Troubleshooting\nPick what's going wrong and I'll walk you through it."), trouble_menu()], false),
        "shortcuts" => reply(4, vec![text("## Shortcuts\n**Ctrl + Win** hold to dictate, or click to start\n**Ctrl + Alt + D** hands-free\n**Esc** cancel\n**Ctrl + Alt + Z** undo the last insertion\n-# Each one can be changed in Settings → Shortcuts, to any combination of keys.")], false),
        "keys" => match s_opt(v, "provider") {
            Some(p) => {
                let (_, label, body) = KEYS.iter().find(|(k, _, _)| *k == p).unwrap_or(&KEYS[0]);
                reply(4, vec![text(&format!("## {label}\n{body}\n-# Add the key in Murmint under **Models**. Keys are encrypted with Windows and only sent to their own provider."))], false)
            }
            None => say("Pick a provider."),
        },
        "tip" => reply(4, vec![text(&format!("## Tip\n{}", TIPS[rand_below(TIPS.len() as u64) as usize]))], true),
        "requirements" => reply(4, vec![text("## What you need\n**Windows 10 or 11, 64-bit.** It installs for your account only and needs no administrator rights.\n**An AI provider key**, or a model on your PC with Ollama or LM Studio. Groq and Cloudflare Workers AI have free tiers.\n**A microphone.**\n-# Windows only for now. Mac and Linux are planned for later, not for the alpha.")], false),
        "privacy" => reply(4, vec![text("## What leaves your PC\n**Audio** goes to the speech provider you picked and is never stored.\n**Text** goes to the formatting provider you picked.\n**Keys** are encrypted with Windows and only sent to their own provider.\n**History** is optional and stays on your PC.\n**Message context** (off by default) also sends the last few hundred characters before your cursor to the formatting provider. Password fields are never read.\nWith a model on your own PC, nothing leaves your computer.\n-# There are no Murmint servers or accounts.")], false),
        "updates" => channel_posts("updates", "Latest updates"),
        "roadmap" => channel_posts("roadmap", "Roadmap"),
        "changelog" => changelog(s_opt(v, "version")),
        "rate" => rate(v),
        "testerguide" => reply(4, vec![
            text("## Alpha tester guide\nThank you for testing Murmint. Here's what helps most:"),
            sep(),
            text("**Report bugs with `/bug`.** Say what you did, what you expected and what happened. Add your Murmint version (shown under About in Settings) and Windows version.\n**Earn points for bugs the team confirms:** minor 1, major 3, critical 6, plus 2 for a bug found on a `/missions` bug hunt and 1 when it's fixed. Points build your level, and levels can earn rewards. `/rewards list` shows them and `/profile` shows your progress. Duplicates and non-bugs don't earn points.\n**Share ideas with `/feedback`**, and tell us how it's going with `/rate`.\n**Include details:** the app you typed into, which provider and model you used, and roughly what you said.\n**Never paste an API key** into the server.\n**Get new builds** on the website. Sign in with Discord there, and watch the builds channel for new versions."),
            row(vec![json!({ "type": 2, "style": 5, "label": "Download", "url": DOWNLOAD })]),
        ], false),
        _ => say("I don't know that command yet."),
    }
}

// ---------- rating ----------
fn rate(v: &Value) -> Value {
    let is_tester = role_id("Alpha Tester").is_some_and(|r| has_role(v, &r)) || is_team(v);
    if !is_tester {
        return say("Ratings are for alpha testers. Use `/apply` to join the alpha, or `/feedback` to share your thoughts.");
    }
    let stars = i_opt(v, "rating").unwrap_or(0).clamp(1, 5) as usize;
    let comment = s_opt(v, "comment").map(|c| clip(c, 800)).unwrap_or_default();
    let Some(channel) = env("DISCORD_FEEDBACK_CHANNEL").or_else(|| channel_named("feedback")) else {
        return say("I couldn't find the feedback channel.");
    };
    let mut parts = vec![text(&format!("## Rating: {}{} ({stars}/5)\nFrom {}", "\u{2605}".repeat(stars), "\u{2606}".repeat(5 - stars), who(&actor(v))))];
    if !comment.is_empty() {
        parts.push(sep());
        parts.push(text(&comment));
    }
    if post_message(&channel, parts, &[]).is_some() {
        say("Thank you. Your rating is posted for the team.")
    } else {
        say("I couldn't post that. Please try again in a moment.")
    }
}

// ---------- posts from the server's own channels ----------
/// Every piece of text in a message, whether it is plain or in a Components v2 card.
pub fn collect_text(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            if m.get("type").and_then(|t| t.as_u64()) == Some(10) {
                if let Some(c) = m.get("content").and_then(|c| c.as_str()) {
                    out.push(c.to_string());
                }
            }
            for (k, x) in m {
                if k == "components" {
                    collect_text(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_text(x, out)),
        _ => {}
    }
}

fn channel_posts(part: &str, title: &str) -> Value {
    let Some(channel) = channel_named(part) else { return say("I couldn't find that channel.") };
    let Some(msgs) = api(Method::GET, &format!("/channels/{channel}/messages?limit=8"), None) else { return say("I couldn't read that channel.") };
    let mut shown = vec![];
    for m in msgs.as_array().into_iter().flatten() {
        let mut parts = vec![];
        if let Some(c) = m["content"].as_str().filter(|c| !c.trim().is_empty()) {
            parts.push(c.to_string());
        }
        collect_text(&m["components"], &mut parts);
        let body = parts.join("\n");
        if !body.trim().is_empty() {
            shown.push(clip(&body, 700));
        }
        if shown.len() == 3 {
            break;
        }
    }
    if shown.is_empty() {
        return say("There's nothing posted there yet.");
    }
    let mut cards = vec![text(&format!("## {title}")), sep()];
    for (i, s) in shown.iter().enumerate() {
        if i > 0 {
            cards.push(sep());
        }
        cards.push(text(s));
    }
    cards.push(text(&format!("-# Newest first. More in <#{channel}>.")));
    reply(4, cards, false)
}

// ---------- releases ----------
/// "2026-11-13T22:08:32Z" as Unix seconds.
pub fn parse_iso(s: &str) -> Option<u64> {
    let (d, t) = s.split_once('T')?;
    let mut dp = d.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (dp.next()??, dp.next()??, dp.next()??);
    let mut tp = t.trim_end_matches('Z').split(':').map(|x| x.split('.').next().and_then(|n| n.parse::<i64>().ok()));
    let (h, mi, sec) = (tp.next()??, tp.next()??, tp.next()??);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec).ok()
}

fn release(tag: Option<&str>) -> Option<Value> {
    let token = env("GITHUB_TOKEN")?;
    let repo = env("RELEASE_REPO").unwrap_or_else(|| "proimpacts/murmint".into());
    let path = match tag {
        Some(t) => format!("tags/v{}", t.trim_start_matches('v')),
        None => "latest".into(),
    };
    let r = crate::auth::http()
        .get(format!("https://api.github.com/repos/{repo}/releases/{path}"))
        .bearer_auth(token)
        .header("User-Agent", "murmint-site")
        .send()
        .ok()?;
    if !r.status().is_success() {
        return None;
    }
    r.json().ok()
}

/// Release notes without the Markdown that Discord can't show well.
pub fn tidy_notes(body: &str) -> String {
    let lines: Vec<String> = body
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim_start_matches('#').trim_start().to_string())
        .collect();
    clip(&lines.join("\n"), 1400)
}

fn release_parts(r: &Value) -> Vec<Value> {
    let version = r["tag_name"].as_str().unwrap_or("").trim_start_matches('v');
    let when = r["published_at"].as_str().and_then(parse_iso).map(|t| format!(" · <t:{t}:D>")).unwrap_or_default();
    let notes = tidy_notes(r["body"].as_str().unwrap_or(""));
    vec![
        text(&format!("## Murmint {version}{when}")),
        sep(),
        text(&if notes.is_empty() { "No notes were written for this version.".to_string() } else { notes }),
        row(vec![json!({ "type": 2, "style": 5, "label": "Download", "url": DOWNLOAD })]),
    ]
}

fn changelog(version: Option<&str>) -> Value {
    match release(version) {
        Some(r) => reply(4, release_parts(&r), false),
        None => say(&if version.is_some() { "I couldn't find that version. Try a number like `0.5.1`.".to_string() } else { "I couldn't read the release notes right now. Try again soon.".to_string() }),
    }
}

/// Every ten minutes, announces a newly published version in the builds channel and pings Alpha Announcements.
/// The first time it runs it only remembers the current version, so a restart never repeats an old announcement.
pub fn watch_release() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let t = now();
    if t < LAST.load(Ordering::Relaxed) + 600 {
        return;
    }
    LAST.store(t, Ordering::Relaxed);
    let Some(r) = release(None) else { return };
    let Some(tag) = r["tag_name"].as_str() else { return };
    let seen = store::get("release.last");
    store::set("release.last", json!(tag));
    if seen.is_null() || seen.as_str() == Some(tag) {
        return;
    }
    let Some(channel) = env("DISCORD_RELEASE_CHANNEL").or_else(|| channel_named("builds")) else { return };
    let ping = role_id("Alpha Announcements");
    let mut parts = release_parts(&r);
    if let Some(p) = &ping {
        parts.insert(0, text(&format!("<@&{p}> a new build is out.")));
    }
    let body = json!({
        "flags": 1 << 15,
        "allowed_mentions": { "parse": [], "roles": ping.iter().collect::<Vec<_>>() },
        "components": [container(parts)],
    });
    crate::moderation::post_json(&channel, body);
}

// ---------- bug reports ----------
fn status_label(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "minor" => "Confirmed (minor)",
        "major" => "Confirmed (major)",
        "critical" => "Confirmed (critical)",
        "duplicate" => "Duplicate",
        "notabug" => "Not a bug",
        "progress" => "In progress",
        "fixed" => "Fixed",
        "norepro" => "Can't reproduce",
        "reopen" => "Open",
        _ => return None,
    })
}

/// Rewrites the status line of a bug report card. Returns the new card and the reporter's ID.
pub fn set_status(card: &Value, label: &str, by: &str) -> Option<(Value, String)> {
    let mut c = card.clone();
    let parts = c["components"].as_array_mut()?;
    let at = parts.iter().position(|p| p["content"].as_str().is_some_and(|t| t.starts_with("**Status:**")))?;
    let old = parts[at]["content"].as_str()?.to_string();
    let reporter = old.split("From <@").nth(1)?.split('>').next()?.to_string();
    let by_text = if label == "Open" { String::new() } else { format!(" · updated by {}", who(by)) };
    parts[at]["content"] = json!(format!("**Status:** {label}{by_text} · From {}", who(&reporter)));
    Some((c, reporter))
}

/// "## BUG-1k2x: It crashes" gives "BUG-1k2x".
pub fn bug_id_of(card: &Value) -> Option<String> {
    let head = card["components"][0]["content"].as_str()?;
    let id = head.trim_start_matches("## ").split(':').next()?;
    id.starts_with("BUG-").then(|| id.to_string())
}

/// A team member pressed a status button on a bug report. Confirming, fixing or rejecting it changes the reporter's points.
pub fn bug_status(v: &Value, kind: &str) -> Value {
    if !is_team(v) {
        return say("Only the Murmint Team can change a bug's status.");
    }
    let Some(label) = status_label(kind) else { return say("That button isn't used anymore.") };
    let card = &v["message"]["components"][0];
    let Some((updated, reporter)) = set_status(card, label, &actor(v)) else { return say("I couldn't read that bug report.") };
    if let Some(id) = bug_id_of(&updated) {
        if let Some(note) = crate::community::triage(&id, &reporter, kind) {
            dm(&reporter, &note);
        }
    }
    json!({ "type": 7, "data": { "flags": 1 << 15, "allowed_mentions": { "parse": [] }, "components": [updated] } })
}

/// Two rows of buttons for the team: decide what the bug is, and where it stands.
pub fn bug_buttons() -> Vec<Value> {
    vec![
        row(vec![
            button("Minor bug", "bug:minor", 2),
            button("Major bug", "bug:major", 2),
            button("Critical bug", "bug:critical", 4),
            button("Duplicate", "bug:duplicate", 2),
            button("Not a bug", "bug:notabug", 2),
        ]),
        row(vec![
            button("In progress", "bug:progress", 2),
            button("Fixed", "bug:fixed", 3),
            button("Can't reproduce", "bug:norepro", 2),
            button("Reopen", "bug:reopen", 2),
        ]),
    ]
}

pub fn definitions() -> Vec<Value> {
    let faq_choices: Vec<Value> = FAQ.iter().map(|(k, l, _)| json!({ "name": l, "value": k })).collect();
    let key_choices: Vec<Value> = KEYS.iter().map(|(k, l, _)| json!({ "name": l, "value": k })).collect();
    vec![
        json!({ "name": "faq", "description": "Answers to the questions people ask first", "options": [{ "type": 3, "name": "topic", "description": "A question (leave empty to browse)", "choices": faq_choices }] }),
        json!({ "name": "guide", "description": "Learn how to use Murmint, one topic at a time" }),
        json!({ "name": "troubleshoot", "description": "Fix a problem with Murmint step by step" }),
        json!({ "name": "shortcuts", "description": "Murmint's keyboard shortcuts" }),
        json!({ "name": "keys", "description": "Where to get an API key for a provider", "options": [{ "type": 3, "name": "provider", "description": "Which provider", "required": true, "choices": key_choices }] }),
        json!({ "name": "tip", "description": "A random tip for getting more from Murmint" }),
        json!({ "name": "requirements", "description": "What you need to run Murmint" }),
        json!({ "name": "privacy", "description": "What leaves your PC and what doesn't" }),
        json!({ "name": "updates", "description": "The latest posts from the updates channel" }),
        json!({ "name": "roadmap", "description": "What's planned, from the roadmap channel" }),
        json!({ "name": "changelog", "description": "Release notes for the latest version, or a specific one", "options": [{ "type": 3, "name": "version", "description": "A version like 0.5.1", "max_length": 20 }] }),
        json!({ "name": "rate", "description": "Tell the team how Murmint is working for you (alpha testers)", "options": [
            { "type": 4, "name": "rating", "description": "1 (poor) to 5 (great)", "required": true, "min_value": 1, "max_value": 5 },
            { "type": 3, "name": "comment", "description": "Anything you'd like to add", "max_length": 800 } ] }),
        json!({ "name": "testerguide", "description": "How to be a great alpha tester" }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_entry_fits_discords_limits() {
        for (name, rows) in [("faq", FAQ.iter().map(|(k, l, b)| (*k, *l, "", *b)).collect::<Vec<_>>()), ("guide", GUIDE.to_vec()), ("trouble", TROUBLE.to_vec())] {
            assert!(rows.len() <= 25, "{name}");
            let mut keys: Vec<&str> = rows.iter().map(|r| r.0).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), rows.len(), "{name}: keys are unique");
            for (k, label, desc, body) in rows {
                assert!(label.chars().count() <= 100 && desc.chars().count() <= 100 && k.len() <= 100, "{name}/{k}");
                assert!(body.chars().count() <= 1800, "{name}/{k} is too long for one card");
                assert!(!body.is_empty() && !label.is_empty(), "{name}/{k}");
            }
        }
        assert!(KEYS.len() <= 25);
        assert!(TIPS.iter().all(|t| t.len() < 300));
    }

    #[test]
    fn menus_and_cards_answer_for_every_topic() {
        for (k, ..) in GUIDE {
            assert_eq!(select("guide", k)["type"], 7);
        }
        for (k, ..) in TROUBLE {
            assert_eq!(select("trouble", k)["type"], 7);
        }
        for (k, ..) in FAQ {
            assert_eq!(select("faq", k)["type"], 7);
            assert_eq!(faq_card(k)["type"], 4);
        }
        let r = run("tip", &json!({ "data": {}, "member": { "user": { "id": "1" } } }));
        assert_eq!(r["type"], 4);
    }

    #[test]
    fn dates_from_github_are_read() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2023-11-14T22:13:20Z"), Some(1_700_000_000));
        assert_eq!(parse_iso("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_iso("nonsense"), None);
    }

    #[test]
    fn release_notes_lose_their_markdown_headings() {
        assert_eq!(tidy_notes("## What's new\n\n- Faster\n- Better\n"), "What's new\n- Faster\n- Better");
        assert!(tidy_notes(&"x".repeat(5000)).chars().count() <= 1400);
    }

    #[test]
    fn text_is_found_inside_cards() {
        let msg = json!([{ "type": 17, "components": [{ "type": 10, "content": "## Hello" }, { "type": 14 }, { "type": 1, "components": [{ "type": 2, "label": "x" }] }, { "type": 10, "content": "World" }] }]);
        let mut out = vec![];
        collect_text(&msg, &mut out);
        assert_eq!(out, vec!["## Hello", "World"]);
    }

    #[test]
    fn a_bug_report_changes_status_and_keeps_its_reporter() {
        let card = json!({ "type": 17, "accent_color": 1, "components": [
            { "type": 10, "content": "## BUG-1: It crashes" },
            { "type": 10, "content": "**Status:** Open · From <@123>" },
            { "type": 10, "content": "details" } ] });
        let (c, reporter) = set_status(&card, "Fixed", "9").unwrap();
        assert_eq!(reporter, "123");
        assert_eq!(c["components"][1]["content"], "**Status:** Fixed · updated by <@9> · From <@123>");
        assert_eq!(c["components"][2]["content"], "details", "the rest is untouched");
        assert_eq!(c["accent_color"], 1);
        let (reopened, _) = set_status(&c, "Open", "9").unwrap();
        assert_eq!(reopened["components"][1]["content"], "**Status:** Open · From <@123>");
        assert!(set_status(&json!({ "components": [] }), "Fixed", "9").is_none());
    }

    #[test]
    fn only_the_team_can_change_a_bug_and_ratings_need_the_role() {
        let click = json!({ "type": 3, "data": { "custom_id": "bug:fixed" }, "member": { "roles": [], "user": { "id": "5" } }, "message": { "components": [{}] } });
        let r = bug_status(&click, "fixed");
        assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().starts_with("Only the Murmint Team"));
        let r = run("rate", &json!({ "data": { "options": [{ "name": "rating", "value": 5 }] }, "member": { "roles": [], "user": { "id": "5" } } }));
        assert!(r["data"]["components"][0]["components"][0]["content"].as_str().unwrap().contains("alpha testers"));
    }
}
