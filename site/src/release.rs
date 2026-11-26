//! The installer from the latest GitHub release of the Murmint repo, streamed to signed-in alpha testers.
//! Used on the host where the installer is not on disk. The repo is private, so a token is needed:
//!   GITHUB_TOKEN    a fine-grained token with read access to the repo's contents (releases)
//!   RELEASE_REPO    owner/name, defaults to proimpacts/murmint

use serde_json::Value;
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Asset {
    id: u64,
    name: String,
    size: u64,
}
static CACHE: Mutex<Option<(Instant, u64, String, u64)>> = Mutex::new(None);

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}
fn repo() -> String {
    env("RELEASE_REPO").unwrap_or_else(|| "proimpacts/murmint".into())
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("murmint-site")
        .timeout(Duration::from_secs(600))
        .build()
        .unwrap()
}

/// The setup .exe on the latest release (remembered for ten minutes so every download does not ask GitHub again).
fn latest(token: &str) -> Option<Asset> {
    if let Some((at, id, name, size)) = CACHE.lock().unwrap().clone() {
        if at.elapsed() < Duration::from_secs(600) {
            return Some(Asset { id, name, size });
        }
    }
    let rel: Value = client()
        .get(format!("https://api.github.com/repos/{}/releases/latest", repo()))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()?;
    let assets = rel["assets"].as_array()?;
    let pick = assets
        .iter()
        .find(|a| {
            let n = a["name"].as_str().unwrap_or("").to_lowercase();
            n.starts_with("murmint") && n.contains("setup") && n.ends_with(".exe") && !n.contains("core")
        })
        .or_else(|| {
            assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.to_lowercase().ends_with(".exe")))
        })?;
    let a = Asset {
        id: pick["id"].as_u64()?,
        name: pick["name"].as_str()?.to_string(),
        size: pick["size"].as_u64().unwrap_or(0),
    };
    *CACHE.lock().unwrap() = Some((Instant::now(), a.id, a.name.clone(), a.size));
    Some(a)
}

/// Streams the installer to the browser. False when there is no token or no release, so the caller can say so.
pub fn stream_latest(stream: &mut TcpStream) -> bool {
    let Some(token) = env("GITHUB_TOKEN") else { return false };
    let Some(asset) = latest(&token) else { return false };
    // GitHub answers with a redirect to its file storage; the token is not sent along to that other host
    let Ok(mut resp) = client()
        .get(format!("https://api.github.com/repos/{}/releases/assets/{}", repo(), asset.id))
        .bearer_auth(&token)
        .header("Accept", "application/octet-stream")
        .send()
    else {
        return false;
    };
    if !resp.status().is_success() {
        *CACHE.lock().unwrap() = None;
        return false;
    }
    let len = resp.content_length().unwrap_or(asset.size);
    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/vnd.microsoft.portable-executable\r\nContent-Length: {len}\r\nContent-Disposition: attachment; filename=\"{}\"\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", asset.name);
    if stream.write_all(head.as_bytes()).is_err() {
        return true;
    }
    let _ = std::io::copy(&mut resp, stream);
    true
}


// ---------- the installer kept on this server ----------
// GitHub won't take files over 100 MB, and the installer is a little over that, so it is stored as 40 MB pieces next to a
// manifest (installer-files/manifest.json) and sent to the browser as one file. Put a new build there with:
//   murmint-site pack-installer "path/to/Murmint Setup 5.2.exe"

const PIECE: usize = 40 * 1024 * 1024;

/// Where the packed installer is: INSTALLER_DIR, or installer-files next to the site.
pub fn packed_dir() -> Option<PathBuf> {
    std::env::var("INSTALLER_DIR")
        .ok()
        .into_iter()
        .chain(["installer-files".to_string(), "../installer-files".to_string()])
        .map(PathBuf::from)
        .find(|d| d.join("manifest.json").is_file())
}

/// Splits an installer into pieces and writes the manifest. Returns a one-line summary.
pub fn pack(exe: &Path, out: &Path) -> Result<String, String> {
    pack_with(exe, out, PIECE)
}

