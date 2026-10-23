//! The reviewer's own names and colours (the developer, 2026-10-23): a project named apart from
//! its folder — two `compose-multiplatform`s under different parents read the same — with a hue
//! of its own on the colour wheel, and a chat named apart from its first message. Kept in the data
//! folder's `labels.json`, read again when another window changed it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// A project's own name and hue; empty, its folder's name and the colour it is given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Degrees on the colour wheel, 0–360.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hue: Option<f32>,
}

impl Project {
    fn is_empty(&self) -> bool {
        self.name.is_empty() && self.hue.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Labels {
    /// By the project's folder.
    #[serde(default)]
    projects: BTreeMap<String, Project>,
    /// By the chat's session.
    #[serde(default)]
    chats: BTreeMap<String, String>,
}

/// The labels as last read: when looked at, the file's time then, and them.
static READ: Mutex<Option<(Instant, Option<SystemTime>, Labels)>> = Mutex::new(None);

fn file() -> Option<PathBuf> {
    Some(crate::platform::data_dir()?.join("labels.json"))
}

/// A folder as a key: the same folder with or without its last slash.
fn key(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches('/').to_owned()
}

/// The labels, read again at most each second, and then only when the file changed.
fn labels() -> Labels {
    let mut read = READ.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, _, labels)) = read.as_ref()
        && at.elapsed() < Duration::from_secs(1)
    {
        return labels.clone();
    }
    let file = file();
    let stamp = file
        .as_ref()
        .and_then(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok());
    if let Some((at, kept, labels)) = read.as_mut()
        && *kept == stamp
    {
        *at = Instant::now();
        return labels.clone();
    }
    let labels: Labels = file
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    *read = Some((Instant::now(), stamp, labels.clone()));
    labels
}

fn save(labels: Labels) {
    let Some(file) = file() else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::to_string_pretty(&labels).unwrap_or_default();
    let tmp = file.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &file).is_ok() {
        let stamp = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        *READ.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), stamp, labels));
    }
}

/// Project `path`'s own name and hue.
pub fn project(path: &Path) -> Project {
    labels()
        .projects
        .get(&key(path))
        .cloned()
        .unwrap_or_default()
}

/// Project `path` as it is shown: its own name, else its folder's.
pub fn project_name(path: &Path) -> String {
    let own = project(path).name;
    if !own.is_empty() {
        return own;
    }
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Keeps project `path`'s name and hue; both empty, it has none of its own again.
pub fn set_project(path: &Path, project: Project) {
    let mut labels = labels();
    let project = Project {
        name: project.name.trim().to_owned(),
        hue: project.hue.map(|h| h.rem_euclid(360.)),
    };
    if project.is_empty() {
        labels.projects.remove(&key(path));
    } else {
        labels.projects.insert(key(path), project);
    }
    save(labels);
}

/// Chat `session`'s own name.
pub fn chat(session: &str) -> Option<String> {
    labels().chats.get(session).cloned()
}

/// Keeps chat `session`'s name; empty, it is named by its first message again.
pub fn set_chat(session: &str, name: &str) {
    let mut labels = labels();
    let name = name.trim();
    if name.is_empty() {
        labels.chats.remove(session);
    } else {
        labels.chats.insert(session.to_owned(), name.to_owned());
    }
    save(labels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_one_key_with_or_without_its_slash() {
        assert_eq!(key(Path::new("/p/app/")), key(Path::new("/p/app")));
    }

    #[test]
    fn labels_round_trip_and_leave_out_what_is_not_set() {
        let mut labels = Labels::default();
        labels.projects.insert(
            "/p/app".into(),
            Project {
                name: "App (Act2Act)".into(),
                hue: Some(210.),
            },
        );
        labels.projects.insert(
            "/q/app".into(),
            Project {
                name: String::new(),
                hue: Some(30.),
            },
        );
        labels.chats.insert("s1".into(), "Paging".into());
        let text = serde_json::to_string(&labels).unwrap();
        assert!(!text.contains("\"name\":\"\""));
        assert_eq!(serde_json::from_str::<Labels>(&text).unwrap(), labels);
        assert_eq!(
            serde_json::from_str::<Labels>("{}").unwrap(),
            Labels::default()
        );
    }
}
