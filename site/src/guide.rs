//! The drop-down menu in the #guide channel: setting Murmint up, building it, and choosing models and a budget.
//! Picking an entry answers privately, so the channel itself stays short.

use crate::bot::{card, link, row, sep, text};
use serde_json::Value;

const SETUP: &[(&str, &str, &str, &str)] = &[
    ("install", "Install it", "Get the installer and run it", "**Install it**\n1. Download the installer at https://murmint.app/download. Murmint is in closed alpha, so sign in with Discord there.\n2. Run it, choose where to install, and finish.\n3. Murmint sits in your tray until you hold the shortcut."),
    ("key", "Add your first key", "Models page, any provider", "**Add your first key**\nOpen **Models**, pick a provider and paste its API key. Cloudflare and Groq both have free tiers, so you can start without paying.\nMurmint has no subscription and no AI of its own: you pay your provider directly, for what you use."),
    ("talk", "Your first dictation", "Hold, speak, let go", "**Your first dictation**\nClick into any text box, hold **Ctrl + Win**, say what you want to say, and let go. It types a clean version where your cursor is.\nTip: keep holding while you pause to think. What you say in one hold comes out as one flowing paragraph."),
    ("keys", "Shortcuts", "Every key, and how to change them", "**Shortcuts** (all of them can be changed in Settings)\n**Ctrl + Win** dictate: hold, or click to start\n**Ctrl + Alt + D** hands-free\n**Esc** cancel\n**Ctrl + Alt + Z** undo the last insertion\n**Ctrl + Alt + F** clean up text you already typed"),
    ("voice", "Things you can say", "Punctuation, emoji, vocabulary", "**Things you can say**\n\"question mark\", \"exclamation mark\", \"comma\", \"colon\", \"semicolon\" give you the character.\n\"thumbs up emoji\", \"heart emoji\" and \"fire emoji\" type the emoji.\n\"add Kubernetes to my vocabulary\" teaches it a word."),
    ("smartscreen", "Windows warns me", "The SmartScreen message", "**Windows warns me**\nThe installer is not signed yet, so Windows may show \"Windows protected your PC\". Click **More info**, then **Run anyway**."),
    ("build", "Build it yourself", "From the source on GitHub", "**Build it yourself**\nYou need Node.js 20+, Rust and the Visual Studio C++ build tools. Then `npm install`, and `npm start` to run it or `npm run dist` to make the installer.\nThe code is at https://github.com/proimpacts/murmint."),
    ("help", "Still stuck?", "Where to ask", "**Still stuck?**\nAsk in #support and someone will help. For a bug in the alpha, use #bugs or the `/bug` command."),
];

