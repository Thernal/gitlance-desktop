//! People's pictures (the developer, 2026-10-23): the `avatar_url` GitLab and GitHub give with a
//! user is fetched once, through `curl` as the rest, into the app's folder (`avatars/`, named by
//! the address), and an avatar shows it; until it is in, and for one without, the initial on its
//! colour. Users are learnt from every answer the hosts give (`harvest`), by username and name.

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

/// Who has which picture: username and name, lower case → its address.
static WHO: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
/// Each address's file once it is in; None while it is being fetched or could not be.
static FILES: LazyLock<Mutex<HashMap<String, Option<PathBuf>>>> = LazyLock::new(Default::default);
/// A picture came in since the window last looked.
static FRESH: AtomicBool = AtomicBool::new(false);

/// Learns the users in an answer: any object with a username (`login` on GitHub) and an
/// `avatar_url`, however deep.
pub fn harvest(value: &Value) {
    match value {
        Value::Object(map) => {
            let url = map.get("avatar_url").and_then(Value::as_str).unwrap_or("");
            let login = ["username", "login"]
                .iter()
                .find_map(|k| map.get(*k).and_then(Value::as_str))
                .unwrap_or("");
            if !url.is_empty() && !login.is_empty() {
                let name = map.get("name").and_then(Value::as_str).unwrap_or("");
                learn(login, name, url);
            }
            map.values().for_each(harvest);
        }
        Value::Array(items) => items.iter().for_each(harvest),
        _ => {}
    }
}

fn learn(login: &str, name: &str, url: &str) {
    {
        let mut who = WHO.lock().unwrap_or_else(|e| e.into_inner());
        for key in [login, name].into_iter().filter(|k| !k.is_empty()) {
            who.insert(key.to_lowercase(), url.to_owned());
        }
    }
    // A test learns who is who and fetches nothing: no folder made, no network asked.
    if cfg!(test) {
        return;
    }
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    if files.contains_key(url) {
        return;
    }
    let Some(stem) = stem_of(url) else {
        files.insert(url.to_owned(), None);
        return;
    };
    // Fetched before: it is there.
    if let Some(file) = KINDS
        .iter()
        .map(|k| stem.with_extension(k))
        .find(|f| f.exists())
    {
        files.insert(url.to_owned(), Some(file));
        return;
    }
    files.insert(url.to_owned(), None);
    let url = url.to_owned();
    std::thread::spawn(move || {
        if let Some(file) = fetch(&url, &stem) {
            FILES
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(url, Some(file));
            FRESH.store(true, Ordering::Relaxed);
        }
    });
}

/// The kinds a picture is kept as, by what its bytes start with: the extension tells the image
/// reader how to read it.
const KINDS: [&str; 5] = ["png", "jpg", "gif", "webp", "svg"];

fn kind_of(head: &[u8]) -> Option<&'static str> {
    Some(match head {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [0xff, 0xd8, ..] => "jpg",
        [b'G', b'I', b'F', ..] => "gif",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => "webp",
        _ if String::from_utf8_lossy(head).contains("<svg") => "svg",
        _ => return None,
    })
}

/// Where the picture at `url` is kept, but its extension.
fn stem_of(url: &str) -> Option<PathBuf> {
    let dir = crate::platform::data_dir()?.join("avatars");
    Some(dir.join(sha1_smol::Sha1::from(url).digest().to_string()))
}

/// Fetches `url` beside `stem`, then names it by its kind, so a half-written one is never shown.
fn fetch(url: &str, stem: &std::path::Path) -> Option<PathBuf> {
    std::fs::create_dir_all(stem.parent()?).ok()?;
    let part = stem.with_extension("part");
    let ok = std::process::Command::new("curl")
        .args(["-sSfL", "--max-time", "15", "-o"])
        .arg(&part)
        .arg(url)
        .status()
        .is_ok_and(|s| s.success());
    let head = std::fs::read(&part).ok().filter(|_| ok)?;
    let Some(kind) = kind_of(&head[..head.len().min(512)]) else {
        let _ = std::fs::remove_file(&part);
        return None;
    };
    let file = stem.with_extension(kind);
    std::fs::rename(&part, &file).ok()?;
    Some(file)
}

/// The pictures read, by file: read once, drawn as often as wanted.
static IMAGES: LazyLock<Mutex<HashMap<PathBuf, Option<std::sync::Arc<gpui::Image>>>>> =
    LazyLock::new(Default::default);

/// The picture of `who` (a username or a name), once it is in.
pub fn picture(who: &str) -> Option<std::sync::Arc<gpui::Image>> {
    let url = WHO
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&who.to_lowercase())
        .cloned()?;
    let file = FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&url)
        .cloned()
        .flatten()?;
    IMAGES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(file.clone())
        .or_insert_with(|| {
            use gpui::ImageFormat;
            let format = match file.extension()?.to_str()? {
                "png" => ImageFormat::Png,
                "jpg" => ImageFormat::Jpeg,
                "gif" => ImageFormat::Gif,
                "webp" => ImageFormat::Webp,
                "svg" => ImageFormat::Svg,
                _ => return None,
            };
            let bytes = std::fs::read(&file).ok()?;
            Some(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
        })
        .clone()
}

/// Whether a picture came in since the last ask: the window is drawn again.
pub fn take_fresh() -> bool {
    FRESH.swap(false, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn users_are_learnt_however_deep() {
        let answer = serde_json::json!([{
            "author": { "username": "ember-avatars-test", "name": "Ember Avatars", "avatar_url": "" },
            "reviewers": [{ "login": "wren-avatars-test", "avatar_url": "x:not-a-url" }]
        }]);
        harvest(&answer);
        let who = WHO.lock().unwrap();
        assert!(!who.contains_key("ember-avatars-test"));
        assert_eq!(
            who.get("wren-avatars-test").map(String::as_str),
            Some("x:not-a-url")
        );
    }
}
