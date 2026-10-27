//! Changes sent to GitLab or GitHub, shown before the host answers (#139). Each change is a list of
//! `Op`s on a request; what shows is what the host last gave with the changes still waiting applied
//! on top, so two in flight, or a read arriving between them, lose neither, and a refusal takes back
//! only its own. New items get temporary ids, kept in their op, so applying it again gives the same
//! ones. While an op applies, its item is faded and says what it waits for where its time goes.
//!
//! The host's answer to a change stamps it with a tick; a read begun at tick `t` holds every change
//! answered before `t`, so when it arrives those stop applying and the rest go on top of it.

use super::Workspace;
use crate::mr::{Approvals, Draft, Note, Thread};
use gpui::Context;

/// What a change does to a request's discussions, pending comments or approvals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// A reply, note `note`, in discussion `thread`.
    Reply {
        thread: String,
        note: u64,
        body: String,
    },
    /// A new discussion with its first note: on a line, or a comment on the request.
    Discussion {
        thread: String,
        note: u64,
        body: String,
        at: Option<At>,
    },
    Edit {
        note: u64,
        body: String,
    },
    /// The note stays, faded, until the host has taken it away.
    Delete {
        note: u64,
    },
    Resolve {
        thread: String,
        resolved: bool,
    },
    /// A pending comment written.
    Draft(Draft),
    /// A pending comment deleted: it stays, faded, until the host has taken it away.
    DropDraft(u64),
    /// Pending comments published (Send this now, Submit review): each, by its id, becomes a
    /// discussion with a temporary id and note.
    Publish(Vec<(u64, String, u64)>),
    Approve {
        mine: bool,
    },
}

/// A new discussion's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct At {
    pub path: String,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
}

/// What a change shows, and what it waits for.
struct Change {
    key: u64,
    iid: u64,
    ops: Vec<Op>,
    /// When it was made: the time of what it adds.
    at: i64,
    /// The tick of the host's answer.
    done: Option<u64>,
}

/// Who makes the changes: their name and their host login.
pub type Me<'a> = (&'a str, &'a str);

#[derive(Default)]
pub struct Waiting {
    changes: Vec<Change>,
    tick: u64,
    next: u64,
}

/// Temporary ids are counted down from the top, far from any a host gives.
const TEMP: u64 = u64::MAX - (1 << 40);
const TEMP_THREAD: &str = "waiting|";

/// An id `Waiting::temp` gave: the item is not on the host yet.
pub fn is_temp(id: u64) -> bool {
    id > TEMP
}

/// A discussion id `Waiting::temp_thread` gave.
pub fn is_temp_thread(id: &str) -> bool {
    id.starts_with(TEMP_THREAD)
}

impl Waiting {
    /// A temporary id for a new note or pending comment.
    pub fn temp(&mut self) -> u64 {
        self.next += 1;
        u64::MAX - self.next
    }

    /// A temporary id for a new discussion.
    pub fn temp_thread(&mut self) -> String {
        format!("{TEMP_THREAD}{}", self.temp())
    }

    /// The tick a read begun now starts at.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// `ops` on request `iid` go to the host; the key names the change when it answers.
    pub fn send(&mut self, iid: u64, ops: Vec<Op>, at: i64) -> u64 {
        self.next += 1;
        let key = self.next;
        self.changes.push(Change {
            key,
            iid,
            ops,
            at,
            done: None,
        });
        key
    }

    /// The host took change `key`: it applies until a read begun after now arrives.
    pub fn took(&mut self, key: u64) {
        if let Some(c) = self.changes.iter_mut().find(|c| c.key == key) {
            c.done = Some(self.tick);
            self.tick += 1;
        }
    }

    /// The host refused change `key`: it is taken back.
    pub fn refused(&mut self, key: u64) {
        self.changes.retain(|c| c.key != key);
    }

    /// The changes answered that every reader holds by now are let go: `held(iid, done)` says
    /// whether what shows of request `iid` was read after the answer at tick `done`.
    pub fn settle(&mut self, held: impl Fn(u64, u64) -> bool) {
        self.changes
            .retain(|c| c.done.is_none_or(|done| !held(c.iid, done)));
    }

    /// Whether any change on request `iid` waits on what a read begun at `from` holds.
    pub fn any(&self, iid: u64, from: u64) -> bool {
        self.on(iid, from).next().is_some()
    }

