//! Notifications for the open repository's GitLab project: the user's to-dos (mentions, review
//! requests, being addressed), new comments where they take part — in discussions they wrote in,
//! on any open merge request, and on their own merge requests — and their discussions resolved by
//! someone else. The bell on the right stripe has a dot while some are unread; ⌘⇧N opens the
//! Notifications tool window (`super::tools`), newest first: j k move, ↵ opens the comment, e is done
//! with it, esc closes.
//!
//! A request seen for the first time sets its comments so far as read, and its resolved discussions
//! as known: only what comes after is news. Kept per project in `seen-notes.txt` and
//! `seen-resolved.txt`; what was opened or marked read in `seen-read.txt`. A request's discussions
//! are read again only when it changed (`updated_at`), or when the list is opened.

use super::keys::{self, On};
use super::px;
use super::tools::{Tool, ToolView, tool_button};
use super::{ROW_RADIUS, Workspace, theme};
use crate::links::{self, Target};
use crate::mr::{self, Mr, Thread, Todo};
use crate::storage;
use gpui::{ClickEvent, Context, FontWeight, ScrollHandle, Task, div, prelude::*};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// How often the list is read again while the app runs.
const EVERY: Duration = Duration::from_secs(180);
/// Open requests whose discussions are read, at most.
const MOST_REQUESTS: usize = 40;

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
    /// A discussion the user took part in, resolved by someone else.
    Resolved,
}

impl Kind {
    fn icon(self) -> &'static str {
        match self {
            Kind::Mention => "@",
            Kind::Review => "◎",
            Kind::Todo => "•",
            Kind::Reply | Kind::OnYours => "↩",
            Kind::Resolved => "✓",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Kind::Mention => "mentioned you",
            Kind::Review => "asked for your review",
            Kind::Todo => "has a to-do for you",
            Kind::Reply => "replied",
            Kind::OnYours => "commented on your request",
            Kind::Resolved => "resolved your discussion",
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
    /// `base|iid|discussion` of a resolved discussion.
    pub resolved: Option<String>,
}

#[derive(Default)]
pub struct Notices {
    pub list: Vec<Notice>,
    pub at: usize,
    /// Opened or looked at: no longer bold, not counted. Kept for the project `read_base`.
    read: HashSet<String>,
    read_base: Option<String>,
    /// `base|iid` → the newest comment id already known.
    seen: HashMap<String, u64>,
    /// `base|iid|discussion` of resolved discussions already known.
    known_resolved: HashSet<String>,
    seen_loaded: bool,
    /// A request's discussions as last read, with its `updated_at` then.
    cache: HashMap<u64, (i64, Vec<Thread>)>,
    task: Option<Task<()>>,
    poll: Option<Task<()>>,
    scroll: ScrollHandle,
}

impl Notice {
    /// "mentioned you", "asked for your review", …
    pub fn kind_verb(&self) -> &'static str {
        self.kind.verb()
    }
}

impl Notices {
    /// The discussion of request `iid` holding note `note`, as last read.
    pub fn thread_of(&self, iid: u64, note: u64) -> Option<&Thread> {
        self.cache
            .get(&iid)?
            .1
            .iter()
            .find(|t| t.notes.iter().any(|n| n.id == note))
    }

    /// Request `iid`'s discussions are read again on the next load (after posting to it).
    pub fn forget(&mut self, iid: u64) {
        self.cache.remove(&iid);
    }

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
        self.known_resolved = storage::read("seen-resolved.txt")
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
    }

    /// The read notices of project `base`, once per project.
    fn load_read(&mut self, base: &str) {
        if self.read_base.as_deref() != Some(base) {
            self.read = read_of(&storage::read(READ_FILE), base);
            self.read_base = Some(base.to_owned());
        }
    }

    /// Writes this project's read notices; other projects' lines (other tabs) are kept as on disk.
    fn save_read(&self) {
        if let Some(base) = &self.read_base {
            storage::save(
                READ_FILE,
                with_read(&storage::read(READ_FILE), base, &self.read),
            );
        }
    }

    fn save_seen(&self) {
        let mut lines: Vec<String> = self.seen.iter().map(|(k, v)| format!("{k}\t{v}")).collect();
        lines.sort_unstable();
        storage::save("seen-notes.txt", lines.join("\n") + "\n");
        let mut known: Vec<&String> = self.known_resolved.iter().collect();
        known.sort_unstable();
        let known: Vec<&str> = known.into_iter().map(String::as_str).collect();
        storage::save("seen-resolved.txt", known.join("\n") + "\n");
    }
}

