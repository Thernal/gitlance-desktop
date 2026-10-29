//! An agent's notes on the lines of its changes (#151, `crate::notes`): drawn under their lines in
//! the agent's colour, apart from the reviewer's comments, with the summary over the diff, while
//! the diff shows the changes they are on — the working tree, or a head the agent named (its
//! branch's pages, a comparison or a request with that head). Answer opens a comment for the agent
//! on the note's line; the reviewer dismisses one, or all of them at once.

use super::discussion::{self, Kind};
use super::nav::Page;
use super::rows::FileData;
use super::toast::Act;
use super::{Header, Workspace, markdown, px, theme};
use crate::agents::{Event, What};
use crate::notes::{Note, Notes, On};
use crate::review::Place;
use gpui::{Context, FontWeight, WeakEntity, div, prelude::*};

impl Workspace {
    /// An agent's notes on its changes (`What::Notes`) come in: kept, its earlier ones on the
    /// same changes replaced; the changes opened under Branches too when it asked, else a toast
    /// offers to show them.
    pub(super) fn take_agent_notes(&mut self, event: Event, cx: &mut Context<Self>) {
        let What::Notes {
            changes,
            summary,
            notes,
            open,
        } = event.what
        else {
            return;
        };
        let Some((page, unstaged)) = super::agent_events::changes_page(&changes) else {
            return;
        };
        let on = match *changes {
            What::Compare { head, head_id, .. } => On::Head {
                name: head,
                commit: head_id,
            },
            _ => On::WorkingTree,
        };
        let agent = event.agent;
        let count = notes.len();
        crate::notes::take(
            &mut self.agent_notes,
            Notes {
                uid: event.uid,
                agent: agent.clone(),
                at: event.at,
                on,
                summary,
                notes,
            },
        );
        self.save_agent_notes();
        self.map_agent_notes(cx);
        self.refresh_rows();
        if open {
            return self.agent_changes(agent, page, unstaged, count, cx);
        }
        let what = super::agent_events::changes_said(&page);
        if let Some(item) = page.item().filter(|item| self.nav.opened(item).is_some()) {
            let notes = super::plural(count, "note");
            self.call_reviewer(item, format!("{agent} left {notes} on it"));
        }
        let act = Act::Notes {
            page,
            unstaged,
            agent: agent.clone(),
        };
        let say = move |_| {
            let notes = super::plural(count, "note");
            format!("{agent} left {notes} on its changes: {what}")
        };
        self.toast_act(act, 1, say, cx);
    }

    /// Show on a toast of notes: their changes open under Branches, on screen.
    pub(super) fn show_agent_notes(
        &mut self,
        page: Page,
        unstaged: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = page.item() else {
            return;
        };
        self.open_quietly(page);
        self.save_nav();
        self.show_changes(&item, unstaged, cx);
    }

    pub(super) fn save_agent_notes(&self) {
        if let Some(project) = &self.project {
            crate::notes::save(project, &self.agent_notes);
        }
    }

    /// The notes on the changes the diff shows: the working tree's on the working tree; a head's
    /// where the diff's head is its commit or bears its name.
    fn notes_shown(&self) -> Vec<&Notes> {
        let Some(header) = self.diff.as_ref().map(|d| &d.header) else {
            return Vec::new();
        };
        let head = self.shown_head().map(|h| h.to_string());
        let named = match header {
            Header::Compare { head, .. } => Some(head.0.clone()),
            _ => None,
        }
        .or_else(|| self.page_branch());
        self.agent_notes
            .iter()
            .filter(|n| match (&n.on, header) {
                (On::WorkingTree, Header::WorkingTree { .. }) => true,
                (_, Header::WorkingTree { .. }) | (On::WorkingTree, _) => false,
                (On::Head { name, commit }, _) => {
                    head.as_ref() == Some(commit) || named.as_ref() == Some(name)
                }
            })
            .collect()
    }

    /// The notes shown on lines of file `path`, built as `data`: (id, line on the new side).
    pub(super) fn notes_in(&self, path: &str, data: &FileData) -> Vec<(u64, u32)> {
        let side = data.side(false);
        self.notes_shown()
            .into_iter()
            .flat_map(|n| n.notes.iter())
            .filter(|n| n.path == path)
            .filter_map(|n| {
                match crate::review::find_line(n.line, &n.code, side.count(), |l| side.line(l).0) {
                    Place::Line(line) => Some((n.id, line)),
                    Place::Outdated => self.note_lines.get(&n.id).map(|&line| (n.id, line)),
                }
            })
            .collect()
    }

