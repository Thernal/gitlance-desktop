//! GitLab links opened in the app (`crate::links`): from ⌘K, where a pasted link is one item, and
//! from links in comments (⌘-click still opens the browser). A link into the open repository's
//! project acts here; into another project, the recent repository with that remote opens first.

use super::Workspace;
use crate::git::Repo;
use crate::links::{self, Link, Target};
use gpui::Context;

/// Where to go once what is needed has loaded: a comment of a request, or a line of its diff.
#[derive(Clone, Debug)]
pub enum Spot {
    Note(u64),
    Line(links::DiffLine),
    /// The first of these pending comments in the diff's order: an agent's (#150).
    Drafts(Vec<u64>),
}

impl Workspace {
    /// Opens `link` here; says why when it cannot.
    pub(super) fn open_link(&mut self, link: Link, cx: &mut Context<Self>) {
        if self.web.as_ref().is_some_and(|w| w.base == link.base) {
            return self.go_to(link.target, cx);
        }
        let other = self.recent.iter().find(|path| {
            Repo::open(path)
                .ok()
                .and_then(|r| r.web_remote())
                .is_some_and(|w| w.base == link.base)
        });
        match other.cloned() {
            Some(path) => self.request_open(path, Some(link.target), cx),
            None => {
                self.error = Some(
                    super::keys::text(&format!(
                        "{} is not open here — open its repository once (⌘O), then its links work.",
                        link.project()
                    ))
                    .into_owned()
                    .into(),
                );
            }
        }
        cx.notify();
    }

    /// A link clicked in a comment: opened here when it is a GitLab link this app can follow.
    pub(super) fn open_url_here(&mut self, url: &str, cx: &mut Context<Self>) -> bool {
        let Some(link) = links::parse(url) else {
            return false;
        };
        let known = self.web.as_ref().is_some_and(|w| w.base == link.base);
        if !known {
            return false;
        }
        self.open_link(link, cx);
        true
    }

    /// Goes to `target` in the open repository.
    pub(super) fn go_to(&mut self, target: Target, cx: &mut Context<Self>) {
        match target {
            Target::Request { iid, note, line } => {
                if self.requests.find(iid).is_none() {
                    // The list is still loading, or it is not an open one (merged, closed: read
                    // on its own, #114): come back when it is in.
                    if self.requests.loading || !self.requests.tried.contains(&iid) {
                        self.pending_link = Some(Target::Request { iid, note, line });
                        if !self.requests.loading {
                            self.read_left(vec![iid], cx);
                        }
                    } else {
                        self.error = Some(
                            format!("{} could not be read.", super::words::number(iid)).into(),
                        );
                    }
                    return cx.notify();
                }
                self.spot = note.map(Spot::Note).or(line.map(Spot::Line));
                // Loaded under another page: it only comes on screen (#71).
                if self.request_loaded(iid) {
                    self.arrive(super::nav::Page::Request(iid));
                    self.resolve_spot(true, cx);
                } else {
                    self.select_request(iid, cx);
                }
            }
            Target::Commit(sha) => {
                match self
                    .commits
                    .iter()
                    .position(|c| c.id.to_string().starts_with(&sha))
                {
                    Some(ix) => {
                        self.select_commit(ix, cx);
                        self.follow_loaded();
                    }
                    // Elsewhere in the repository: the first branch that has it, at that commit.
                    None => match self.branch_with(&sha) {
                        Some((ix, id)) => self.open_branch(ix, Some(id), cx),
                        None => {
                            self.error = Some(
                                format!(
                                    "Commit {} is not on any branch here (or not fetched yet).",
                                    &sha[..sha.len().min(7)]
                                )
                                .into(),
                            )
                        }
                    },
                }
            }
            Target::Branch(name) => {
                let ix = self
                    .branches
                    .iter()
                    .position(|b| b.name == name)
                    .or_else(|| {
                        self.branches
                            .iter()
                            .position(|b| b.name.split_once('/').is_some_and(|(_, n)| n == name))
                    });
                match ix {
                    Some(ix) => self.open_branch(ix, None, cx),
                    None => {
                        self.error =
                            Some(format!("No branch {name} here (fetch to see it).").into())
                    }
                }
            }
        }
        cx.notify();
    }

