//! Pictures from the web, kept (the developer, 2026-10-29: "remote imagelar cachelənsin"): a
//! person's picture (`crate::avatars`) and an image a comment, a description or a document shows
//! are fetched once, through `curl` as the rest, into a shelf of the app's folder (named by the
//! address), and read from there from then on — after a restart too. An upload never changes
//! (GitLab's `/uploads/`, GitHub's attachments) and is kept as it is; another image (a badge) is
//! drawn as kept and asked again once a day. A GitLab upload written in a comment of the project
//! is asked through the API with the host's token (`crate::mr::upload`): a private project's
//! uploads need sign-in, and the token goes to that project's host only, never to an address a
//! comment names. One that could not be had is not asked for again for a week (`<name>.none`);
//! images not written for 60 days are swept.

use crate::git::WebRemote;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Once};
use std::time::Duration;

/// Where a picture is kept: people's pictures apart from the images pages show.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Avatars,
    Images,
}

impl Shelf {
    fn dir(self) -> Option<PathBuf> {
        let name = match self {
            Self::Avatars => "avatars",
            Self::Images => "images",
        };
        Some(crate::platform::data_dir()?.join(name))
    }
}

/// A picture and its size in pixels, when it could be read.
pub type Picture = (Arc<gpui::Image>, Option<(u32, u32)>);

/// Each address's file once it is in; None while it is being fetched or could not be.
static FILES: LazyLock<Mutex<HashMap<String, Option<PathBuf>>>> = LazyLock::new(Default::default);
/// The pictures read, by file: read once, drawn as often as wanted.
static IMAGES: LazyLock<Mutex<HashMap<PathBuf, Option<Picture>>>> = LazyLock::new(Default::default);
/// A picture came in since the window last looked.
static FRESH: AtomicBool = AtomicBool::new(false);
static SWEPT: Once = Once::new();

/// How long a picture that could not be had is not asked for again.
const RETRY: Duration = Duration::from_secs(7 * 24 * 3600);
/// How long an image that may change (not an upload) is drawn before it is asked for again.
const REFRESH: Duration = Duration::from_secs(24 * 3600);
/// How long an image is kept without being written again.
const KEEP: Duration = Duration::from_secs(60 * 24 * 3600);

/// The image at `url` (a web address) once it is in; the first ask fetches it.
pub fn image(url: &str) -> Option<Picture> {
    if !is_web(url) {
        return None;
    }
    want(url, Shelf::Images, None);
    get(url)
}

/// An upload of `project` (GitLab's `https://host/group/app`) as a comment there writes it,
/// `/uploads/<secret>/<file>`, once it is in; the first ask fetches it with the host's token.
pub fn upload(project: &str, src: &str) -> Option<Picture> {
    let url = format!("{project}{src}");
    let Some(path) = upload_path(src) else {
        return image(&url);
    };
    want(
        &url,
        Shelf::Images,
        Some((project.to_owned(), path.to_owned())),
    );
    get(&url)
}

/// The `<secret>/<file>` of a GitLab upload as a comment writes it: the API is asked for that
/// upload and nothing else.
fn upload_path(src: &str) -> Option<&str> {
    let path = src.strip_prefix("/uploads/")?;
    let (secret, file) = path.split_once('/')?;
    let plain = !secret.is_empty()
        && secret.chars().all(|c| c.is_ascii_hexdigit())
        && !file.is_empty()
        && !file.contains("..")
        && !file.contains(['?', '#']);
    plain.then_some(path)
}

/// Starts fetching `url` onto `shelf` unless it is known; a GitLab upload (`project`, the path after
/// `/uploads/`) is asked through the API first.
pub fn want(url: &str, shelf: Shelf, upload: Option<(String, String)>) {
    // A test fetches nothing: no folder made, no network asked.
    if cfg!(test) {
        return;
    }
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    if files.contains_key(url) {
        return;
    }
    files.insert(url.to_owned(), None);
    let Some(dir) = shelf.dir() else {
        return;
    };
    if shelf == Shelf::Images {
        let dir = dir.clone();
        SWEPT.call_once(|| {
            std::thread::spawn(move || sweep(&dir));
        });
    }
    let stem = dir.join(sha1_smol::Sha1::from(url).digest().to_string());
    // Fetched before: it is there. One that may change is asked again once a day, and one about to
    // be swept, the kept one drawn meanwhile.
    if let Some(file) = kept(&stem) {
        let changes = shelf == Shelf::Images && !is_upload(url);
        let stale = age(&file) > Some(if changes { REFRESH } else { KEEP });
        files.insert(url.to_owned(), Some(file));
        if stale {
            spawn(url, stem, upload);
        }
        return;
    }
    // Not to be had a short while ago: not asked for again yet.
    if age(&stem.with_extension("none")).is_some_and(|a| a < RETRY) {
        return;
    }
    spawn(url, stem, upload);
}

fn spawn(url: &str, stem: PathBuf, upload: Option<(String, String)>) {
    let url = url.to_owned();
    std::thread::spawn(move || {
        if let Some(file) = fetch(&url, &stem, upload) {
            IMAGES
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&file);
            FILES
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(url, Some(file));
            FRESH.store(true, Ordering::Relaxed);
        }
    });
}

