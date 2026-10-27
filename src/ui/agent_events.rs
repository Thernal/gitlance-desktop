//! What agents leave for the reviewer, as a tab sees it (#150): the marks that tell an agent's
//! pending comments from one's own, and the events of the tab's project — pending comments an
//! agent wrote on a request — taken once, each with a toast offering to review them. Nothing here
//! moves the screen until the reviewer asks.

use super::Workspace;
use super::goto::Spot;
use super::toast::Act;
use crate::agents::{Bus, What};
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
            // A host that gave no id still got one comment.
            if let What::Pending { iid, ids } = event.what {
                self.agent_left(event.agent, iid, ids.len().max(1), cx);
            }
        }
    }

    /// `agent` wrote `count` pending comments on request `iid`: the request on screen reads them
    /// in, and a toast offers to go to them.
    fn agent_left(&mut self, agent: String, iid: u64, count: usize, cx: &mut Context<Self>) {
        if self.requests.current == Some(iid) {
            self.reload_threads(cx);
        }
        let sign = if self.web.as_ref().is_some_and(|w| w.github) {
            '#'
        } else {
            '!'
        };
        let act = Act::Review {
            iid,
            agent: agent.clone(),
        };
        let say = move |n| {
            let comments = super::plural(n, "pending comment");
            format!("{agent} left {comments} on {sign}{iid}")
        };
        self.toast_act(act, count, say, cx);
    }

    /// Review on that toast: request `iid` comes on screen at the first, in the diff's order, of
    /// the pending comments `agent` left on it, and ⌥N goes on through them.
    pub(super) fn review_agent(&mut self, iid: u64, agent: &str, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self
            .agent_marks
            .iter()
            .filter(|m| m.iid == iid && m.agent == agent)
            .map(|m| m.draft)
            .collect();
        self.go_to(
            crate::links::Target::Request {
                iid,
                note: None,
                line: None,
            },
            cx,
        );
        self.spot = Some(Spot::Drafts(ids));
        // Loaded with its comments in: now; else as they come in (`sync_request`).
        if self.request_loaded(iid) && !self.requests.reading {
            self.resolve_spot(true, cx);
        }
        cx.notify();
    }

    /// The agent that wrote pending comment `id` of the request open, when one did.
    pub(super) fn draft_agent(&self, id: u64) -> Option<&str> {
        let iid = self.requests.current?;
        self.agent_marks
            .iter()
            .find(|m| m.iid == iid && m.draft == id)
            .map(|m| m.agent.as_str())
    }
}
