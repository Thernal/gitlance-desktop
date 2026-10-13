//! Notifications for the open repository's GitLab project: the user's to-dos (mentions, review
//! requests, being addressed) and new comments where they take part — in discussions they wrote
//! in, and on their own merge requests. A bell in the title bar counts the unread; ⌘⇧N opens the
//! list, newest first: j k move, ↵ opens the comment, e is done with it, esc closes.
//!
//! A request seen for the first time sets its comments so far as read: only what comes after is
//! news. What was read is kept per project in `seen-notes.txt`.

use super::keys::{self, On};
use super::px;
use super::{ISLAND_RADIUS, ROW_RADIUS, TITLE_BAR, Workspace, format, theme};
use crate::links::{self, Target};
use crate::mr::{self, Mr, Thread, Todo};
use crate::storage;
use gpui::{ClickEvent, Context, FontWeight, KeyDownEvent, MouseButton, Task, div, prelude::*};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// How often the list is read again while the app runs.
const EVERY: Duration = Duration::from_secs(180);
/// Requests whose discussions are read for replies, at most.
const MOST_REQUESTS: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mention,
    Review,
    /// Another to-do (assigned, approval required, …).
    Todo,
    /// A comment in a discussion the user wrote in.
    Reply,
    /// A comment on the user's own merge request.
    OnYours,
}

impl Kind {
    fn icon(self) -> &'static str {
        match self {
            Kind::Mention => "@",
            Kind::Review => "◎",
            Kind::Todo => "•",
            Kind::Reply | Kind::OnYours => "↩",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Kind::Mention => "mentioned you",
            Kind::Review => "asked for your review",
            Kind::Todo => "has a to-do for you",
            Kind::Reply => "replied",
            Kind::OnYours => "commented on your request",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// `todo:<id>` or `note:<id>`.
    pub key: String,
    pub kind: Kind,
    pub iid: u64,
    pub title: String,
    pub who: String,
    pub text: String,
    pub created: i64,
    pub note: Option<u64>,
    pub todo: Option<u64>,
}

#[derive(Default)]
pub struct Notices {
    pub list: Vec<Notice>,
    pub open: bool,
    pub at: usize,
    /// Opened or looked at: no longer bold, not counted.
    read: HashSet<String>,
    /// `base|iid` → the newest comment id already known.
    seen: HashMap<String, u64>,
    seen_loaded: bool,
    task: Option<Task<()>>,
    poll: Option<Task<()>>,
}

impl Notices {
    pub fn unread(&self) -> usize {
        self.list
            .iter()
            .filter(|n| !self.read.contains(&n.key))
            .count()
    }

    fn load_seen(&mut self) {
        if self.seen_loaded {
            return;
        }
        self.seen_loaded = true;
        self.seen = storage::read("seen-notes.txt")
            .lines()
            .filter_map(|l| {
                let (key, id) = l.split_once('\t')?;
                Some((key.to_owned(), id.parse().ok()?))
            })
            .collect();
    }