const READ_FILE: &str = "seen-read.txt";

/// The read notice keys of project `base` in `text`: lines `base<TAB>key`.
fn read_of(text: &str, base: &str) -> HashSet<String> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(b, _)| *b == base)
        .map(|(_, key)| key.to_owned())
        .collect()
}

/// `text` with project `base`'s lines replaced by `read`.
fn with_read(text: &str, base: &str, read: &HashSet<String>) -> String {
    let mut keys: Vec<&String> = read.iter().collect();
    keys.sort_unstable();
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.split_once('\t').is_some_and(|(b, _)| b != base))
        .map(str::to_owned)
        .chain(keys.into_iter().map(|k| format!("{base}\t{k}")))
        .collect();
    lines.sort_unstable();
    lines.join("\n") + "\n"
}

/// The new comments of request `mr` for user `me`: after `floor` (the newest already known), by
/// someone else — in a discussion `me` wrote in only those after `me`'s last note there (what came
/// before was answered), and in any discussion of `me`'s own request.
pub fn replies(me: &str, mr: &Mr, threads: &[Thread], floor: u64) -> Vec<Notice> {
    let mine = mr.author_login == me;
    let mut out = Vec::new();
    for t in threads {
        let last_mine = t.notes.iter().filter(|n| n.login == me).map(|n| n.id).max();
        let took_part = last_mine.is_some();
        if !(took_part || mine) {
            continue;
        }
        let after = floor.max(last_mine.unwrap_or(0));
        for n in t.notes.iter().filter(|n| n.id > after && n.login != me) {
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
                resolved: None,
            });
        }
    }
    out
}

