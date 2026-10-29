//! System notifications (#26): what the Inbox and the agents bring while no GitLance window is in
//! front goes to the system's notifications as well — Settings → General, off by default. GPUI
//! posts them (Notification Center on macOS, a toast on Windows, the freedesktop service on
//! Linux) and hands a click back; the click brings GitLance forward with that item open. Where the
//! system has none, or refuses, nothing happens: the Inbox stays the source.

use super::Workspace;
use super::shell::Shell;
use super::toast::Act;
use gpui::{App, Context, Global, SharedString, SystemNotification, WeakEntity};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// What GitLance is to the system: the macOS bundle id, the Windows AppUserModelID.
pub const IDENTITY: &str = "dev.gitlance.app";

/// More than this many at once are said as one: "5 new in payments-service".
const MOST_APART: usize = 3;

/// What a click on one opens.
#[derive(Clone, Debug)]
pub(super) enum Open {
    /// The Inbox's notice with this key, its request at its comment.
    Notice(String),
    /// The Inbox.
    Inbox,
    /// What the toast of the same news does (an agent's).
    Act(Act),
    /// The Agents window, its answers in it.
    Agents,
}

/// The notifications posted, by tag: the tab each is about and what a click opens there.
#[derive(Default)]
struct Posted(HashMap<SharedString, (WeakEntity<Workspace>, Open)>);

impl Global for Posted {}

thread_local! {
    /// The windows in front, by the shell's id: while there is one, nothing goes to the system.
    static FRONT: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
}

/// Window `id` came to the front, or left it.
pub(super) fn set_front(id: u64, front: bool) {
    FRONT.with(|f| {
        let mut f = f.borrow_mut();
        if front {
            f.insert(id);
        } else {
            f.remove(&id);
        }
    });
}

fn in_front() -> bool {
    FRONT.with(|f| !f.borrow().is_empty())
}

/// At launch: what a click does, and who GitLance is when Settings sends notifications.
pub fn install(cx: &mut App) {
    if crate::storage::Settings::load().system_notifications {
        identify(cx);
    }
    cx.on_system_notification_response(|response, cx| clicked(&response.tag, cx));
}

/// Who GitLance is to the system's notifications. Only once they are on: on Windows it is also the
/// taskbar's id for its windows, which a GitLance pinned before would no longer group with.
pub(super) fn identify(cx: &mut App) {
    cx.set_app_identity(IDENTITY, "GitLance");
}

/// A click: the tab it is about comes forward in its window, then what it opens.
fn clicked(tag: &str, cx: &mut App) {
    let Some((ws, open)) = cx.default_global::<Posted>().0.remove(tag) else {
        return;
    };
    let Some(ws) = ws.upgrade() else {
        return;
    };
    cx.activate(true);
    for window in cx.windows() {
        let Some(shell) = window.downcast::<Shell>() else {
            continue;
        };
        let shown = shell.update(cx, |shell, window, cx| {
            let ix = shell.tab_ix(ws.entity_id())?;
            window.activate_window();
            shell.select(ix, window, cx);
            Some(())
        });
        if matches!(shown, Ok(Some(()))) {
            break;
        }
    }
    ws.update(cx, |ws, cx| ws.open_system_note(open, cx));
}

impl Workspace {
    /// `title` and `body` to the system's notifications, when Settings says so and no GitLance
    /// window is in front; `tag` the same again replaces it there. A click opens `open` here.
    pub(super) fn notify_system(
        &mut self,
        tag: String,
        title: String,
        body: String,
        open: Open,
        cx: &mut Context<Self>,
    ) {
        let snapshot =
            cfg!(feature = "snapshot") && std::env::var_os("GITLANCE_SNAPSHOT").is_some();
        if !self.settings.system_notifications || in_front() || snapshot {
            return;
        }
        let tag = SharedString::from(format!("{}:{tag}", cx.entity_id()));
        let ws = cx.entity().downgrade();
        cx.default_global::<Posted>()
            .0
            .insert(tag.clone(), (ws, open));
        cx.show_system_notification(SystemNotification {
            tag,
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
        });
    }

    /// New notices in the Inbox (#26): each said apart, or many as one.
    pub(super) fn notify_notices(
        &mut self,
        fresh: Vec<super::notifications::Notice>,
        cx: &mut Context<Self>,
    ) {
        let project = self.tab_title();
        if fresh.len() > MOST_APART {
            let title = format!("{} new in {project}", fresh.len());
            let body = fresh
                .iter()
                .take(MOST_APART)
                .map(|n| format!("{} {}", n.who, n.kind_verb()))
                .collect::<Vec<_>>()
                .join(" · ");
            return self.notify_system("inbox".to_owned(), title, body, Open::Inbox, cx);
        }
        for n in fresh {
            let title = format!(
                "{} {} · {}",
                n.who,
                n.kind_verb(),
                super::words::number(n.iid)
            );
            let body = format!("{}\n{project}", n.title);
            self.notify_system(n.key.clone(), title, body, Open::Notice(n.key), cx);
        }
    }

    /// An agent's answers (#26): comments it addressed or replied to since `before` (each one's
    /// id, state and replies then), said by the agent that took them.
    pub(super) fn notify_answers(
        &mut self,
        before: &[(u64, crate::review::State, usize)],
        cx: &mut Context<Self>,
    ) {
        use crate::review::State;
        let mut by: HashMap<String, usize> = HashMap::new();
        for c in &self.comments {
            let Some(&(_, state, replies)) = before.iter().find(|(id, ..)| *id == c.id) else {
                continue;
            };
            let addressed = c.state == State::Addressed && state != State::Addressed;
            if !addressed && c.replies.len() <= replies {
                continue;
            }
            let session = c.sent.as_ref().map_or("", |s| s.session.as_str());
            let agent = self
                .agent_chats
                .iter()
                .find(|chat| chat.session == session)
                .map_or("An agent", |chat| chat.agent.as_str());
            *by.entry(agent.to_owned()).or_default() += 1;
        }
        let project = self.tab_title();
        for (agent, n) in by {
            let title = match n {
                1 => format!("{agent} answered your comment"),
                n => format!("{agent} answered {n} of your comments"),
            };
            let tag = format!("answers:{agent}");
            self.notify_system(tag, title, project.clone(), Open::Agents, cx);
        }
    }

    /// What a click on one opens, its tab on screen.
    fn open_system_note(&mut self, open: Open, cx: &mut Context<Self>) {
        match open {
            Open::Notice(key) => match self.notices.list.iter().position(|n| n.key == key) {
                Some(ix) => self.open_notice(ix, cx),
                None => self.go_space(super::spaces::Space::Inbox, cx),
            },
            Open::Inbox => self.go_space(super::spaces::Space::Inbox, cx),
            Open::Act(act) => self.do_toast_act(act, cx),
            Open::Agents => {
                if self.tool() != Some(super::tools::Tool::Agents) {
                    self.toggle_tool(super::tools::Tool::Agents, cx);
                }
            }
        }
        cx.notify();
    }
}
