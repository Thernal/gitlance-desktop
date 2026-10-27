//! What agents leave for the reviewer, as a tab sees it: the marks that tell an agent's pending
//! comments from one's own (#150) and the requests it gathered (#148), and the events of the tab's
//! project — pending comments an agent wrote on a request, requests it put under Merge requests,
//! the changes it opened under Branches (#149) — taken once, each with a toast offering to go and
//! see. Nothing here moves the screen until the reviewer asks.

use super::Workspace;
use super::goto::Spot;
use super::nav::{Item, Page};
use super::toast::Act;
use crate::agents::{Bus, Mark, What};
use gpui::Context;
use std::path::Path;

impl Workspace {
    /// On the agents' beat (`poll_agents`): the marks read again when they changed, and this
    /// project's events taken. Only a tab on screen polls, so a tab behind hears its news when it
    /// comes on screen.
    pub(super) fn agent_events(&mut self, project: &Path, bus: &Bus, cx: &mut Context<Self>) {
        let stamp = bus.marks_stamp(project);
        if stamp != self.marks_stamp {
            self.marks_stamp = stamp;
            self.agent_marks = bus.marks(project);
            cx.notify();
        }
        // The window's own beat takes `Open` (`Shell::agents_beat`).
        for event in bus.events() {
            if event.project != project || event.what == What::Open || !bus.take_event(&event.uid) {
                continue;
            }
            match event.what {
                // A host that gave no id still got one comment.
                What::Pending { iid, ids } => {
                    self.agent_left(event.agent, iid, ids.len().max(1), cx)
                }
                What::Gathered { iids } => self.agent_gathered(event.agent, iids, cx),
                What::Compare {
                    base,
                    base_id,
                    head,
                    head_id,
                    direct,
                } => {
                    let (Ok(a), Ok(b)) =
                        (git2::Oid::from_str(&base_id), git2::Oid::from_str(&head_id))
                    else {
                        continue;
                    };
                    let page = Page::Compare((base, a), (head, b), !direct);
                    self.agent_changes(event.agent, page, None, cx);
                }
                What::WorkingTree { unstaged } => {
                    self.agent_changes(event.agent, Page::WorkingTree, Some(unstaged), cx)
                }
                What::Open => {}
            }
        }
    }

    /// `#12` on GitHub, `!12` on GitLab, for this tab's host whatever another window last drew.
    fn number(&self, iid: u64) -> String {
        let sign = if self.web.as_ref().is_some_and(|w| w.github) {
            '#'
        } else {
            '!'
        };
        format!("{sign}{iid}")
    }

    /// `agent` wrote `count` pending comments on request `iid`: the request on screen reads them
    /// in, and a toast offers to go to them.
    fn agent_left(&mut self, agent: String, iid: u64, count: usize, cx: &mut Context<Self>) {
        if self.requests.current == Some(iid) {
            self.reload_threads(cx);
        }
        let number = self.number(iid);
        let act = Act::Review {
            iid,
            agent: agent.clone(),
        };
        let say = move |n| {
            let comments = super::plural(n, "pending comment");
            format!("{agent} left {comments} on {number}")
        };
        self.toast_act(act, count, say, cx);
    }

    /// `agent` gathered `iids` (#148): each becomes a row under Merge requests, in that order, not
    /// loaded; the toasts of its comments on them make way for one that offers the first.
    fn agent_gathered(&mut self, agent: String, iids: Vec<u64>, cx: &mut Context<Self>) {
        let Some(&first) = iids.first() else {
            return;
        };
        for &iid in &iids {
            self.open_quietly(Page::Request(iid));
            if !self.request_loaded(iid) {
                self.agent_fresh.insert(iid);
            }
        }
        self.save_nav();
        self.toast_drop(
            |act| matches!(act, Act::Review { iid, agent: a } if *a == agent && iids.contains(iid)),
            cx,
        );
        let act = Act::Gathered {
            iid: first,
            agent: agent.clone(),
        };
        let say = move |n| format!("{agent} gathered {}", super::plural(n, "merge request"));
        self.toast_act(act, iids.len(), say, cx);
    }

