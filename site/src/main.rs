//! The Murmint website's tooling, in Rust and with no dependencies.
//!
//!   murmint-site build [website-dir]        turn website/src/<page>.body.html into website/<page>.html (shared header and footer)
//!   murmint-site serve [website-dir] [port] serve the site locally (or on a small server) with clean URLs and safe headers
//!
//! The pages themselves (HTML, CSS and the little script) stay as the website's UI; everything that builds or serves them is here.
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
mod acknowledge;
mod auth;
mod link;
mod store;
mod release;

pub struct Page {
    pub file: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    pub nav: &'static str,
}

pub const PAGES: [Page; 12] = [
    Page { file: "index", title: "Murmint | Speak. It types.", desc: "Murmint turns your voice into clean, formatted text in any Windows app. Hold a shortcut, talk, and finished text appears where your cursor is.", nav: "" },
    Page { file: "features", title: "Features | Murmint", desc: "Live transcription, punctuation that follows your pauses, spoken corrections, lists, writing styles, rules and a vocabulary you teach by voice.", nav: "features" },
    Page { file: "how-it-works", title: "How it works | Murmint", desc: "Shortcuts, the three ways to dictate, and everything you can say to Murmint.", nav: "how-it-works" },
    Page { file: "providers", title: "AI providers | Murmint", desc: "Use the AI you already pay for: OpenAI, Claude, Groq, Cloudflare, Mistral, Gemini and more, or a model on your own PC.", nav: "providers" },
    Page { file: "privacy", title: "Privacy | Murmint", desc: "What Murmint sends, where it goes, and what stays on your PC.", nav: "privacy" },
    Page { file: "download", title: "Download | Murmint", desc: "Download Murmint for Windows 10 and 11, or build it yourself from the source.", nav: "download" },
    Page { file: "terms", title: "Terms of service | Murmint", desc: "The terms for using the Murmint app and website.", nav: "terms" },
    Page { file: "privacy-policy", title: "Privacy policy | Murmint", desc: "How the Murmint app and website handle your information.", nav: "privacy-policy" },
    Page { file: "performance", title: "Performance | Murmint", desc: "How much memory Murmint uses, why it is light, and why it feels quick.", nav: "performance" },
    Page { file: "mission", title: "Mission | Murmint", desc: "Why Murmint exists: voice typing that is fast, light and fair, built on Rust and your own AI services.", nav: "mission" },
    Page { file: "contributors", title: "Contributors | Murmint", desc: "The people behind Murmint.", nav: "contributors" },
    Page { file: "faq", title: "FAQ | Murmint", desc: "Is Murmint free, what happens to your data, why Windows shows a security warning, and how to install it.", nav: "faq" },
];

const NAV: [(&str, &str, &str); 6] = [
    ("/features", "Features", "features"),
    ("/how-it-works", "How it works", "how-it-works"),
    ("/providers", "Providers", "providers"),
    ("/performance", "Performance", "performance"),
    ("/privacy", "Privacy", "privacy"),
    ("/faq", "FAQ", "faq"),
];

fn current(nav: &str, key: &str) -> &'static str {
    if nav == key {
        " aria-current=\"page\""
    } else {
        ""
    }
}

