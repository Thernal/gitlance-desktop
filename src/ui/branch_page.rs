//! A branch's page (#74): a branch that forks from the main line has the tabs Commits · Changes,
//! Changes being the branch against the main line since they parted, with one line of facts over
//! it — how far ahead and behind, whether it is pushed, its request — and Create merge request as
//! its primary action. A commit of it is a level under Commits; a run of its commits picked with
//! shift is one diff there. Its pages carry review marks, so your own work is reviewed before it
//! goes out. The main line itself has no base: its page is the commit browser it always was.

use super::nav::Page;
use super::{Header, NamedTip, Workspace, island, plural, px, theme};
use git2::Oid;
use gpui::{AnyElement, Context, div, prelude::*};

/// What a branch's page says of it, read when the branch loads.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// The branch it is a page of.
    pub refname: String,
    /// The main line it forks from, at its tip; none for the main line itself.
    pub base: Option<NamedTip>,
    /// Commits it has that the main line has not, and the other way round.
    pub ahead: usize,
    pub behind: usize,
    /// Its upstream, with how far it is ahead of it and behind it; none when it was never pushed.
    pub upstream: Option<(String, usize, usize)>,
    /// The files of its Changes, once read.
    pub files: Option<usize>,
}

impl Facts {
    /// Reads them for branch `name` (`refname`) at `tip`: its base is the main line's local branch,
    /// else its remote one.
    pub fn read(repo: &crate::git::Repo, refname: &str, name: &str, tip: Oid) -> Facts {
        let main = repo.default_branch();
        let base = main
            .as_deref()
            .filter(|m| name != *m && name != format!("origin/{m}"))
            .and_then(|m| {
                [m.to_owned(), format!("origin/{m}")]
                    .into_iter()
                    .find_map(|n| repo.resolve(&n).ok().map(|id| (n, id)))
            });
        let (ahead, behind) = base
            .as_ref()
            .and_then(|(_, b)| repo.ahead_behind(tip, *b))
            .unwrap_or_default();
        Facts {
            refname: refname.to_owned(),
            base,
            ahead,
            behind,
            upstream: repo.tracking(name),
            files: None,
        }
    }
}

impl Workspace {
    /// The branch loaded has the tabs Commits · Changes: it forks from the main line.
    pub(super) fn branch_tabbed(&self, refname: &str) -> bool {
        self.branch_page
            .as_ref()
            .is_some_and(|f| f.refname == refname && f.base.is_some())
    }

    /// The diff loaded is the branch page's own: the branch against the main line.
    pub(super) fn branch_compare(&self) -> bool {
        let (Some(f), Some(ix)) = (self.branch_page.as_ref(), self.branch) else {
            return false;
        };
        matches!(
            (self.diff.as_ref().map(|d| &d.header), &f.base, self.branches.get(ix)),
            (
                Some(Header::Compare { base, head, request: None, .. }),
                Some(main),
                Some(b),
            ) if base.1 == main.1 && head.1 == b.tip
        )
    }

    /// Shows the branch loaded against the main line since they parted: its Changes.
    pub(super) fn run_branch_compare(&mut self, cx: &mut Context<Self>) {
        let (Some(ix), Some(base)) = (
            self.branch,
            self.branch_page.as_ref().and_then(|f| f.base.clone()),
        ) else {
            return;
        };
        let Some(b) = self.branches.get(ix) else {
            return;
        };
        let head = (b.name.clone(), b.tip);
        self.run_compare(base, head, true, None, cx);
    }

    /// One line over a branch's Changes and Commits: ahead of the main line, behind it, where it
    /// is pushed, its request.
    pub(super) fn render_branch_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(Page::Branch(refname)) = self.nav_page() else {
            return None;
        };
        if !self.branch_tabbed(refname) {
            return None;
        }
        let f = self.branch_page.as_ref()?;
        let main = f.base.as_ref()?.0.trim_start_matches("origin/").to_owned();
        let remote = match &f.upstream {
            None => "not pushed".to_owned(),
            Some((up, 0, 0)) => format!("pushed to {}", up.split('/').next().unwrap_or(up)),
            Some((_, ahead, 0)) => format!("{} not pushed", plural(*ahead, "commit")),
            Some((up, _, behind)) => format!("{behind} behind {up}"),
        };
        let request = self
            .branch
            .and_then(|ix| self.request_of_branch(ix))
            .map_or_else(|| "none yet".to_owned(), |m| super::words::number(m.iid));
        let fact = |label: String, value: String| {
            div()
                .flex()
                .gap(px(6.))
                .child(div().text_color(theme::faint()).child(label))
                .child(div().child(value))
        };
        let ix = self.branch;
        Some(
            island()
                // Its link, the branch's name, a request (#162).
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                        if let Some(ix) = ix {
                            this.branch_context(ix, e.position, cx)
                        }
                    }),
                )
                .flex_none()
                .flex_row()
                .items_center()
                .gap(px(20.))
                .h(px(40.))
                .px(px(14.))
                .text_size(px(12.))
                .child(fact(format!("Ahead of {main}"), plural(f.ahead, "commit")))
                .child(fact("Behind".to_owned(), f.behind.to_string()))
                .child(fact("Remote".to_owned(), remote))
                .child(fact(super::words::say("Request").into_owned(), request))
                .into_any_element(),
        )
    }
}
