//! Reads the GitHub releases list, as the Mac app does (`Sources/Updates.swift`). The request is
//! made by the app's updater; this part only reads the answer, so it is tested on any OS.

use base64::Engine;
use serde_json::Value;
use std::collections::BTreeMap;
use url::Url;

pub const FEED: &str = "https://api.github.com/repos/sayre4ux/TypeThru/releases?per_page=10";
pub const ASSET_SUFFIX: &str = "-windows.exe";

/// The public key built into both apps; the private key is in the release signer's keychain.
pub const PUBLIC_KEY: &str = "BXnQoapwfgyibBMFwhRrwwvxzlQxg8124OvklxqxoAI="; // gitleaks:allow (public key)

/// One version's notes, from CHANGELOG.md or a release on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogEntry {
    pub version: String,
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub page: Url,
    pub download: Option<Url>,
    pub signature: Option<Url>,
    /// What changed in every release after this copy, newest first, shown before Install.
    pub notes: Vec<ChangelogEntry>,
}

/// "v0.3.0-beta" → [0, 3, 0]
pub fn numbers(version: &str) -> Vec<u32> {
    version
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()
        .unwrap_or("")
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect()
}

pub fn normalized(version: &str) -> String {
    numbers(version).iter().map(u32::to_string).collect::<Vec<_>>().join(".")
}

/// "0.7.0" reads as "0.7", as the releases are named.
pub fn display(version: &str) -> &str {
    if version.split('.').count() > 2 { version.strip_suffix(".0").unwrap_or(version) } else { version }
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (a, b) = (numbers(candidate), numbers(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// HTTPS on github.com's default port, no user name, and a path under `prefix`. The path is
/// checked after the URL is parsed, so "/releases/../../x" cannot pass.
fn on_github(url: &Url, prefix: &str) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path().starts_with(prefix)
}

/// Only TypeThru's own release pages, so a button cannot open anything else.
pub fn is_release_page(page: &Url) -> bool {
    on_github(page, "/sayre4ux/TypeThru/releases/")
}

/// Only files attached to TypeThru's own releases.
pub fn is_release_file(file: &Url) -> bool {
    on_github(file, "/sayre4ux/TypeThru/releases/download/")
}

fn items(json: &str) -> Vec<serde_json::Map<String, Value>> {
    match serde_json::from_str(json) {
        Ok(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::Object(object) => Some(object),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn text<'a>(item: &'a serde_json::Map<String, Value>, name: &str) -> Option<&'a str> {
    item.get(name).and_then(Value::as_str)
}

fn is_draft(item: &serde_json::Map<String, Value>) -> bool {
    item.get("draft") == Some(&Value::Bool(true))
}

/// The release's tag and page, when it is a published release of this project.
fn release_of(item: &serde_json::Map<String, Value>) -> Option<(String, Url)> {
    if is_draft(item) {
        return None;
    }
    let version = normalized(text(item, "tag_name")?);
    let page = Url::parse(text(item, "html_url")?).ok()?;
    (!version.is_empty() && is_release_page(&page)).then_some((version, page))
}

/// The newest release of this project, with the Windows download and its signature when attached.
/// None when this copy is up to date.
pub fn newest(json: &str, current: &str) -> Option<Release> {
    let mut newest: Option<Release> = None;
    for item in items(json) {
        let Some((version, page)) = release_of(&item) else { continue };
        let mut files = BTreeMap::new();
        for asset in item.get("assets").and_then(Value::as_array).into_iter().flatten() {
            let Some(asset) = asset.as_object() else { continue };
            let (Some(name), Some(link)) = (text(asset, "name"), text(asset, "browser_download_url")) else { continue };
            if let Ok(file) = Url::parse(link)
                && is_release_file(&file)
            {
                files.insert(name.to_owned(), file);
            }
        }
        let exe = files.keys().find(|name| name.ends_with(ASSET_SUFFIX) && files.contains_key(&format!("{name}.sig"))).cloned();
        let (download, signature) = match exe {
            Some(name) => (files.get(&name).cloned(), files.get(&format!("{name}.sig")).cloned()),
            None => (None, None),
        };
        let release = Release { version, page, download, signature, notes: Vec::new() };
        if newest.as_ref().is_none_or(|n| is_newer(&release.version, &n.version)) {
            newest = Some(release);
        }
    }
    let mut newest = newest.filter(|n| is_newer(&n.version, current))?;
    newest.notes = notes(json, current);
    Some(newest)
}

/// The notes of every release after `current`, newest first. Only the bullets under New, Fixes,
/// and Security; install steps and checksums are left out.
pub fn notes(json: &str, current: &str) -> Vec<ChangelogEntry> {
    let mut entries: Vec<ChangelogEntry> = items(json)
        .iter()
        .filter_map(|item| {
            let (version, _) = release_of(item)?;
            let body = text(item, "body")?;
            if !is_newer(&version, current) {
                return None;
            }
            let lines = summary(body);
            (!lines.is_empty()).then(|| ChangelogEntry { title: display(&version).to_owned(), version, lines })
        })
        .collect();
    entries.sort_by(|a, b| {
        if is_newer(&a.version, &b.version) {
            std::cmp::Ordering::Less
        } else if is_newer(&b.version, &a.version) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    entries.truncate(10);
    entries
}

/// The `- ` bullets under New, Fixes, Security, or Changes headings, at most 20, each at most 400 characters.
pub fn summary(body: &str) -> Vec<String> {
    const KEPT: [&str; 5] = ["new", "what's new", "fixes", "security", "changes"];
    let mut keep = true;
    let mut lines = Vec::new();
    for raw in body.split('\n') {
        let line = raw.trim();
        if line.starts_with('#') {
            let heading = line.trim_start_matches('#').trim().to_lowercase();
            keep = KEPT.iter().any(|h| heading.starts_with(h));
        } else if keep && line.starts_with("- ") {
            lines.push(line.chars().take(400).collect());
            if lines.len() == 20 {
                break;
            }
        }
    }
    lines
}

/// The built-in public key.
pub fn public_key() -> [u8; 32] {
    let bytes = base64::engine::general_purpose::STANDARD.decode(PUBLIC_KEY).unwrap_or_default();
    bytes.try_into().unwrap_or([0; 32])
}

/// Whether `signature` (base64 text, as in the .sig file) signs `data` with `key`.
pub fn is_signed(data: &[u8], signature: &[u8], key: &[u8; 32]) -> bool {
    let Ok(text) = std::str::from_utf8(signature) else { return false };
    let compact: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(compact) else { return false };
    let Ok(raw) = <[u8; 64]>::try_from(raw.as_slice()) else { return false };
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(key) else { return false };
    // DECISION: strict verification rejects weak keys and malleable signatures; CryptoKit's signatures pass it.
    key.verify_strict(data, &ed25519_dalek::Signature::from_bytes(&raw)).is_ok()
}