pub fn layout(p: &Page, body: &str) -> String {
    let base = auth::public_url(0);
    let canon = if p.file == "index" { format!("{base}/") } else { format!("{base}/{}", p.file) };
    let img = format!("{base}/assets/social.png");
    let ver = asset_version(); // changes on every build, so browsers never pair new pages with an old stylesheet
    // the logo sits in the middle: four links on its left, two on its right (next to the Download button), so both sides weigh about the same
    let anchors = |from: usize, to: usize| -> String { NAV[from..to].iter().map(|(href, label, key)| format!("<a href=\"{href}\"{}>{label}</a>", current(p.nav, key))).collect::<Vec<_>>().join("") };
    // logo on the left with every link right beside it; the Download button sits at the far end
    let (links_l, links_r) = (anchors(0, NAV.len()), String::new());
    let links: String = NAV
        .iter()
        .map(|(href, label, key)| format!("<a href=\"{href}\"{}>{label}</a>", current(p.nav, key)))
        .collect::<Vec<_>>()
        .join("\n    ");
    format!(
        r##"<!doctype html>
<html lang="en" data-theme="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{desc}">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{desc}">
<meta property="og:type" content="website">
<meta property="og:site_name" content="Murmint">
<meta property="og:url" content="{canon}">
<meta property="og:image" content="{img}">
<meta property="og:image:type" content="image/png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:alt" content="Murmint: speak, and it types. AI voice typing for Windows.">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{title}">
<meta name="twitter:description" content="{desc}">
<meta name="twitter:image" content="{img}">
<link rel="canonical" href="{canon}">
<meta name="theme-color" content="#000000">
<link rel="icon" href="assets/favicon.png">
<link rel="preload" href="assets/inter-latin.woff2" as="font" type="font/woff2" crossorigin>
<script>try{{var t=localStorage.getItem("theme");if(t==="light")document.documentElement.dataset.theme=t}}catch(e){{}}</script>
<link rel="stylesheet" href="/style.css?v={ver}">
</head>
<body>
<header class="nav">
  <a class="brand" href="/" aria-label="Murmint, home"><img src="assets/logo.svg" alt="" width="30" height="30"><span>Murmint</span></a>
  <nav class="links links-l" aria-label="Main">{links_l}</nav>
  <div class="nav-right">
    <nav class="links links-r" aria-label="More">{links_r}</nav>
    <div class="cta-slot">
      <a class="btn primary small" href="/download" data-download{dl}>Download for Windows</a>
      <button class="menu" type="button" aria-label="Menu" aria-expanded="false" aria-controls="links"><i></i><i></i></button>
    </div>
  </div>
  <nav class="links links-m" id="links" aria-label="Menu">{links}</nav>
</header>
<main>
{body}
</main>
<footer>
  <div class="wrap">
    <div class="foot">
      <div><a class="brand" href="/"><img src="assets/logo.svg" alt="" width="30" height="30"><span>Murmint</span></a><p>Voice typing for Windows. Speak, and clean text appears where your cursor is.</p></div>
      <nav aria-label="Product"><b>Product</b><a href="/features">Features</a><a href="/how-it-works">How it works</a><a href="/providers">Providers</a><a href="/download">Download</a></nav>
      <nav aria-label="About"><b>About</b><a href="/mission">Mission</a><a href="/contributors">Contributors</a><a href="/acknowledgements">Acknowledgements</a></nav>
      <nav aria-label="Help"><b>Help</b><a href="/faq">FAQ</a><a href="/privacy">Privacy</a><a href="https://discord.gg/MEfdzfyARV">Discord</a></nav>
      <nav aria-label="Legal"><b>Legal</b><a href="/terms">Terms of service</a><a href="/privacy-policy">Privacy policy</a></nav>
    </div>
    <p class="legal">&copy; <span id="yr"></span> Murmint. For Windows 10 and 11.</p>
  </div>
</footer>
<div class="theme" role="group" aria-label="Colour theme"><button type="button" data-theme-set="light" aria-label="Light theme"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg></button><button type="button" data-theme-set="dark" aria-label="Dark theme"><svg viewBox="0 0 24 24"><path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8Z"/></svg></button></div>
<div class="alpha-pop" id="alpha-pop" role="dialog" aria-label="Closed alpha" hidden><span class="alpha-dot"></span><div><b>Murmint is in closed alpha</b><p>Things are still changing. Sign in with Discord to join the testers and download.</p><div class="alpha-actions"><a class="btn primary small" href="/auth/discord">Join with Discord</a><button type="button" class="btn ghost small" id="alpha-close">Maybe later</button></div></div></div>
<script src="/script.js?v={ver}"></script>
</body>
</html>
"##,
        title = p.title,
        canon = canon,
        img = img,
        desc = p.desc,
        links = links,
        links_l = links_l,
        links_r = links_r,
        dl = current(p.nav, "download"),
        body = body.replace("?v=@ver", &format!("?v={ver}"))
    )
}