    /// The changes on request `iid` that a read begun at tick `from` does not hold, oldest first.
    fn on(&self, iid: u64, from: u64) -> impl DoubleEndedIterator<Item = (&Op, i64)> {
        self.changes
            .iter()
            .filter(move |c| c.iid == iid && c.done.is_none_or(|done| done >= from))
            .flat_map(|c| c.ops.iter().map(move |op| (op, c.at)))
    }

    /// The changes waiting on request `iid` applied to what a read begun at `from` gave.
    pub fn apply(
        &self,
        iid: u64,
        from: u64,
        me: Me,
        threads: &mut Vec<Thread>,
        drafts: &mut Vec<Draft>,
        approvals: &mut Option<Approvals>,
    ) {
        let note = |id: u64, body: &str, at: i64| Note {
            id,
            author: me.0.to_owned(),
            login: me.1.to_owned(),
            body: body.to_owned(),
            created: at,
        };
        let discussion = |id: &str, note: Note, at: Option<&At>| Thread {
            id: id.to_owned(),
            notes: vec![note],
            path: at.map(|a| a.path.clone()),
            new_line: at.and_then(|a| a.new_line),
            old_line: at.and_then(|a| a.old_line),
            resolved: false,
            resolvable: at.is_some(),
            resolved_by: None,
            resolved_at: 0,
            commit: None,
            outdated: false,
        };
        for (op, at) in self.on(iid, from) {
            match op {
                Op::Reply {
                    thread,
                    note: id,
                    body,
                } => {
                    if let Some(t) = threads.iter_mut().find(|t| t.id == *thread)
                        && !t.notes.iter().any(|n| n.id == *id)
                    {
                        t.notes.push(note(*id, body, at));
                    }
                }
                Op::Discussion {
                    thread,
                    note: id,
                    body,
                    at: line,
                } => {
                    if !threads.iter().any(|t| t.id == *thread) {
                        threads.push(discussion(thread, note(*id, body, at), line.as_ref()));
                    }
                }
                Op::Edit { note: id, body } => {
                    if let Some(n) = threads
                        .iter_mut()
                        .flat_map(|t| t.notes.iter_mut())
                        .find(|n| n.id == *id)
                    {
                        n.body.clone_from(body);
                    }
                }
                Op::Resolve { thread, resolved } => {
                    if let Some(t) = threads.iter_mut().find(|t| t.id == *thread) {
                        t.resolved = *resolved;
                        t.resolved_by = resolved.then(|| (me.0.to_owned(), me.1.to_owned()));
                        t.resolved_at = at;
                    }
                }
                Op::Draft(d) => {
                    if !drafts.iter().any(|x| x.id == d.id) {
                        drafts.push(d.clone());
                    }
                }
                Op::Publish(sent) => {
                    for (id, thread, n) in sent {
                        if let Some(ix) = drafts.iter().position(|d| d.id == *id) {
                            let d = drafts.remove(ix);
                            let line = d.path.map(|path| At {
                                path,
                                new_line: d.new_line,
                                old_line: d.old_line,
                            });
                            threads.push(discussion(thread, note(*n, &d.body, at), line.as_ref()));
                        }
                    }
                }
                Op::Approve { mine } => {
                    if let Some(a) = approvals.as_mut().filter(|a| a.mine != *mine) {
                        a.mine = *mine;
                        if *mine {
                            a.by.push(me.0.to_owned());
                            a.left = a.left.saturating_sub(1);
                        } else {
                            a.by.retain(|b| b != me.0);
                            if a.required > 0 {
                                a.left = (a.left + 1).min(a.required);
                            }
                        }
                    }
                }
                Op::Delete { .. } | Op::DropDraft(_) => {}
            }
        }
    }

    /// What note `id` waits for, while it does.
    pub fn note(&self, iid: u64, from: u64, id: u64) -> Option<&'static str> {
        self.on(iid, from).rev().find_map(|(op, _)| match op {
            Op::Reply { note, .. } | Op::Discussion { note, .. } if *note == id => Some("Sending…"),
            Op::Publish(sent) if sent.iter().any(|(_, _, n)| *n == id) => Some("Sending…"),
            Op::Edit { note, .. } if *note == id => Some("Saving…"),
            Op::Delete { note } if *note == id => Some("Deleting…"),
            _ => None,
        })
    }

    /// What discussion `id` waits for, while it does.
    pub fn thread(&self, iid: u64, from: u64, id: &str) -> Option<&'static str> {
        self.on(iid, from).rev().find_map(|(op, _)| match op {
            Op::Discussion { thread, .. } if thread == id => Some("Sending…"),
            Op::Publish(sent) if sent.iter().any(|(_, t, _)| t == id) => Some("Sending…"),
            Op::Resolve { thread, resolved } if thread == id => Some(if *resolved {
                "Resolving…"
            } else {
                "Reopening…"
            }),
            _ => None,
        })
    }

    /// What pending comment `id` waits for, while it does.
    pub fn draft(&self, iid: u64, from: u64, id: u64) -> Option<&'static str> {
        self.on(iid, from).rev().find_map(|(op, _)| match op {
            Op::Draft(d) if d.id == id => Some("Saving…"),
            Op::DropDraft(d) if *d == id => Some("Deleting…"),
            _ => None,
        })
    }

    /// Whether an approval, or its taking back, waits.
    pub fn approving(&self, iid: u64, from: u64) -> bool {
        self.on(iid, from)
            .any(|(op, _)| matches!(op, Op::Approve { .. }))
    }
}

