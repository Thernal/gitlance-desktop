//! The app's own log (#118): a line for every error the app showed or met, and a panic, kept in
//! the data folder's `log.txt` (its newest lines), so a report can carry what happened just before.

use std::io::Write;
use std::path::PathBuf;

/// The lines the file keeps; past twice this many it is cut back.
const KEEP: usize = 2_000;

fn file() -> Option<PathBuf> {
    crate::platform::data_dir().map(|d| d.join("log.txt"))
}

/// Adds `text` with the time, one line.
pub fn line(text: &str) {
    let Some(file) = file() else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let when = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let text = text.replace('\n', " ⏎ ");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    {
        let _ = writeln!(f, "{when} {text}");
    }
    // Cut back now and then, not on every line.
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > 400_000) {
        let lines = tail(KEEP);
        let _ = std::fs::write(&file, lines.join("\n") + "\n");
    }
}

/// The newest `n` lines, oldest first.
pub fn tail(n: usize) -> Vec<String> {
    let Some(text) = file().and_then(|f| std::fs::read_to_string(f).ok()) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

/// A panic goes in the log too, before the usual message.
pub fn catch_panics() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        line(&format!("panic: {info}"));
        before(info);
    }));
}