fn build(dir: &Path) -> Result<(), String> {
    for p in PAGES.iter() {
        let src = dir.join("src").join(format!("{}.body.html", p.file));
        let body = fs::read_to_string(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        let out = dir.join(format!("{}.html", p.file));
        fs::write(&out, layout(p, &body)).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("website/{}.html", p.file);
    }
    // every page, so search engines can find them all
    let base = auth::public_url(0);
    let urls: String = PAGES
        .iter()
        .map(|p| {
            let loc = if p.file == "index" { format!("{base}/") } else { format!("{base}/{}", p.file) };
            format!("  <url><loc>{loc}</loc></url>
")
        })
        .collect();
    let sitemap = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">
{urls}</urlset>
");
    fs::write(dir.join("sitemap.xml"), sitemap).map_err(|e| format!("sitemap.xml: {e}"))?;
    println!("website/sitemap.xml");
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// serving
// ------------------------------------------------------------------------------------------------
pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "exe" => "application/octet-stream",
        "webp" => "image/webp",
        "xml" => "application/xml",
        _ => "application/octet-stream",
    }
}

/// Turn a request path into a file inside `root`, or None if it tries to leave it or reach what must not be public
/// (the page sources and the reference folder). "/" and "/faq" map to index.html and faq.html.
pub fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let clean = url_path.split(['?', '#']).next().unwrap_or("");
    let mut rel = PathBuf::new();
    for part in clean.split('/').filter(|p| !p.is_empty()) {
        if part == ".." || part == "." || part.contains('\\') || part.contains(':') || part.contains('\0') {
            return None;
        }
        rel.push(part);
    }
    let first =
        rel.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
    if first == "src"
        || first.eq_ignore_ascii_case("Example website")
        || first.eq_ignore_ascii_case("Website baseline")
        || first.starts_with('.')
    {
        return None;
    }
    if rel.as_os_str().is_empty() {
        rel.push("index.html");
    }
    let mut full = root.join(&rel);
    if full.is_dir() {
        full = full.join("index.html");
    }
    if !full.exists() && full.extension().is_none() {
        let with = root.join(format!("{}.html", rel.display()));
        if with.exists() {
            full = with;
        }
    }
    if full.is_file() {
        Some(full)
    } else {
        None
    }
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8], head_only: bool, cache: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: {cache}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nX-Frame-Options: DENY\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    if !head_only {
        let _ = stream.write_all(body);
    }
}