const MODELS: &[(&str, &str, &str, &str)] = &[
    ("budget", "What will it cost a month?", "Light, everyday and heavy use", "**What will it cost a month?**\nMurmint has no subscription: you only pay your provider for what you dictate. Most people spend **under $5 a month**.\nAbout what each setup costs, by how much you talk:\n**Cheap** (Cloudflare Whisper + GPT-5.4 nano): $0.20 light, $0.65 everyday, $2 heavy\n**Balanced** (GPT-4o mini Transcribe + GPT-5.4 mini): $1.20 light, $3.70 everyday, $11 heavy\n**Best** (GPT-4o Transcribe + Claude Sonnet 5.5): $4 light, $12 everyday, $36 heavy\nPick \"How much will I use?\" if you are not sure which you are.\n-# Estimates from the prices Murmint ships with. Your provider's current rates and your own use decide the real number."),
    ("plan1", "Under $1 a month", "The cheapest setup", "**Under $1 a month**\nCloudflare Whisper for speech and GPT-5.4 nano for text.\nLight use: about $0.20. Everyday: about $0.65. Even heavy use is about $2.\nGood accuracy for clear speech, and the easiest way to start. A free Cloudflare or Groq key can make it free."),
    ("plan5", "About $5 a month", "A good default for most people", "**About $5 a month**\nGPT-4o mini Transcribe for speech and GPT-5.4 mini for text.\nLight use: about $1.20. Everyday: about $3.70. Heavy use: about $11.\nA clear step up in accuracy and polish, and it still fits well inside $5 for normal days."),
    ("plan10", "About $10 a month", "More accurate speech", "**About $10 a month**\nGPT-4o Transcribe for speech and GPT-5.4 mini for text.\nLight use: about $2. Everyday: about $6.40. Heavy use: about $19.\nThe most accurate speech at a modest text price. It suits accents, fast talkers and noisy rooms."),
    ("reviews", "What reviews say", "How the picks were chosen", "**What reviews say**\n**Speech:** independent benchmarks (Artificial Analysis, Coval, AIMultiple) put OpenAI GPT-4o Transcribe among the most accurate speech models, within a point or two of the best. A few newer models from Microsoft and AssemblyAI score lower word-error rates, but Murmint does not offer them yet, so GPT-4o Transcribe is the best you can pick.\n**Text:** writing and editing rankings from 2026 put Claude Opus 5.5 at or near the top, tied with Google's newest Gemini, and ahead of GPT. Reviewers add that for plain tidying a cheaper model is often enough.\n-# Rankings disagree and change often. The cheap and balanced picks are the best value, and the top picks are for when quality matters most."),
    ("plan20", "$20 a month: the best", "Top accuracy and polish", "**$20 a month: the best**\nGPT-4o Transcribe for speech and Claude Sonnet 5.5 for text.\nLight use: about $4. Everyday: about $12. Heavy use: about $36, so heavy talkers should watch the total.\nWant the very top? Claude Opus 5.5 for text brings everyday use to about $18, and heavy use to about $55."),
    ("plan100", "About $100 a month", "The best, for people who talk a lot", "**About $100 a month**\nThe best Murmint offers: GPT-4o Transcribe for speech and Claude Opus 5.5 for text.\nAbout $18 at 30 minutes of talking a day, $55 at 90 minutes, and $92 at 2.5 hours.\nThe ceiling for this budget is about 2 hours 40 minutes a day. Beyond that the best setup goes over $100, so drop to Claude Sonnet 5.5 for text if you talk more.\nTip: set a monthly spending limit in your provider's dashboard so you can never go over."),
    ("plan150", "About $150 a month", "The best, all day long", "**About $150 a month**\nThe same best setup: GPT-4o Transcribe for speech and Claude Opus 5.5 for text.\nAbout $92 at 2.5 hours of talking a day, $110 at 3 hours and $147 at 4 hours.\nThe ceiling for this budget is about 4 hours a day. Past that, use Claude Sonnet 5.5 for text and the same speech model.\nTip: set a monthly spending limit in your provider's dashboard so you can never go over."),
    ("usage", "How much will I use?", "What light, everyday and heavy mean", "**How much will I use?**\n**Light:** about 10 minutes of talking a day. A few messages and emails.\n**Everyday:** about 30 minutes a day. Messages, emails and notes through the day.\n**Heavy:** about 90 minutes a day. Writing and replying most of the working day.\nMurmint shows what every dictation cost in **History**, so after a week you will know your real number. You can also set a monthly limit in your provider's dashboard."),
    ("speech", "Speech models, cheapest to best", "What hears you", "**Speech models** (what hears you, per minute of talking)\n**Cheapest:** Cloudflare Whisper Large V3 Turbo, $0.0005. Groq also has a free tier.\n**Best value:** OpenAI GPT-4o mini Transcribe, $0.003.\n**Most accurate:** OpenAI GPT-4o Transcribe, $0.006. GPT Live Transcribe, also $0.006, streams while you talk."),
    ("budget-text", "Budget text models", "Cleans and formats for almost nothing", "**Budget text** (per million tokens, in / out)\nGPT-5.4 nano: $0.05 / $0.40\nClaude Haiku 5.5: $0.10 / $0.50\nCloudflare Llama 3.2 3B: $0.05 / $0.34"),
    ("balanced-text", "Balanced text models", "The sweet spot for most people", "**Balanced text** (per million tokens, in / out)\nGPT-5.4 mini: $0.25 / $2\nMistral Small 3.1 on Cloudflare: $0.35 / $0.56\nGPT-4.1 mini: $0.40 / $1.60"),
    ("premium-text", "Premium text models", "The best polish", "**Premium text** (per million tokens, in / out)\nClaude Sonnet 5.5: $2 / $10\nClaude Opus 5.5: $4 / $20\nWorth it for long emails and documents."),
    ("free", "Free and local options", "Start without paying", "**Free and local options**
A free Cloudflare or Groq key covers speech and text. Gemini has a free tier too, and OpenCode Zen has some free models. Free tiers have limits, and Murmint falls back to your next model if one runs out.
Or run a model on your own PC with Ollama or LM Studio: nothing leaves your computer and it costs nothing per dictation."),
    ("pairs", "Good starting pairs", "A speech model and a text model", "**Good starting pairs**\n**Cheap:** Cloudflare Whisper + GPT-5.4 nano\n**Balanced:** GPT-4o mini Transcribe + GPT-5.4 mini\n**Best:** GPT-4o Transcribe + Claude Sonnet 5.5\nClaude can write text but cannot transcribe, so it always needs a speech model beside it.\n-# Prices are what Murmint ships with. Check your provider for today's rate."),
];

fn find<'a>(list: &'a [(&str, &str, &str, &str)], value: &str) -> Option<&'a (&'a str, &'a str, &'a str, &'a str)> {
    list.iter().find(|e| e.0 == value)
}

/// What a pick in the guide menu answers with, privately to whoever picked it.
pub fn answer(v: &Value) -> Value {
    let value = v["data"]["values"][0].as_str().unwrap_or("");
    let found = find(SETUP, value).map(|e| (e, link("Download", "https://murmint.app/download")))
        .or_else(|| find(MODELS, value).map(|e| (e, link("Providers", "https://murmint.app/providers"))));
    match found {
        Some((e, button)) => card(vec![text(e.3), sep(), row(vec![button])], false),
        None => card(vec![text("That entry is not available any more.")], false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_entry_answers_fits_discords_limits_and_has_its_own_value() {
        let all: Vec<_> = SETUP.iter().chain(MODELS.iter()).collect();
        assert!(all.len() <= 25, "one select menu holds at most 25 options");
        let mut values: Vec<_> = all.iter().map(|e| e.0).collect();
        values.sort();
        values.dedup();
        assert_eq!(values.len(), all.len());
        for e in all {
            assert!(e.1.len() <= 100 && e.2.len() <= 100, "{}", e.0);
            let a = answer(&json!({ "data": { "custom_id": "guide:menu", "values": [e.0] } }));
            assert!(a["data"]["components"][0]["components"][0]["content"].as_str().unwrap().len() < 2000, "{}", e.0);
        }
    }
}
