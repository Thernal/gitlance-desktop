//! Bookmarks for the first pass (F3): a mark on a line, a list of them (⌘F3), a click on the mark
//! takes it off. For "look at this again" without writing a comment. Kept per commit, range or
//! merge request on this Mac. Designed in `../Design/mockups/ide-ideas/a-ideas.html` (8).

use super::Workspace;
use crate::storage;
use gpui::Context;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub path: String,
    pub line: u32,
    /// The line as it read when it was marked, shown in the list.
    pub text: String,
}

#[derive(Default)]
pub struct Store {
    marks: HashMap<String, Vec<Bookmark>>,
}

impl Store {
    pub fn load() -> Self {
        Self::parse(&storage::read("bookmarks.txt"))
    }

    fn parse(text: &str) -> Self {
        let mut marks: HashMap<String, Vec<Bookmark>> = HashMap::new();
        for line in text.lines() {
            let mut parts = line.splitn(4, '\t');
            if let (Some(scope), Some(path), Some(n), Some(text)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
                && let Ok(n) = n.parse()
            {
                marks.entry(scope.to_owned()).or_default().push(Bookmark {
                    path: path.to_owned(),
                    line: n,
                    text: text.to_owned(),
                });
            }
        }
        Self { marks }
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (scope, marks) in &self.marks {
            for m in marks {
                out.push_str(&format!(
                    "{scope}\t{}\t{}\t{}\n",
                    m.path,
                    m.line,
                    m.text.replace(['\t', '\n'], " ")
                ));
            }
        }
        out
    }

    pub fn of(&self, scope: &str) -> &[Bookmark] {
        self.marks.get(scope).map_or(&[], Vec::as_slice)
    }

    /// Marks the line, or takes the mark off; whether it is marked now.
    fn toggle(&mut self, scope: &str, mark: Bookmark) -> bool {
        let list = self.marks.entry(scope.to_owned()).or_default();
        let at = list
            .iter()
            .position(|m| m.path == mark.path && m.line == mark.line);
        let now = at.is_none();
        match at {
            Some(i) => {
                list.remove(i);
            }
            None => list.push(mark),
        }
        if list.is_empty() {
            self.marks.remove(scope);
        }
        storage::save("bookmarks.txt", self.render());
        now
    }
}

impl Workspace {
    /// The marked lines of the open file (new side).
    pub(super) fn bookmarks_here(&self) -> Vec<u32> {
        let (Some(scope), Some(path)) = (self.review_scope.as_deref(), self.current_path()) else {
            return Vec::new();
        };
        self.bookmarks
            .of(scope)
            .iter()
            .filter(|m| m.path == path)
            .map(|m| m.line)
            .collect()
    }

    /// F3: the line under the last click gets a mark, or loses it.
    pub(super) fn toggle_bookmark(&mut self, cx: &mut Context<Self>) {
        let line = self
            .sel
            .filter(|s| !s.old)
            .map(|s| s.ends().0.line)
            .or_else(|| self.bookmark_fallback_line());
        if let Some(line) = line {
            self.toggle_bookmark_at(line, cx);
        }
    }

    /// With nothing clicked: the first changed line shown.
    fn bookmark_fallback_line(&self) -> Option<u32> {
        let row = *self.changes.first()?;
        self.new_line_of_row(row)
    }

    pub(super) fn toggle_bookmark_at(&mut self, line: u32, cx: &mut Context<Self>) {
        let (Some(scope), Some(path), Some(data)) = (
            self.review_scope.clone(),
            self.current_path(),
            self.data.clone(),
        ) else {
            return;
        };
        if line == 0 || line > data.side(false).count() {
            return;
        }
        let text = data.side(false).line(line).0.trim().to_owned();
        self.bookmarks.toggle(&scope, Bookmark { path, line, text });
        self.relayout_notes();
        cx.notify();
    }

    /// The marks of the open diff, for the list.
    pub(super) fn bookmark_list(&self) -> Vec<Bookmark> {
        self.review_scope
            .as_deref()
            .map(|s| self.bookmarks.of(s).to_vec())
            .unwrap_or_default()
    }

    fn relayout_notes(&mut self) {
        self.notes = self.line_notes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_round_trip_and_toggle() {
        let mut store = Store::default();
        let mark = Bookmark {
            path: "a.rs".into(),
            line: 7,
            text: "let x = 1;".into(),
        };
        store
            .marks
            .entry("s".into())
            .or_default()
            .push(mark.clone());
        let again = Store::parse(&store.render());
        assert_eq!(again.of("s"), [mark]);
        assert!(again.of("other").is_empty());
    }
}