fn handle(mut stream: TcpStream, root: &Path, port: u16) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    // read the headers, keeping the cookies (Discord sign-in needs them)
    let mut cookies = String::new();
    let mut length = 0usize;
    let mut forwarded = String::new();
    let mut authorization = String::new();
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if h == "\r\n" || h == "\n" {
                    break;
                }
            }
        }
        if let Some((k, v)) = h.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if k.eq_ignore_ascii_case("cookie") {
                cookies = v.to_string();
            } else if k.eq_ignore_ascii_case("content-length") {
                length = v.parse().unwrap_or(0);
            } else if k.eq_ignore_ascii_case("authorization") {
                authorization = v.to_string();
            } else if k.eq_ignore_ascii_case("x-real-ip") {
                forwarded = v.to_string();
            } else if k.eq_ignore_ascii_case("x-forwarded-for") && forwarded.is_empty() {
                // the last address is the one the hosting proxy added; earlier ones can be made up by the sender
                forwarded = v.rsplit(',').next().unwrap_or("").trim().to_string();
            }
        }
    }
    let mut it = line.split_whitespace();
    let (method, target) = (it.next().unwrap_or(""), it.next().unwrap_or("/"));
    let path_only = target.split('?').next().unwrap_or("/");
    // signing the app in with Discord (see link.rs)
    if path_only.starts_with("/api/link/") || path_only.starts_with("/api/account") || path_only == "/link" {
        let mut body = vec![0u8; length.min(4096)];
        if method == "POST" && reader.read_exact(&mut body).is_err() {
            return respond(&mut stream, "400 Bad Request", "application/json", br#"{"error":"bad request"}"#, false, "no-store");
        }
        let json = |stream: &mut TcpStream, (status, out): (&'static str, String)| {
            respond(stream, status, "application/json", out.as_bytes(), false, "no-store")
        };
        return match (method, path_only) {
            ("POST", "/api/link/start") => json(&mut stream, link::start(&auth::public_url(port))),
            ("POST", "/api/link/poll") => json(&mut stream, link::poll(&body)),
            ("GET", "/api/account") => json(&mut stream, link::account(&authorization)),
            ("POST", "/api/account/signout") => json(&mut stream, link::sign_out(&authorization)),
            ("GET" | "POST", "/link") => {
                let (status, headers, html) = link::page(method, target, &cookies, &body);
                let mut head = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nConnection: close\r\n", html.len());
                for (k, v) in headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(html.as_bytes());
                return;
            }
            _ => respond(&mut stream, "404 Not Found", "application/json", br#"{"error":"not found"}"#, false, "no-store"),
        };
    }
    // the old Marketplace is gone. Older apps that still ask get a plain answer.
    if path_only == "/api/marketplace" || path_only.starts_with("/api/marketplace/") {
        return respond(&mut stream, "410 Gone", "application/json; charset=utf-8", br#"{"error":"The Marketplace has been retired."}"#, false, "no-store");
    }
    // clean addresses: /features.html -> /features, /index.html -> /
    if (method == "GET" || method == "HEAD") && path_only.ends_with(".html") && !path_only.contains("..") {
        let clean = path_only.trim_end_matches(".html");
        let to = if clean == "/index" { "/".to_string() } else { clean.to_string() };
        let q = target.split_once('?').map(|(_, q)| format!("?{q}")).unwrap_or_default();
        let _ = stream.write_all(format!("HTTP/1.1 301 Moved Permanently\r\nLocation: {to}{q}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
        return;
    }
    // the thank-you page, built live from Discord and GitHub
    if (method == "GET" || method == "HEAD") && path_only == "/acknowledgements" {
        let p = Page {
            file: "acknowledgements",
            title: "Acknowledgements | Murmint",
            desc: "The staff, alpha testers and contributors who make Murmint.",
            nav: "",
        };
        let html = layout(&p, &acknowledge::body());
        return respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            html.as_bytes(),
            method == "HEAD",
            "no-cache",
        );
    }
    // the installer, for signed-in Alpha Testers only
    if method == "GET" && path_only == "/download/installer" {
        if !auth::can_download(&cookies) {
            let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /auth/discord\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return;
        }
        match installer() {
            Some(file) => {
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Murmint Setup.exe".into());
                let len = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/vnd.microsoft.portable-executable\r\nContent-Length: {len}\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(head.as_bytes());
                if let Ok(mut f) = fs::File::open(&file) {
                    let _ = std::io::copy(&mut f, &mut stream);
                }
            }
            None => {
                // the installer stored on this server first, then GitHub as a fallback
                let sent = release::packed_dir().is_some_and(|d| release::stream_packed(&mut stream, &d));
                if !sent && !release::stream_latest(&mut stream) {
                    respond(
                        &mut stream,
                        "503 Service Unavailable",
                        "text/plain; charset=utf-8",
                        b"The installer is not on this server yet.",
                        false,
                        "no-store",
                    );
                }
            }
        }
        return;
    }
    if method == "GET" && (path_only == "/auth/discord" || path_only == "/auth/discord/callback") {
        let r = if path_only == "/auth/discord" {
            auth::start(target, port)
        } else {
            auth::callback(target, &cookies, port)
        };
        let body = r.html.unwrap_or_default();
        let mut head = format!("HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nConnection: close\r\n", r.status, body.len());
        for (k, v) in r.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body.as_bytes());
        return;
    }
    if method != "GET" && method != "HEAD" {
        return respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"Method not allowed",
            false,
            "no-store",
        );
    }
    match resolve(root, target) {
        Some(path) => {
            let mut body = Vec::new();
            match fs::File::open(&path).and_then(|mut f| f.read_to_end(&mut body)) {
                Ok(_) => {
                    let name = path.to_string_lossy().to_string();
                    let cache = if name.ends_with(".html") { "no-cache" } else { "public, max-age=3600" };
                    respond(&mut stream, "200 OK", content_type(&name), &body, method == "HEAD", cache)
                }
                Err(_) => respond(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain",
                    b"Could not read the file",
                    false,
                    "no-store",
                ),
            }
        }
        None => respond(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Not found",
            method == "HEAD",
            "no-store",
        ),
    }
}

/// Discord sign-in settings can live in site/discord.env (KEY=value lines, kept out of git); real environment variables win.
fn load_env_file() {
    for p in ["site/discord.env", "discord.env"] {
        let Ok(text) = fs::read_to_string(p) else { continue };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if std::env::var(k.trim()).is_err() {
                    std::env::set_var(k.trim(), v.trim());
                }
            }
        }
        break;
    }
}