    fn save_seen(&self) {
        let mut lines: Vec<String> = self.seen.iter().map(|(k, v)| format!("{k}\t{v}")).collect();
        lines.sort_unstable();
        storage::save("seen-notes.txt", lines.join("\n") + "\n");
    }
}

/// The new comments of request `mr` for user `me`: after `floor` (the newest already known), by
/// someone else, in a discussion `me` wrote in — or anywhere on `me`'s own request.
pub fn replies(me: &str, mr: &Mr, threads: &[Thread], floor: u64) -> Vec<Notice> {
    let mine = mr.author_login == me;
    let mut out = Vec::new();
    for t in threads {
        let took_part = t.notes.iter().any(|n| n.login == me);
        if !(took_part || mine) {
            continue;
        }
        for n in t.notes.iter().filter(|n| n.id > floor && n.login != me) {
            out.push(Notice {
                key: format!("note:{}", n.id),
                kind: if took_part {
                    Kind::Reply
                } else {
                    Kind::OnYours
                },
                iid: mr.iid,
                title: mr.title.clone(),
                who: n.author.clone(),
                text: n.body.clone(),
                created: n.created,
                note: Some(n.id),
                todo: None,
            });
        }
    }
    out
}

fn from_todo(t: &Todo) -> Notice {
    let note = match links::parse(&t.url).map(|l| l.target) {
        Some(Target::Request { note, .. }) => note,
        _ => None,
    };
    Notice {
        key: format!("todo:{}", t.id),
        kind: match t.action.as_str() {
            "mentioned" | "directly_addressed" => Kind::Mention,
            "review_requested" => Kind::Review,
            _ => Kind::Todo,
        },
        iid: t.iid,
        title: t.title.clone(),
        who: t.author.clone(),
        text: t.body.clone(),
        created: t.created,
        note,
        todo: Some(t.id),
    }
}

impl Workspace {
    /// Reads the to-dos and the discussions of the user's requests again, in the background.
    pub(super) fn load_notices(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        if !self.requests.available {
            self.notices.list.clear();
            return;
        }
        self.notices.load_seen();
        let base = remote.base.clone();
        let requests = self.requests.list.clone();
        let seen = self.notices.seen.clone();
        self.notices.task = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let me = mr::username(&remote)?;
                    let todos = mr::todos(&remote).unwrap_or_default();
                    let mut notices: Vec<Notice> = todos.iter().map(from_todo).collect();
                    let mut newest = HashMap::new();
                    let involved = requests
                        .iter()
                        .filter(|m| m.author_login == me || m.reviewers.contains(&me))
                        .take(MOST_REQUESTS);
                    for m in involved {
                        let Ok(threads) = mr::threads(&remote, m.iid) else {
                            continue;
                        };
                        let top = threads
                            .iter()
                            .flat_map(|t| t.notes.iter().map(|n| n.id))
                            .max()
                            .unwrap_or(0);
                        let key = format!("{base}|{}", m.iid);
                        // A request seen for the first time: what is there now is not news.
                        match seen.get(&key) {
                            Some(&floor) => notices.extend(replies(&me, m, &threads, floor)),
                            None => {
                                newest.insert(key, top);
                            }
                        }
                    }
                    notices.sort_by_key(|n| std::cmp::Reverse(n.created));
                    anyhow::Ok((notices, newest))
                })
                .await;
            this.update(cx, |this, cx| {
                if let Ok((list, newest)) = found {
                    if !newest.is_empty() {
                        this.notices.seen.extend(newest);
                        this.notices.save_seen();
                    }
                    this.notices.list = list;
                    this.notices.at = this
                        .notices
                        .at
                        .min(this.notices.list.len().saturating_sub(1));
                }
                cx.notify();
            })
            .ok();
        }));
        if self.notices.poll.is_none() {
            self.notices.poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(EVERY).await;
                    if this.update(cx, |this, cx| this.load_notices(cx)).is_err() {
                        return;
                    }
                }
            }));
        }
    }

    /// ⌘⇧N: the list, read again as it opens.
    pub(super) fn toggle_notices(&mut self, cx: &mut Context<Self>) {
        self.notices.open = !self.notices.open;
        if self.notices.open {
            self.notices.at = 0;
            self.load_notices(cx);
        }
        cx.notify();
    }

    /// Opens notice `ix`: its request, at its comment.
    fn open_notice(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(n) = self.notices.list.get(ix).cloned() else {
            return;
        };
        self.notices.read.insert(n.key.clone());
        self.notices.open = false;
        self.go_to(
            Target::Request {
                iid: n.iid,
                note: n.note,
                line: None,
            },
            cx,
        );
    }

    /// `e`: done with notice `ix` — a to-do is marked done on GitLab, a comment counts as read
    /// from now on.
    fn dismiss_notice(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.notices.list.len() {
            return;
        }
        let n = self.notices.list.remove(ix);
        self.notices.at = self
            .notices
            .at
            .min(self.notices.list.len().saturating_sub(1));
        if let Some(id) = n.todo
            && let Some(remote) = self.web.clone()
        {
            cx.background_executor()
                .spawn(async move { mr::mark_todo_done(&remote, id) })
                .detach();
        }
        if let (Some(id), Some(base)) = (n.note.filter(|_| n.todo.is_none()), self.web.as_ref()) {
            let key = format!("{}|{}", base.base, n.iid);
            let floor = self.notices.seen.entry(key).or_default();
            *floor = (*floor).max(id);
            self.notices.save_seen();
        }
        cx.notify();
    }

    /// Everything listed counts as read.
    fn read_all_notices(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.notices.list.iter().map(|n| n.key.clone()).collect();
        self.notices.read.extend(keys);
        cx.notify();
    }

    pub(super) fn notices_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let n = self.notices.list.len();
        match event.keystroke.key.as_str() {
            "escape" => self.notices.open = false,
            "j" | "down" if n > 0 => self.notices.at = (self.notices.at + 1) % n,
            "k" | "up" if n > 0 => self.notices.at = (self.notices.at + n - 1) % n,
            "enter" => return self.open_notice(self.notices.at, cx),
            "e" => return self.dismiss_notice(self.notices.at, cx),
            _ => return,
        }
        cx.notify();
    }

    /// ↑ ↓ arrive as the palette's actions while the list is open.
    pub(super) fn notices_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let n = self.notices.list.len();
        if n > 0 {
            self.notices.at = if down {
                (self.notices.at + 1) % n
            } else {
                (self.notices.at + n - 1) % n
            };
            cx.notify();
        }
    }

    /// The bell in the title bar, with the unread count.
    pub(super) fn render_bell(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.requests.available {
            return None;
        }
        let unread = self.notices.unread();
        Some(
            super::icon_button("notices-open", "bell", "Notifications  ⌘⇧N")
                .relative()
                .when(self.notices.open, |s| s.bg(theme::selected()))
                .children((unread > 0).then(|| {
                    div()
                        .absolute()
                        .top(px(-2.))
                        .right(px(-4.))
                        .min_w(px(16.))
                        .h(px(16.))
                        .px(px(4.))
                        .rounded_full()
                        .bg(theme::removed())
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::base())
                        .child(if unread > 99 {
                            "99+".to_owned()
                        } else {
                            unread.to_string()
                        })
                }))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_notices(cx))),
        )
    }

    /// The list, hanging from the bell.
    pub(super) fn render_notices(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if !self.notices.open {
            return None;
        }
        let unread = self.notices.unread();
        let rows = self.notices.list.iter().enumerate().map(|(ix, n)| {
            let fresh = !self.notices.read.contains(&n.key);
            let on = ix == self.notices.at;
            div()
                .id(("notice", ix))
                .flex()
                .items_start()
                .gap(px(10.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .when(on, |s| s.bg(theme::selected()))
                .hover(|s| s.bg(theme::hover()))
                .child(
                    div()
                        .flex_none()
                        .size(px(22.))
                        .rounded_full()
                        .bg(theme::hover())
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .text_color(theme::accent())
                        .child(n.kind.icon()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .when(fresh, |s| s.font_weight(FontWeight::SEMIBOLD))
                                        .child(format!("{} {}", n.who, n.kind.verb())),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(11.))
                                        .text_color(theme::faint())
                                        .child(format::ago(n.created)),
                                ),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(theme::accent())
                                .child(format!("!{} {}", n.iid, n.title)),
                        )
                        .children((!n.text.trim().is_empty()).then(|| {
                            div()
                                .line_clamp(2)
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(self.one_line(&n.text))
                        })),
                )
                .children(fresh.then(|| {
                    div()
                        .flex_none()
                        .mt(px(6.))
                        .size(px(7.))
                        .rounded_full()
                        .bg(theme::accent())
                }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_notice(ix, cx)))
        });
        let empty = self.notices.list.is_empty();
        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    div()
                        .id("notices-backdrop")
                        .absolute()
                        .size_full()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.notices.open = false;
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .occlude()
                        .top(px(TITLE_BAR - 4.))
                        .right(px(48.))
                        .w(px(440.))
                        .max_h(px(560.))
                        .flex()
                        .flex_col()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .text_size(px(13.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py(px(10.))
                                .border_b_1()
                                .border_color(theme::island_border())
                                .child(
                                    div()
                                        .flex_1()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(if unread > 0 {
                                            format!("Notifications · {unread} new")
                                        } else {
                                            "Notifications".to_owned()
                                        }),
                                )
                                .children((unread > 0).then(|| {
                                    div()
                                        .id("notices-read-all")
                                        .px_2()
                                        .rounded(px(ROW_RADIUS))
                                        .text_size(px(12.))
                                        .text_color(theme::muted())
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                                        .child("Mark all read")
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.read_all_notices(cx)
                                        }))
                                })),
                        )
                        .child(
                            div()
                                .id("notices-list")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .p(px(6.))
                                .children(rows)
                                .children(empty.then(|| {
                                    div()
                                        .px_3()
                                        .py(px(14.))
                                        .text_size(px(12.))
                                        .text_color(theme::muted())
                                        .child("Nothing new. Mentions, review requests and replies in your discussions show up here.")
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .px_3()
                                .py(px(8.))
                                .border_t_1()
                                .border_color(theme::island_border())
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(keys::caps("J", On::Plain))
                                .child(keys::caps("K", On::Plain))
                                .child("move")
                                .child(keys::caps("↵", On::Plain))
                                .child("open")
                                .child(keys::caps("E", On::Plain))
                                .child("done"),
                        ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mr::Note;

    fn note(id: u64, login: &str) -> Note {
        Note {
            id,
            author: login.to_uppercase(),
            login: login.into(),
            body: format!("note {id}"),
            created: id as i64,
        }
    }

    fn thread(notes: Vec<Note>) -> Thread {
        Thread {
            id: "t".into(),
            notes,
            path: Some("a.rs".into()),
            new_line: Some(1),
            old_line: None,
            resolved: false,
            resolvable: true,
        }
    }

    fn request(author: &str) -> Mr {
        Mr {
            iid: 7,
            title: "Clamp".into(),
            author: author.into(),
            source: "f".into(),
            target: "main".into(),
            url: String::new(),
            updated: 0,
            created: 0,
            head: String::new(),
            comments: 0,
            draft: false,
            author_login: author.into(),
            reviewers: vec!["you".into()],
        }
    }

    #[test]
    fn replies_come_from_discussions_one_wrote_in_after_the_floor() {
        let threads = [
            thread(vec![note(1, "you"), note(2, "anna"), note(5, "boris")]),
            thread(vec![note(3, "anna"), note(6, "boris")]),
        ];
        let got = replies("you", &request("anna"), &threads, 2);
        let ids: Vec<_> = got.iter().map(|n| n.note).collect();
        assert_eq!(ids, vec![Some(5)]);
        assert_eq!(got[0].kind, Kind::Reply);
    }

    #[test]
    fn on_ones_own_request_every_new_comment_counts() {
        let threads = [thread(vec![
            note(3, "anna"),
            note(4, "you"),
            note(6, "boris"),
        ])];
        let got = replies("you", &request("you"), &threads, 3);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].note, Some(6));
    }
}