pub fn pack_with(exe: &Path, out: &Path, piece: usize) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(exe).map_err(|e| format!("can't read {}: {e}", exe.display()))?;
    if bytes.is_empty() {
        return Err("that file is empty".into());
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for old in std::fs::read_dir(out).map_err(|e| e.to_string())?.flatten() {
        let n = old.file_name().to_string_lossy().to_string();
        if n == "manifest.json" || (n.starts_with("part-") && n.ends_with(".bin")) {
            let _ = std::fs::remove_file(old.path());
        }
    }
    let mut names = vec![];
    for (i, chunk) in bytes.chunks(piece).enumerate() {
        let name = format!("part-{}.bin", i + 1);
        std::fs::write(out.join(&name), chunk).map_err(|e| e.to_string())?;
        names.push(name);
    }
    let sha = hex::encode(Sha256::digest(&bytes));
    let file_name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Murmint Setup.exe".into());
    let manifest = serde_json::json!({ "name": file_name, "size": bytes.len(), "sha256": sha, "parts": names });
    std::fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&manifest).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(format!("packed {file_name}: {} bytes in {} pieces, sha256 {sha}", bytes.len(), names.len()))
}

/// Sends the packed installer as one download. False, without writing anything, when it is missing or damaged.
pub fn stream_packed<W: Write>(w: &mut W, dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("manifest.json")) else { return false };
    let Ok(m) = serde_json::from_str::<Value>(&text) else { return false };
    let (Some(name), Some(size), Some(parts)) = (m["name"].as_str(), m["size"].as_u64(), m["parts"].as_array()) else { return false };
    let mut files = vec![];
    for p in parts {
        let Some(p) = p.as_str().filter(|p| !p.contains('/') && !p.contains('\\') && !p.contains("..")) else { return false };
        files.push(dir.join(p));
    }
    let total: u64 = files.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).sum();
    if files.is_empty() || total != size {
        eprintln!("[installer] the packed installer is incomplete ({total} of {size} bytes)");
        return false;
    }
    let safe = name.replace('"', "");
    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/vnd.microsoft.portable-executable\r\nContent-Length: {size}\r\nContent-Disposition: attachment; filename=\"{safe}\"\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n");
    if w.write_all(head.as_bytes()).is_err() {
        return true;
    }
    for f in files {
        let Ok(mut file) = std::fs::File::open(&f) else { return true };
        if std::io::copy(&mut file, w).is_err() {
            return true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("murmint-pack-{tag}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_packed_installer_comes_back_byte_for_byte() {
        let d = tmp("a");
        let exe = d.join("Murmint Setup 9.9.9.exe");
        let original: Vec<u8> = (0..250_000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(&exe, &original).unwrap();
        let out = d.join("installer-files");
        let summary = pack_with(&exe, &out, 100_000).unwrap();
        assert!(summary.contains("3 pieces"), "{summary}");
        let mut sent = vec![];
        assert!(stream_packed(&mut sent, &out));
        let split = sent.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let (head, body) = sent.split_at(split);
        let head = String::from_utf8_lossy(head);
        assert!(head.contains("Content-Length: 250000") && head.contains("filename=\"Murmint Setup 9.9.9.exe\""), "{head}");
        assert_eq!(body, &original[..]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_or_damaged_installer_sends_nothing() {
        let d = tmp("b");
        let mut sent = vec![];
        assert!(!stream_packed(&mut sent, &d), "no manifest");
        let exe = d.join("Murmint Setup 1.exe");
        std::fs::write(&exe, vec![1u8; 5000]).unwrap();
        let out = d.join("files");
        pack_with(&exe, &out, 2000).unwrap();
        std::fs::remove_file(out.join("part-2.bin")).unwrap();
        assert!(!stream_packed(&mut sent, &out), "a piece is missing");
        assert!(sent.is_empty(), "nothing is written before the checks pass");
        std::fs::write(out.join("manifest.json"), r#"{"name":"x.exe","size":1,"parts":["../secret"]}"#).unwrap();
        assert!(!stream_packed(&mut sent, &out), "pieces can't point outside the folder");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn packing_again_replaces_the_old_pieces() {
        let d = tmp("c");
        let out = d.join("out");
        let big = d.join("a.exe");
        std::fs::write(&big, vec![9u8; 9000]).unwrap();
        pack_with(&big, &out, 2000).unwrap();
        assert!(out.join("part-5.bin").exists());
        let small = d.join("b.exe");
        std::fs::write(&small, vec![8u8; 1500]).unwrap();
        pack_with(&small, &out, 2000).unwrap();
        assert!(!out.join("part-2.bin").exists(), "old pieces are removed");
        let _ = std::fs::remove_dir_all(&d);
    }
}