    /// Follows the line of every note on a head the diff has moved on from, from the commit it was
    /// written on to the one shown, in the background (#46).
    pub(super) fn map_agent_notes(&mut self, cx: &mut Context<Self>) {
        self.note_lines.clear();
        let (Some(root), Some(head)) = (self.root.clone(), self.shown_head()) else {
            return;
        };
        let asks: Vec<(u64, String, String, u32)> = self
            .notes_shown()
            .into_iter()
            .filter_map(|n| match &n.on {
                On::Head { commit, .. } if !head.to_string().starts_with(commit.as_str()) => Some(
                    n.notes
                        .iter()
                        .map(move |x| (x.id, commit.clone(), x.path.clone(), x.line)),
                ),
                _ => None,
            })
            .flatten()
            .collect();
        if asks.is_empty() {
            return;
        }
        self.note_task = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let Ok(repo) = crate::git::Repo::open(&root) else {
                        return std::collections::HashMap::new();
                    };
                    asks.into_iter()
                        .filter_map(|(id, commit, path, line)| {
                            let from = git2::Oid::from_str(&commit).ok()?;
                            match repo.follow_line(from, head, &path, line) {
                                crate::git::Fate::Same(n) | crate::git::Fate::Changed(n, _) => {
                                    Some((id, n))
                                }
                                crate::git::Fate::Gone => None,
                            }
                        })
                        .collect()
                })
                .await;
            this.update(cx, |this, cx| {
                this.note_lines = found;
                this.refresh_rows();
                cx.notify();
            })
            .ok();
        }));
    }

    /// Note `id`, and the notes it is one of.
    fn agent_note(&self, id: u64) -> Option<(&Notes, &Note)> {
        self.agent_notes
            .iter()
            .find_map(|n| Some((n, n.notes.iter().find(|x| x.id == id)?)))
    }

    /// Answer on a note: a comment for the agent on its line, as comments for it are written; it
    /// carries the note, so the agent reads what it answers.
    pub(super) fn answer_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some((path, body)) = self
            .agent_note(id)
            .map(|(_, n)| (n.path.clone(), n.body.clone()))
        else {
            return;
        };
        // In one scroll of all the files, its file becomes the one the keyboard works in.
        if let Some(file) = self
            .diff
            .as_ref()
            .and_then(|d| d.files.iter().position(|f| f.path() == path))
            && self.stack.is_some()
        {
            self.activate_file(file, cx);
        }
        let Some(line) = self
            .data
            .as_ref()
            .and_then(|data| self.notes_in(&path, data).into_iter().find(|n| n.0 == id))
            .map(|(_, line)| line)
        else {
            return;
        };
        self.start_comment(false, line, cx);
        if let Some(compose) = &mut self.compose {
            compose.target = super::comments::Target::Agent;
            compose.answers = Some(body);
        }
        cx.notify();
    }

    /// Answer pressed on the first note on the file shown (the snapshot checks).
    #[cfg(feature = "snapshot")]
    pub(super) fn snapshot_answer_note(&mut self, cx: &mut Context<Self>) {
        let path = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| f.path().to_owned());
        let first = match (&self.data, path) {
            (Some(data), Some(path)) => self.notes_in(&path, data).first().map(|&(id, _)| id),
            _ => None,
        };
        if let Some(id) = first {
            self.answer_note(id, cx);
        }
    }

    /// Dismisses note `id`; its notes go with their last.
    pub(super) fn dismiss_note(&mut self, id: u64, cx: &mut Context<Self>) {
        for n in &mut self.agent_notes {
            n.notes.retain(|x| x.id != id);
        }
        self.agent_notes
            .retain(|n| !n.notes.is_empty() || !n.summary.is_empty());
        self.save_agent_notes();
        self.refresh_rows();
        cx.notify();
    }

    /// Dismiss all on a summary: those notes and their summary go.
    pub(super) fn dismiss_agent_notes(&mut self, uid: &str, cx: &mut Context<Self>) {
        self.agent_notes.retain(|n| n.uid != uid);
        self.save_agent_notes();
        self.refresh_rows();
        cx.notify();
    }

    /// A note under its line: the agent's mark and name in its colour, Answer and Dismiss, the
    /// note.
    pub(super) fn render_agent_note(
        &self,
        id: u64,
        indent: f32,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        let Some((notes, note)) = self.agent_note(id) else {
            return div().into_any_element();
        };
        let (letter, hue) = self.agent_mark(&notes.agent);
        let (answer, dismiss, selecting) = (this.clone(), this.clone(), self.selecting(this));
        let bar = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(super::agent::chat_badge(letter, hue, false))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(notes.agent.clone()),
            )
            .child(
                super::hover::cut("note-hint", "· a note on its change")
                    .flex_1()
                    .text_color(theme::muted()),
            )
            .child(
                discussion::icon_button(
                    ("note-answer", id as usize),
                    "message",
                    "Answer",
                    None,
                    Some("A comment for the agent on this line"),
                    Kind::Plain,
                    self.tight,
                )
                .h(px(24.))
                .px(px(8.))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    answer.update(cx, |this, cx| this.answer_note(id, cx)).ok();
                }),
            )
            .child(
                discussion::icon_button(
                    ("note-dismiss", id as usize),
                    "x",
                    "Dismiss",
                    None,
                    None,
                    Kind::Quiet,
                    true,
                )
                .h(px(24.))
                .px(px(6.))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    dismiss
                        .update(cx, |this, cx| this.dismiss_note(id, cx))
                        .ok();
                }),
            );
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(6.))
            .child(note_frame(hue).child(bar).child(markdown::body(
                &note.body,
                self.settings.rich_comments,
                format!("md-agent-note-{id}"),
                None,
                Some(selecting),
            )))
            .into_any_element()
    }

    /// Over the diff, the summary of each agent's notes on these changes: its mark, how many notes
    /// on how many files, the summary, Dismiss all.
    pub(super) fn render_notes_summary(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let shown = self.notes_shown();
        if shown.is_empty() {
            return None;
        }
        let this = cx.entity().downgrade();
        let cards = shown.into_iter().map(|n| {
            let (letter, hue) = self.agent_mark(&n.agent);
            let mut files: Vec<&str> = n.notes.iter().map(|x| x.path.as_str()).collect();
            files.sort_unstable();
            files.dedup();
            let count = match n.notes.len() {
                0 => String::new(),
                notes => format!(
                    "· {} on {}",
                    super::plural(notes, "note"),
                    super::plural(files.len(), "file")
                ),
            };
            let (dismiss, uid) = (this.clone(), n.uid.clone());
            note_frame(hue)
                .max_w_full()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(super::agent::chat_badge(letter, hue, false))
                        .child(
                            div()
                                .flex_none()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{}'s notes", n.agent)),
                        )
                        .child(
                            super::hover::cut("notes-count", count)
                                .flex_1()
                                .text_color(theme::muted()),
                        )
                        .child(
                            discussion::button(
                                gpui::SharedString::from(format!("notes-dismiss-{}", n.uid)),
                                "Dismiss all",
                                None,
                                Kind::Quiet,
                            )
                            .h(px(24.))
                            .px(px(8.))
                            .on_click(move |_, _, cx| {
                                dismiss
                                    .update(cx, |this, cx| this.dismiss_agent_notes(&uid, cx))
                                    .ok();
                            }),
                        ),
                )
                // A long one scrolls in its card; the diff keeps the room.
                .children((!n.summary.is_empty()).then(|| {
                    div()
                        .id(gpui::SharedString::from(format!("notes-summary-{}", n.uid)))
                        .max_h(px(160.))
                        .overflow_y_scroll()
                        .child(markdown::body(
                            &n.summary,
                            self.settings.rich_comments,
                            format!("md-notes-summary-{}", n.uid),
                            None,
                            Some(self.selecting(this.clone())),
                        ))
                }))
        });
        Some(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(super::gap()))
                .pb(px(super::gap()))
                .children(cards)
                .into_any_element(),
        )
    }
}

/// A note's frame: a discussion's, its rail in the agent's colour.
fn note_frame(hue: gpui::Rgba) -> gpui::Div {
    const RAIL: f32 = 3.;
    div()
        .w_full()
        .max_w(px(720.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .rounded(px(10.))
        .bg(theme::panel())
        .border_l(px(RAIL))
        .border_color(hue)
        .pl(px(14. - RAIL))
        .pr(px(14.))
        .pt(px(8.))
        .pb(px(10.))
        .font_family(theme::ui_font())
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(theme::text())
}