/// The picture at `url`, once it is in.
pub fn get(url: &str) -> Option<Picture> {
    let file = FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(url)
        .cloned()
        .flatten()?;
    IMAGES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(file.clone())
        .or_insert_with(|| read(&file))
        .clone()
}

/// Whether a picture came in since the last ask: the window is drawn again.
pub fn take_fresh() -> bool {
    FRESH.swap(false, Ordering::Relaxed)
}

fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// An address whose image never changes: a GitLab or GitHub upload.
fn is_upload(url: &str) -> bool {
    url.contains("/uploads/")
        || url.contains("/user-attachments/")
        || url.contains("user-images.githubusercontent.com/")
}

/// How long ago `file` was written; None when it is not there.
fn age(file: &Path) -> Option<Duration> {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
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

/// The file kept beside `stem`, of whichever kind.
fn kept(stem: &Path) -> Option<PathBuf> {
    KINDS
        .iter()
        .map(|k| stem.with_extension(k))
        .find(|f| f.exists())
}

/// Fetches `url` beside `stem`, then names it by its kind, so a half-written one is never shown.
fn fetch(url: &str, stem: &Path, upload: Option<(String, String)>) -> Option<PathBuf> {
    std::fs::create_dir_all(stem.parent()?).ok()?;
    let part = stem.with_extension("part");
    let none = stem.with_extension("none");
    // Through the API first; a GitLab without the call (before 17.4) or a token it refused: the
    // address itself, as for a public project.
    let by_api = upload
        .and_then(|(project, path)| Some((WebRemote::parse(&project).filter(|r| !r.github)?, path)))
        .is_some_and(|(remote, path)| crate::mr::upload(&remote, &path, &part));
    if !by_api {
        let status = std::process::Command::new("curl")
            .args([
                "-sSfL",
                "--max-time",
                "20",
                "--max-filesize",
                "25000000",
                "-o",
            ])
            .arg(&part)
            .arg(url)
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        // The host said no (curl's 22: 404 for no Gravatar, 401 behind sign-in): not again for a
        // while. A host not reached (the VPN off) is asked again next time.
        if status.code() == Some(22) {
            let _ = std::fs::remove_file(&part);
            let _ = std::fs::write(&none, "");
            return None;
        }
        if !status.success() {
            let _ = std::fs::remove_file(&part);
            return None;
        }
    }
    let head = std::fs::read(&part).ok()?;
    let Some(kind) = kind_of(&head[..head.len().min(512)]) else {
        let _ = std::fs::remove_file(&part);
        let _ = std::fs::write(&none, "");
        return None;
    };
    // Asked again, it may come back as another kind: the one kept before goes.
    if let Some(old) = kept(stem) {
        let _ = std::fs::remove_file(old);
    }
    let file = stem.with_extension(kind);
    std::fs::rename(&part, &file).ok()?;
    Some(file)
}

/// The picture in `file`, with its size.
fn read(file: &Path) -> Option<Picture> {
    use gpui::ImageFormat;
    let format = match file.extension()?.to_str()? {
        "png" => ImageFormat::Png,
        "jpg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "svg" => ImageFormat::Svg,
        _ => return None,
    };
    let bytes = std::fs::read(file).ok()?;
    let size = match format {
        ImageFormat::Svg => crate::git::svg_size(&String::from_utf8_lossy(&bytes)),
        _ => imagesize::blob_size(&bytes)
            .ok()
            .map(|s| (s.width as u32, s.height as u32)),
    };
    Some((Arc::new(gpui::Image::from_bytes(format, bytes)), size))
}

/// Removes what `dir` has kept without writing it again for `KEEP`.
fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if age(&path).is_some_and(|a| a > KEEP) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uploads_are_kept_and_other_images_asked_again() {
        assert!(is_upload(
            "https://gitlab.x.io/group/app/uploads/0123abcd/shot.png"
        ));
        assert!(is_upload(
            "https://github.com/user-attachments/assets/1f2e-3d4c"
        ));
        assert!(!is_upload(
            "https://img.shields.io/badge/build-passing-green"
        ));
        assert!(image("docs/logo.png").is_none());
    }

    #[test]
    fn only_an_upload_is_asked_of_the_api() {
        assert_eq!(
            upload_path("/uploads/0a1b2c/shot one.png"),
            Some("0a1b2c/shot one.png")
        );
        assert_eq!(upload_path("/uploads/zz/../../user"), None);
        assert_eq!(upload_path("/uploads/0a1b/../../user"), None);
        assert_eq!(upload_path("/uploads/0a1b/a.png?x=1"), None);
        assert_eq!(upload_path("/elsewhere/a.png"), None);
    }

    #[test]
    fn a_kind_is_read_from_the_bytes() {
        assert_eq!(kind_of(&[0x89, b'P', b'N', b'G', 0, 0]), Some("png"));
        assert_eq!(
            kind_of(b"<?xml version=\"1.0\"?><svg width=\"9\">"),
            Some("svg")
        );
        assert_eq!(kind_of(b"<!doctype html><title>Sign in</title>"), None);
    }
}