/// What the request open showed before the waiting changes went on it: the host's last answer.
#[derive(Default)]
pub struct Base {
    pub threads: Vec<Thread>,
    pub drafts: Vec<Draft>,
    pub approvals: Option<Approvals>,
}

impl Workspace {
    /// Sends `ops` on request `iid` with `work`: they show at once, faded; taken, the request's
    /// discussions are read again, quietly; refused, they are taken back and `refused` is told why.
    /// The request is read again either way when it is open, for whatever went through.
    pub(super) fn send_change(
        &mut self,
        iid: u64,
        ops: Vec<Op>,
        work: impl FnOnce(crate::git::WebRemote) -> anyhow::Result<()> + Send + 'static,
        refused: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let key = self.requests.waiting.send(iid, ops, super::inbox::now());
        self.show_waiting(iid);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { work(remote) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.requests.waiting.took(key),
                    Err(e) => {
                        this.requests.waiting.refused(key);
                        this.show_waiting(iid);
                        refused(this, format!("{e:#}"), cx);
                    }
                }
                // The Inbox reads it again on its next look; at once while it is on screen.
                this.notices.stale(iid);
                if this.inbox {
                    this.load_notices(cx);
                }
                if this.requests.current == Some(iid) {
                    this.reload_threads(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The error bar says why a change was refused.
    pub(super) fn say_refused(&mut self, why: String, cx: &mut Context<Self>) {
        self.error = Some(why.into());
        cx.notify();
    }

    /// What shows of request `iid` made again: the host's last answer with the changes still
    /// waiting on it. The folds, the keyboard's discussion and pending comment, and a reply being
    /// written follow theirs wherever it moved.
    pub(super) fn show_waiting(&mut self, iid: u64) {
        let me = (
            self.me.clone(),
            self.requests.login.clone().unwrap_or_default(),
        );
        let me = (me.0.as_str(), me.1.as_str());
        if self.requests.current == Some(iid) {
            let r = &self.requests;
            let thread_id = |ix: usize| r.threads.get(ix).map(|t| t.id.clone());
            let toggled: Vec<String> = self
                .toggled_mr
                .iter()
                .filter_map(|&ix| thread_id(ix))
                .collect();
            let at = self.thread_at.and_then(thread_id);
            let draft_at = self.draft_at.and_then(|ix| r.drafts.get(ix)).map(|d| d.id);
            let composing = self
                .compose
                .as_ref()
                .and_then(|c| c.thread())
                .and_then(thread_id);
            let (mut threads, mut drafts, mut approvals) = (
                r.base.threads.clone(),
                r.base.drafts.clone(),
                r.base.approvals.clone(),
            );
            r.waiting
                .apply(iid, r.from, me, &mut threads, &mut drafts, &mut approvals);
            let ix_of = |id: &str| threads.iter().position(|t| t.id == id);
            self.toggled_mr = toggled.iter().filter_map(|id| ix_of(id)).collect();
            self.thread_at = at.and_then(|id| ix_of(&id));
            self.draft_at = draft_at.and_then(|id| drafts.iter().position(|d| d.id == id));
            if let (Some(id), Some(c)) = (composing, self.compose.as_mut())
                && let Some(ix) = ix_of(&id)
            {
                c.set_thread(ix);
            }
            self.requests.threads = threads;
            self.requests.drafts = drafts;
            self.requests.approvals = approvals;
            self.refresh_rows();
        }
        self.notices.show(iid, &self.requests.waiting, me);
    }

    /// The host's answer for the open request `iid`, from a read begun at tick `from`: what shows
    /// is made again on it, and the changes every reader holds are let go.
    pub(super) fn take_base(&mut self, iid: u64, from: u64, base: Base, cx: &mut Context<Self>) {
        self.requests.base = base;
        self.requests.from = from;
        self.settle_waiting();
        self.show_waiting(iid);
        // Both read the discussions just taken in.
        self.compute_histories(cx);
        self.load_awards(cx);
    }

    /// The changes the request open and the Inbox both hold now are let go.
    pub(super) fn settle_waiting(&mut self) {
        let (current, from) = (self.requests.current, self.requests.from);
        let notices = &self.notices;
        self.requests
            .waiting
            .settle(|iid, done| (current != Some(iid) || from > done) && notices.holds(iid, done));
    }

    /// A note's time, or what it waits for; and whether it waits.
    pub(super) fn note_time(&self, id: u64, created: i64) -> (String, bool) {
        match self
            .requests
            .current
            .and_then(|iid| self.requests.waiting.note(iid, self.requests.from, id))
        {
            Some(word) => (word.to_owned(), true),
            None => (super::format::ago(created), false),
        }
    }

    /// What the open request's discussion `id` waits for.
    pub(super) fn thread_waits(&self, id: &str) -> Option<&'static str> {
        let iid = self.requests.current?;
        self.requests.waiting.thread(iid, self.requests.from, id)
    }

    /// What the open request's pending comment `id` waits for.
    pub(super) fn draft_waits(&self, id: u64) -> Option<&'static str> {
        let iid = self.requests.current?;
        self.requests.waiting.draft(iid, self.requests.from, id)
    }

    /// Whether the open request's approval waits.
    pub(super) fn approval_waits(&self) -> bool {
        self.requests
            .current
            .is_some_and(|iid| self.requests.waiting.approving(iid, self.requests.from))
    }
}

/// How faded a waiting item is.
pub const FADED: f32 = 0.5;

impl Workspace {
    /// The snapshot checks' changes on the request open: a reply in its first discussion on a line,
    /// the second resolved or opened again, the approval given or taken back, the first pending
    /// comment sent now and the second deleted.
    #[cfg(feature = "snapshot")]
    pub(super) fn snapshot_changes(&mut self, cx: &mut Context<Self>) {
        let on_lines: Vec<usize> = (self.requests.threads.iter().enumerate())
            .filter(|(_, t)| t.path.is_some() && t.resolvable)
            .map(|(ix, _)| ix)
            .collect();
        if let Some(&ix) = on_lines.first() {
            self.start_reply(ix, cx);
            if let Some(c) = self.compose.as_mut() {
                c.edit = crate::textedit::TextEdit::new("Moved it into the next push.");
            }
            self.add_comment(cx);
        }
        if let Some(&ix) = on_lines.get(1) {
            self.toggle_resolved(ix, cx);
        }
        self.toggle_approval(cx);
        // The first pending comment sent now, the second deleted.
        let drafts: Vec<u64> = self.requests.drafts.iter().map(|d| d.id).collect();
        if let Some(&id) = drafts.first() {
            self.send_draft_now(id, cx);
        }
        if let Some(&id) = drafts.get(1) {
            self.discard_draft(id, cx);
        }
        // Where they show: on Changes, at the first.
        self.show_section(super::nav::Section::Changes, cx);
        self.thread_at = None;
        self.step_thread(true, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, notes: &[u64]) -> Thread {
        Thread {
            id: id.into(),
            notes: notes
                .iter()
                .map(|&id| Note {
                    id,
                    author: "Ann".into(),
                    login: "ann".into(),
                    body: format!("note {id}"),
                    created: 1,
                })
                .collect(),
            path: Some("a.rs".into()),
            new_line: Some(3),
            old_line: None,
            resolved: false,
            resolvable: true,
            resolved_by: None,
            resolved_at: 0,
            commit: None,
            outdated: false,
        }
    }

    fn draft(id: u64) -> Draft {
        Draft {
            id,
            body: format!("draft {id}"),
            path: Some("a.rs".into()),
            new_line: Some(7),
            old_line: None,
        }
    }

    const ME: Me = ("Me", "me");

    #[test]
    fn a_change_shows_until_a_read_begun_after_its_answer_holds_it() {
        let mut w = Waiting::default();
        let note = w.temp();
        assert!(is_temp(note) && !is_temp(9_999_999));
        let reply = w.send(
            1,
            vec![Op::Reply {
                thread: "t".into(),
                note,
                body: "thanks".into(),
            }],
            5,
        );
        let base = vec![thread("t", &[10])];
        let shown = |w: &Waiting, from: u64| {
            let (mut t, mut d, mut a) = (base.clone(), Vec::new(), None);
            w.apply(1, from, ME, &mut t, &mut d, &mut a);
            t
        };
        // Waiting: on top of any read, faded; and not on another request.
        let t = shown(&w, 0);
        assert_eq!(t[0].notes.len(), 2);
        assert_eq!(t[0].notes[1].login, "me");
        assert_eq!(t[0].notes[1].created, 5);
        assert_eq!(w.note(1, 0, note), Some("Sending…"));
        assert_eq!(w.note(2, 0, note), None);
        // Taken at tick 0: a read begun before still lacks it, one begun after holds it.
        let from_before = w.tick();
        w.took(reply);
        let from_after = w.tick();
        assert_eq!(shown(&w, from_before)[0].notes.len(), 2);
        assert_eq!(shown(&w, from_after)[0].notes.len(), 1);
        assert_eq!(w.note(1, from_after, note), None);
        // Let go once every reader holds it.
        w.settle(|_, done| from_before > done);
        assert!(w.any(1, from_before));
        w.settle(|_, done| from_after > done);
        assert!(!w.any(1, 0));
    }

    #[test]
    fn a_refusal_takes_back_only_its_own_change() {
        let mut w = Waiting::default();
        let (thread_id, note) = (w.temp_thread(), w.temp());
        assert!(is_temp_thread(&thread_id));
        let on_line = w.send(
            1,
            vec![Op::Discussion {
                thread: thread_id.clone(),
                note,
                body: "why?".into(),
                at: Some(At {
                    path: "b.rs".into(),
                    new_line: None,
                    old_line: Some(4),
                }),
            }],
            0,
        );
        w.send(
            1,
            vec![Op::Resolve {
                thread: "t".into(),
                resolved: true,
            }],
            0,
        );
        let (mut t, mut d, mut a) = (vec![thread("t", &[10])], Vec::new(), None);
        w.apply(1, 0, ME, &mut t, &mut d, &mut a);
        assert_eq!(t.len(), 2);
        assert!(t[0].resolved);
        assert_eq!(t[0].resolved_by, Some(("Me".into(), "me".into())));
        assert_eq!(w.thread(1, 0, "t"), Some("Resolving…"));
        assert_eq!(t[1].id, thread_id);
        assert_eq!((t[1].old_line, t[1].new_line), (Some(4), None));
        assert!(t[1].resolvable);
        w.refused(on_line);
        let (mut t, mut d, mut a) = (vec![thread("t", &[10])], Vec::new(), None);
        w.apply(1, 0, ME, &mut t, &mut d, &mut a);
        assert_eq!(t.len(), 1);
        assert!(t[0].resolved);
    }

    #[test]
    fn pending_comments_and_approvals_change_in_place() {
        let mut w = Waiting::default();
        let (new, thread_id, note) = (w.temp(), w.temp_thread(), w.temp());
        w.send(
            1,
            vec![Op::Draft(Draft {
                id: new,
                ..draft(0)
            })],
            0,
        );
        w.send(1, vec![Op::DropDraft(20)], 0);
        w.send(1, vec![Op::Publish(vec![(21, thread_id.clone(), note)])], 0);
        w.send(1, vec![Op::Approve { mine: true }], 0);
        let (mut t, mut d) = (Vec::new(), vec![draft(20), draft(21)]);
        let mut a = Some(Approvals {
            by: vec!["Ann".into()],
            required: 2,
            left: 1,
            mine: false,
            can: true,
        });
        w.apply(1, 0, ME, &mut t, &mut d, &mut a);
        // Deleted stays faded; sent became a discussion on its line; new is last.
        assert_eq!(d.iter().map(|d| d.id).collect::<Vec<_>>(), [20, new]);
        assert_eq!(w.draft(1, 0, 20), Some("Deleting…"));
        assert_eq!(w.draft(1, 0, new), Some("Saving…"));
        assert_eq!(t.len(), 1);
        assert_eq!(
            (t[0].id.as_str(), t[0].new_line),
            (thread_id.as_str(), Some(7))
        );
        assert_eq!(t[0].notes[0].body, "draft 21");
        assert_eq!(w.note(1, 0, note), Some("Sending…"));
        let a = a.unwrap();
        assert!(a.mine && a.left == 0 && a.by == ["Ann", "Me"]);
        assert!(w.approving(1, 0));
    }
}
