//! Recent places (⌘E): the files, commits and merge requests opened lately, newest first, in the
//! palette. Kept for the session; ⌘[ and ⌘] stay for stepping back and forward. Designed in
//! `../Design/mockups/ide-ideas/a-ideas.html` (9).

use super::{Workspace, format};
use git2::Oid;
use gpui::Context;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Key {
    File(String),
    Commit(Oid),
    Request(u64),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub key: Key,
    pub label: String,
    pub detail: String,
}

const KEEP: usize = 30;

impl Workspace {
    /// Puts a place first in the list (the same place twice is one entry).
    pub(super) fn remember_place(&mut self, key: Key, label: String, detail: String) {
        self.recents.retain(|e| e.key != key);
        self.recents.insert(0, Entry { key, label, detail });
        self.recents.truncate(KEEP);
    }

    /// Goes back to a place; says so when it is not in what is open now.
    pub(super) fn open_recent(&mut self, key: Key, cx: &mut Context<Self>) {
        match key {
            Key::File(path) => {
                let ix = self
                    .diff
                    .as_ref()
                    .and_then(|d| d.files.iter().position(|f| f.path() == path));
                match ix {
                    Some(ix) => self.select_file(ix, cx),
                    None => {
                        self.error = Some(format!("{path} is not in the open diff.").into());
                        cx.notify();
                    }
                }
            }
            Key::Commit(id) => match self.commits.iter().position(|c| c.id == id) {
                Some(ix) => self.select_commit(ix, cx),
                None => {
                    self.error = Some(
                        format!("{} is not in the open list of commits.", format::short(id)).into(),
                    );
                    cx.notify();
                }
            },
            Key::Request(iid) => self.select_request(iid, cx),
        }
    }
}