    /// A local branch (else a remote one) whose history has commit `sha`, and its full id.
    fn branch_with(&self, sha: &str) -> Option<(usize, git2::Oid)> {
        let repo = Repo::open(self.root.as_ref()?).ok()?;
        let id = repo.resolve(sha).ok()?;
        let mut order: Vec<usize> = (0..self.branches.len()).collect();
        order.sort_by_key(|&ix| self.branches[ix].kind != crate::git::RefKind::Local);
        order
            .into_iter()
            .find(|&ix| repo.contains(self.branches[ix].tip, id))
            .map(|ix| (ix, id))
    }

    /// After a repository opened for a link: go on to what the link points at, once what it needs
    /// is loaded (the requests, the commits, the branches).
    pub(super) fn follow_pending_link(&mut self, cx: &mut Context<Self>) {
        let ready = match &self.pending_link {
            None => return,
            Some(Target::Request { .. }) => self.root.is_some() && !self.requests.loading,
            Some(Target::Commit(_)) => !self.commits.is_empty(),
            Some(Target::Branch(_)) => !self.branches.is_empty(),
        };
        if ready && let Some(target) = self.pending_link.take() {
            self.go_to(target, cx);
        }
    }

    /// Goes to the spot a link asked for, once the request's diff and discussions are in; keeps it
    /// while they are not. `last`: what it needs is all in — not found, it is let go, and said.
    pub(super) fn resolve_spot(&mut self, last: bool, cx: &mut Context<Self>) {
        let Some(spot) = self.spot.clone() else {
            return;
        };
        let place = match &spot {
            Spot::Note(id) => self
                .requests
                .threads
                .iter()
                .enumerate()
                .find(|(_, t)| t.notes.iter().any(|n| n.id == *id))
                .and_then(|(ix, t)| {
                    let line = t.new_line.or(t.old_line)?;
                    Some((ix, t.path.clone()?, line, t.new_line.is_none()))
                })
                .map(|(ix, path, line, old)| (Some(ix), None, path, line, old)),
            Spot::Drafts(ids) => self
                .draft_order()
                .into_iter()
                .map(|(.., ix)| ix)
                .find(|&ix| ids.contains(&self.requests.drafts[ix].id))
                .and_then(|ix| {
                    let (path, new, old) = self.draft_place(&self.requests.drafts[ix])?;
                    Some((None, Some(ix), path, new.or(old)?, new.is_none()))
                }),
            Spot::Line(l) => self.diff.as_ref().and_then(|d| {
                let file = d
                    .files
                    .iter()
                    .find(|f| links::path_hash(f.path()) == l.file)?;
                let (line, old) = match (l.new, l.old) {
                    (Some(n), _) => (n, false),
                    (None, Some(o)) => (o, true),
                    _ => (1, false),
                };
                Some((None, None, file.path().to_owned(), line, old))
            }),
        };
        let Some((thread, draft, path, line, old)) = place else {
            if last {
                self.spot = None;
                self.error = Some(
                    match spot {
                        Spot::Note(_) => "That comment is not on a line of this merge request.",
                        Spot::Line(_) => "That file is not in this merge request's diff.",
                        Spot::Drafts(_) => {
                            "Those pending comments are not on this merge request any more."
                        }
                    }
                    .into(),
                );
            }
            return;
        };
        self.spot = None;
        if let Some(ix) = draft {
            // ⌥N goes on from it; the diff, not the overview, is where it is.
            self.draft_at = Some(ix);
            self.show_section(super::nav::Section::Changes, cx);
        }
        if let Some(ix) = thread {
            self.thread_at = Some(ix);
            if !self.mr_open(ix) {
                self.toggled_mr.insert(ix);
            }
        }
        self.jump_to_line(&path, line, old, cx);
    }
}