fn serve(dir: &Path, port: u16) -> Result<(), String> {
    load_env_file();
    let listener =
        TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    println!(
        "Murmint site on http://localhost:{port}  (serving {}){}",
        dir.display(),
        if auth::enabled() { ", Discord sign-in on" } else { ", Discord sign-in off" }
    );
    let root = dir.to_path_buf();
    for stream in listener.incoming().flatten() {
        let root = root.clone();
        std::thread::spawn(move || handle(stream, &root, port));
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = |i: usize| PathBuf::from(args.get(i).cloned().unwrap_or_else(|| "website".into()));
    let result = match args.first().map(|s| s.as_str()) {
        Some("build") => build(&dir(1)),
        Some("serve") => serve(
            &dir(1),
            args.get(2)
                .and_then(|p| p.parse().ok())
                .or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok()))
                .unwrap_or(8080),
        ), // hosts give the port in $PORT
        Some("pack-installer") => match args.get(1) {
            Some(exe) => release::pack(Path::new(exe), Path::new(args.get(2).map(|s| s.as_str()).unwrap_or("installer-files"))).map(|s| println!("{s}")),
            None => Err("usage: murmint-site pack-installer <installer.exe> [folder]".into()),
        },
        _ => Err("usage: murmint-site build [dir] | serve [dir] [port] | pack-installer <installer.exe> [folder]".into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0); // tests run in parallel: each gets its own folder
        let d = std::env::temp_dir().join(format!(
            "murmint-site-{}-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(d.join("src")).unwrap();
        fs::create_dir_all(d.join("Example website")).unwrap();
        for f in [
            "index.html",
            "faq.html",
            "style.css",
            "src/index.body.html",
            "Example website/index.html",
            ".secret",
        ] {
            fs::write(d.join(f), "x").unwrap();
        }
        d
    }

    #[test]
    fn pages_get_the_shared_header_and_the_right_current_link() {
        let faq = PAGES.iter().find(|p| p.file == "faq").unwrap();
        let h = layout(faq, "<p>body</p>");
        assert!(h.contains("<title>FAQ | Murmint</title>") && h.contains("<p>body</p>"));
        assert!(h.contains("href=\"/faq\" aria-current=\"page\""));
        assert!(!h.contains("href=\"/features\" aria-current"));
        assert!(layout(&PAGES[0], "")
            .contains("<a class=\"btn primary small\" href=\"/download\" data-download>"));
        assert_eq!(
            PAGES.iter().map(|p| p.file).collect::<std::collections::HashSet<_>>().len(),
            PAGES.len(),
            "every page has its own file"
        );
    }

    #[test]
    fn clean_urls_work_and_nothing_outside_the_site_is_reachable() {
        let d = tmp();
        assert_eq!(resolve(&d, "/"), Some(d.join("index.html")));
        assert_eq!(resolve(&d, "/faq"), Some(d.join("faq.html")));
        assert_eq!(resolve(&d, "/style.css?v=2"), Some(d.join("style.css")));
        for bad in [
            "/../secret",
            "/src/index.body.html",
            "/Example website/index.html",
            "/.secret",
            "/a/../../etc/passwd",
            "/c:/windows",
            "/nope",
        ] {
            assert_eq!(resolve(&d, bad), None, "{bad}");
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn content_types() {
        assert!(
            content_type("a.html").starts_with("text/html")
                && content_type("a.js").starts_with("text/javascript")
        );
        assert_eq!(content_type("a.woff2"), "font/woff2");
        assert_eq!(content_type("a.unknown"), "application/octet-stream");
    }

    fn get(port: u16, path: &str, cookie: &str) -> Vec<u8> {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
        let mut out = vec![];
        let _ = c.read_to_end(&mut out);
        out
    }

    #[test]
    fn a_signed_in_tester_downloads_the_installer_stored_on_the_server() {
        let d = tmp();
        let exe = d.join("Murmint Setup 7.7.7.exe");
        let original: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
        fs::write(&exe, &original).unwrap();
        release::pack_with(&exe, &d.join("installer-files"), 128_000).unwrap();
        std::env::set_var("INSTALLER_DIR", d.join("installer-files"));
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let root = d.clone();
        std::thread::spawn(move || {
            for _ in 0..2 {
                if let Ok((s, _)) = l.accept() {
                    handle(s, &root, 0);
                }
            }
        });
        // not signed in: sent to sign in, and nothing of the installer is sent
        let anon = get(port, "/download/installer", "x=1");
        assert!(String::from_utf8_lossy(&anon).starts_with("HTTP/1.1 302"), "{}", String::from_utf8_lossy(&anon[..60.min(anon.len())]));
        assert!(anon.len() < 400);
        // signed in: the whole file, byte for byte
        let token = auth::test_session();
        let got = get(port, "/download/installer", &format!("ms_dl={token}"));
        let split = got.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let head = String::from_utf8_lossy(&got[..split]).to_string();
        assert!(head.starts_with("HTTP/1.1 200") && head.contains("Content-Length: 300000") && head.contains("filename=\"Murmint Setup 7.7.7.exe\""), "{head}");
        assert_eq!(&got[split..], &original[..]);
        std::env::remove_var("INSTALLER_DIR");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn serves_a_page_over_a_real_socket() {
        let d = tmp();
        fs::write(d.join("index.html"), "<h1>hi</h1>").unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let root = d.clone();
        std::thread::spawn(move || {
            if let Ok((s, _)) = l.accept() {
                handle(s, &root, 0);
            }
        });
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(
            out.starts_with("HTTP/1.1 200 OK")
                && out.contains("<h1>hi</h1>")
                && out.contains("X-Content-Type-Options: nosniff"),
            "{out}"
        );
        let _ = fs::remove_dir_all(&d);
    }
}

/// The installer to hand out: MURMINT_INSTALLER, or the newest "Murmint Setup *.exe" in ../dist or ./dist.
fn installer() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MURMINT_INSTALLER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    ["dist", "../dist"]
        .iter()
        .filter_map(|d| fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with("Murmint Setup") && n.ends_with(".exe")
            })
        })
        .max_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// A short version for the stylesheet and script addresses: a hash of their contents.
pub fn asset_version() -> String {
    use std::hash::{Hash, Hasher};
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for dir in ["website", "."] {
            for name in ["style.css", "script.js"] {
                if let Ok(b) = fs::read(Path::new(dir).join(name)) {
                    b.hash(&mut h);
                }
            }
        }
        format!("{:x}", h.finish() & 0xffff_ffff)
    })
    .clone()
}