    /// `agent` opened its changes (#149): `page` a row under Branches, quietly, and a toast that
    /// shows it — the working tree with what is staged or not as the agent asked.
    fn agent_changes(
        &mut self,
        agent: String,
        page: Page,
        unstaged: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = page.item() else {
            return;
        };
        // What it is, so that two are told apart.
        let what = match &page {
            Page::Compare(base, head, _) => format!("{} … {}", base.0, head.0),
            _ => "the working tree".to_owned(),
        };
        self.open_quietly(page);
        self.save_nav();
        let act = Act::Show {
            item,
            unstaged,
            agent: agent.clone(),
        };
        let say = move |_| format!("{agent} opened its changes: {what}");
        self.toast_act(act, 1, say, cx);
    }

    /// Show on that toast: the row's page on screen.
    pub(super) fn show_changes(
        &mut self,
        item: &Item,
        unstaged: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        if let Some(on) = unstaged {
            self.wt.only_unstaged = on;
        }
        self.show_open(item, cx);
        // On screen already, it is read again as asked.
        if let Some(on) = unstaged
            && matches!(
                self.diff.as_ref().map(|d| &d.header),
                Some(super::Header::WorkingTree { only_unstaged }) if *only_unstaged != on
            )
        {
            self.set_only_unstaged(on, cx);
        }
    }

    /// Review on that toast: request `iid` comes on screen at the first, in the diff's order, of
    /// the pending comments `agent` left on it, and ⌥N goes on through them; with none, on
    /// Changes.
    pub(super) fn review_agent(&mut self, iid: u64, agent: &str, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self
            .agent_marks
            .iter()
            .filter(|m| m.iid == iid && m.agent == agent)
            .filter_map(|m| m.draft)
            .collect();
        self.go_to(
            crate::links::Target::Request {
                iid,
                note: None,
                line: None,
            },
            cx,
        );
        if ids.is_empty() {
            self.show_section(super::nav::Section::Changes, cx);
            return cx.notify();
        }
        self.spot = Some(Spot::Drafts(ids));
        // Loaded with its comments in: now; else as they come in (`sync_request`).
        if self.request_loaded(iid) && !self.requests.reading {
            self.resolve_spot(true, cx);
        }
        cx.notify();
    }

    /// A gathered request's row opened the first time: at its agent's first pending comment.
    pub(super) fn open_gathered(&mut self, iid: u64, cx: &mut Context<Self>) {
        let agent = self
            .gathered(iid)
            .map(|m| m.agent.clone())
            .unwrap_or_default();
        self.review_agent(iid, &agent, cx);
        // From its row, not a link: no way back to another space.
        self.nav.from = None;
    }

    /// The agent's line on request `iid`, when one gathered it: the latest.
    pub(super) fn gathered(&self, iid: u64) -> Option<&Mark> {
        self.agent_marks
            .iter()
            .rev()
            .find(|m| m.iid == iid && m.note.is_some())
    }

    /// The pending comments `agent` left on request `iid` that still wait there: as the host
    /// lists them when the request is loaded, else as the marks say.
    pub(super) fn agent_waiting(&self, iid: u64, agent: &str) -> usize {
        let ids = self
            .agent_marks
            .iter()
            .filter(|m| m.iid == iid && m.agent == agent)
            .filter_map(|m| m.draft);
        if self.requests.current == Some(iid) && !self.requests.reading {
            ids.filter(|id| self.requests.drafts.iter().any(|d| d.id == *id))
                .count()
        } else {
            ids.count()
        }
    }

    /// An agent's mark: its letter on the hue of its newest chat here, else the accent.
    pub(super) fn agent_mark(&self, agent: &str) -> (char, gpui::Rgba) {
        match self
            .agent_chats
            .iter()
            .filter(|c| c.agent == agent)
            .max_by_key(|c| c.beat)
        {
            Some(chat) => self.chat_mark(&chat.session),
            None => (agent.chars().next().unwrap_or('?'), super::theme::accent()),
        }
    }

    /// The agent that wrote pending comment `id` of the request open, when one did.
    pub(super) fn draft_agent(&self, id: u64) -> Option<&str> {
        let iid = self.requests.current?;
        self.agent_marks
            .iter()
            .find(|m| m.iid == iid && m.draft == Some(id))
            .map(|m| m.agent.as_str())
    }

    /// The Review on a gathering's toast: its first request, opened from its row.
    pub(super) fn review_gathered(&mut self, iid: u64, cx: &mut Context<Self>) {
        self.show_open(&Item::Request(iid), cx);
    }
}