/// The discussions of request `mr` that `me` took part in and someone else resolved, not yet in
/// `known` (keys `base|iid|discussion`).
pub fn resolutions(
    me: &str,
    base: &str,
    mr: &Mr,
    threads: &[Thread],
    known: &HashSet<String>,
) -> Vec<Notice> {
    threads
        .iter()
        .filter(|t| t.resolved && t.notes.iter().any(|n| n.login == me))
        .filter_map(|t| {
            let (who, login) = t.resolved_by.clone()?;
            let key = format!("{base}|{}|{}", mr.iid, t.id);
            (login != me && !known.contains(&key)).then(|| Notice {
                key: format!("resolved:{key}"),
                kind: Kind::Resolved,
                iid: mr.iid,
                title: mr.title.clone(),
                who,
                text: t.notes.first().map(|n| n.body.clone()).unwrap_or_default(),
                created: t.resolved_at,
                note: t.notes.first().map(|n| n.id),
                todo: None,
                resolved: Some(key),
            })
        })
        .collect()
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
        resolved: None,
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
        self.notices.load_read(&remote.base);
        let base = remote.base.clone();
        let requests = self.requests.list.clone();
        let seen = self.notices.seen.clone();
        let known = self.notices.known_resolved.clone();
        let cache = self.notices.cache.clone();
        self.notices.task = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let me = mr::username(&remote)?;
                    let todos = mr::todos(&remote);
                    // Everything read: only then may read keys no longer listed be forgotten.
                    let mut complete = todos.is_ok();
                    let mut notices: Vec<Notice> =
                        todos.unwrap_or_default().iter().map(from_todo).collect();
                    let mut floors = HashMap::new();
                    let mut first_resolved = Vec::new();
                    let mut reopened = Vec::new();
                    let mut fresh = HashMap::new();
                    for m in requests.iter().take(MOST_REQUESTS) {
                        // Read again only what changed since the last look.
                        let threads = match cache.get(&m.iid) {
                            Some((updated, threads)) if *updated == m.updated => threads.clone(),
                            _ => match mr::threads(&remote, m.iid) {
                                Ok(threads) => threads,
                                Err(_) => {
                                    complete = false;
                                    continue;
                                }
                            },
                        };
                        let key = format!("{base}|{}", m.iid);
                        let resolved_keys: Vec<String> = threads
                            .iter()
                            .filter(|t| t.resolved)
                            .map(|t| format!("{key}|{}", t.id))
                            .collect();
                        match seen.get(&key) {
                            Some(&floor) => {
                                notices.extend(replies(&me, m, &threads, floor));
                                notices.extend(resolutions(&me, &base, m, &threads, &known));
                                // Opened again: a later resolve is news again.
                                reopened.extend(
                                    threads
                                        .iter()
                                        .filter(|t| !t.resolved)
                                        .map(|t| format!("{key}|{}", t.id))
                                        .filter(|k| known.contains(k)),
                                );
                            }
                            // A request seen for the first time: what is there now is not news.
                            None => {
                                let top = threads
                                    .iter()
                                    .flat_map(|t| t.notes.iter().map(|n| n.id))
                                    .max()
                                    .unwrap_or(0);
                                floors.insert(key, top);
                                first_resolved.extend(resolved_keys);
                            }
                        }
                        fresh.insert(m.iid, (m.updated, threads));
                    }
                    notices.sort_by_key(|n| std::cmp::Reverse(n.created));
                    anyhow::Ok((notices, floors, first_resolved, reopened, fresh, complete))
                })
                .await;
            this.update(cx, |this, cx| {
                if let Ok((list, floors, first_resolved, reopened, fresh, complete)) = found {
                    let changed =
                        !floors.is_empty() || !first_resolved.is_empty() || !reopened.is_empty();
                    this.notices.seen.extend(floors);
                    this.notices.known_resolved.extend(first_resolved);
                    for key in reopened {
                        this.notices.known_resolved.remove(&key);
                    }
                    if changed {
                        this.notices.save_seen();
                    }
                    this.notices.cache = fresh;
                    if complete {
                        let before = this.notices.read.len();
                        this.notices
                            .read
                            .retain(|k| list.iter().any(|n| &n.key == k));
                        if this.notices.read.len() != before {
                            this.notices.save_read();
                        }
                    }
                    // The selected notice stays selected where it moved to.
                    let selected = this
                        .notices
                        .list
                        .get(this.notices.at)
                        .map(|n| n.key.clone());
                    this.notices.list = list;
                    this.notices.at = selected
                        .and_then(|key| this.notices.list.iter().position(|n| n.key == key))
                        .unwrap_or(this.notices.at)
                        .min(this.notices.list.len().saturating_sub(1));
                    if this.inbox {
                        this.inbox_selected(cx);
                    }
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
        self.toggle_tool(Tool::Notifications, cx);
    }

    /// The window opened: everything read afresh, not only what reports a change.
    pub(super) fn refresh_notices(&mut self, cx: &mut Context<Self>) {
        self.notices.at = 0;
        self.notices.scroll.scroll_to_item(0);
        self.notices.cache.clear();
        self.load_notices(cx);
    }

    /// Opens notice `ix`: its request, at its comment.
    /// Notice `ix` is not read yet.
    pub(super) fn notice_unread(&self, ix: usize) -> bool {
        self.notices
            .list
            .get(ix)
            .is_some_and(|n| !self.notices.read.contains(&n.key))
    }

    pub(super) fn open_notice(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(n) = self.notices.list.get(ix).cloned() else {
            return;
        };
        if self.notices.read.insert(n.key.clone()) {
            self.notices.save_read();
        }
        // On an outdated comment it opens on the diff since it, once that is known (#46).
        self.since_on_open = n.note.map(|note| (n.iid, note));
        // The window stays; the keyboard goes to what was opened.
        self.set_zone(super::zones::Zone::Diff, cx);
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
    pub(super) fn dismiss_notice(&mut self, ix: usize, cx: &mut Context<Self>) {
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
        if let Some(key) = n.resolved.clone() {
            self.notices.known_resolved.insert(key);
            self.notices.save_seen();
        } else if let (Some(id), Some(base)) =
            (n.note.filter(|_| n.todo.is_none()), self.web.as_ref())
        {
            let key = format!("{}|{}", base.base, n.iid);
            let floor = self.notices.seen.entry(key).or_default();
            *floor = (*floor).max(id);
            self.notices.save_seen();
        }
        cx.notify();
    }

    /// Everything listed counts as read.
    pub(super) fn read_all_notices(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.notices.list.iter().map(|n| n.key.clone()).collect();
        self.notices.read.extend(keys);
        self.notices.save_read();
        cx.notify();
    }

    /// j k, ↑ ↓ while the window has the keyboard: the next or previous notice, scrolled into view.
    pub(super) fn notices_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let n = self.notices.list.len();
        if n > 0 {
            self.notices.at = if down {
                (self.notices.at + 1) % n
            } else {
                (self.notices.at + n - 1) % n
            };
            self.notices.scroll.scroll_to_item(self.notices.at);
            cx.notify();
        }
    }

    /// The Notifications tool window.
    pub(super) fn notifications_tool(&self, cx: &mut Context<Self>) -> ToolView {
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
                                    super::hover::cut(
                                        gpui::SharedString::from(format!("note-who-{}", n.created)),
                                        format!("{} {}", n.who, n.kind.verb()),
                                    )
                                    .flex_1()
                                    .when(fresh, |s| s.font_weight(FontWeight::SEMIBOLD)),
                                )
                                .child(
                                    super::hover::ago(
                                        gpui::SharedString::from(format!("note-ago-{}", n.created)),
                                        n.created,
                                    )
                                    .text_size(px(11.))
                                    .text_color(theme::faint()),
                                ),
                        )
                        .child(
                            super::hover::cut(
                                gpui::SharedString::from(format!("note-title-{}", n.created)),
                                format!("{} {}", super::words::number(n.iid), n.title),
                            )
                            .text_size(px(12.))
                            .text_color(theme::accent()),
                        )
                        // A long hover reads what was written, whole (X59).
                        .children((!n.text.trim().is_empty()).then(|| {
                            super::hover::cut_to(
                                gpui::SharedString::from(format!("note-text-{}", n.created)),
                                self.one_line(&n.text),
                                2,
                            )
                            .text_size(px(12.))
                            .text_color(theme::muted())
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
        ToolView {
            title: "Notifications".into(),
            count: (unread > 0).then(|| format!("{unread} new")),
            actions: (unread > 0)
                .then(|| {
                    tool_button("notices-read-all", "Mark all read")
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.read_all_notices(cx)),
                        )
                        .into_any_element()
                })
                .into_iter()
                .collect(),
            body: div()
                .id("notices-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.notices.scroll)
                .p(px(6.))
                .children(rows)
                .children(empty.then(|| {
                    div()
                        .px_3()
                        .py(px(14.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("Nothing new. Mentions, review requests and replies in your discussions show up here.")
                }))
                .into_any_element(),
            foot: Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(keys::caps("J", On::Plain))
                    .child(keys::caps("K", On::Plain))
                    .child("move")
                    .child(keys::caps("↵", On::Plain))
                    .child("open")
                    .child(keys::caps("E", On::Plain))
                    .child("done")
                    .into_any_element(),
            ),
        }
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
            resolved_by: None,
            resolved_at: 0,
            commit: None,
            outdated: false,
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
    fn a_discussion_one_wrote_in_resolved_by_someone_else_is_news_once() {
        let mut done = thread(vec![note(1, "you"), note(2, "anna")]);
        done.resolved = true;
        done.resolved_by = Some(("Anna".into(), "anna".into()));
        let mut mine = thread(vec![note(3, "you")]);
        mine.id = "u".into();
        mine.resolved = true;
        mine.resolved_by = Some(("You".into(), "you".into()));
        let threads = [done, mine];
        let got = resolutions("you", "b", &request("anna"), &threads, &HashSet::new());
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].kind, got[0].who.as_str()), (Kind::Resolved, "Anna"));
        let known: HashSet<String> = ["b|7|t".to_owned()].into();
        assert!(resolutions("you", "b", &request("anna"), &threads, &known).is_empty());
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

    #[test]
    fn a_comment_one_answered_is_not_a_reply() {
        // #2: boris comments on your line on anna's request; you answer. Boris did not reply to you.
        let answered = [thread(vec![note(5, "boris"), note(6, "you")])];
        assert!(replies("you", &request("anna"), &answered, 4).is_empty());
        // On your own request too: what you answered is not news.
        let own = [thread(vec![
            note(5, "boris"),
            note(6, "you"),
            note(8, "boris"),
        ])];
        let got = replies("you", &request("you"), &own, 4);
        assert_eq!(
            got.iter().map(|n| n.note).collect::<Vec<_>>(),
            vec![Some(8)]
        );
        assert_eq!(got[0].kind, Kind::Reply);
    }

    #[test]
    fn read_notices_are_kept_per_project() {
        let disk = "a\tnote:1\nb\tnote:2\nb\ttodo:3\n";
        let read = read_of(disk, "b");
        assert_eq!(read, ["note:2".to_owned(), "todo:3".to_owned()].into());
        // b's lines replaced, a's (another tab) kept.
        let now: HashSet<String> = ["todo:3".to_owned(), "note:9".to_owned()].into();
        let saved = with_read(disk, "b", &now);
        assert_eq!(saved, "a\tnote:1\nb\tnote:9\nb\ttodo:3\n");
        assert_eq!(read_of(&saved, "b"), now);
        assert_eq!(read_of(&saved, "a"), ["note:1".to_owned()].into());
    }
}
