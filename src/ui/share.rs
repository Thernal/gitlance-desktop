//! Links to what GitLance shows, copied without the browser (#162): a merge request's and its
//! number, a branch's page on the host, a comparison's, a commit's, a file's in a diff the host
//! anchors, and the page on screen (⌘K, ⌥⇧⌘C). A toast says what was copied; what has no link (a
//! branch only on this Mac, what is not committed) says why instead.

use super::menu::{Act, Entry};
use super::nav::{Item, Page};
use super::spaces::Space;
use super::{Workspace, format, words};
use crate::git::{RefKind, WebRemote};
use gpui::{Context, Pixels, Point};

/// No GitLab or GitHub remote: nothing here has a link.
const NO_HOST: &str = "No GitLab or GitHub remote: nothing here has a link";

/// A link, and what it is in the toast's words: `!398`, `branch feat/x`.
pub(super) struct Link {
    pub url: String,
    pub what: String,
}

impl Link {
    pub(super) fn act(self) -> Act {
        Act::CopySaid {
            said: format!("Link to {}", self.what),
            text: self.url,
        }
    }

    /// The same page at `anchor`, a file of its diff.
    fn at(self, anchor: &str, name: &str) -> Link {
        Link {
            url: format!("{}#{anchor}", self.url),
            what: format!("{name} in {}", self.what),
        }
    }
}

impl Workspace {
    fn web_or_why(&self) -> Result<&WebRemote, String> {
        self.web.as_ref().ok_or_else(|| NO_HOST.to_owned())
    }

    /// Request `iid`'s page: the one the host gave, else made from the remote.
    fn request_link(&self, web: &WebRemote, iid: u64) -> Link {
        Link {
            url: self
                .requests
                .find(iid)
                .map_or_else(|| web.request(iid), |m| m.url.clone()),
            what: words::number(iid),
        }
    }

    /// Request `iid`'s diff: GitLab's Changes, GitHub's Files, or one commit of it.
    fn request_diff_link(&self, web: &WebRemote, iid: u64, commit: Option<git2::Oid>) -> Link {
        let page = self.request_link(web, iid);
        let url = match (web.github, commit) {
            (true, None) => format!("{}/files", page.url),
            (true, Some(sha)) => format!("{}/commits/{sha}", page.url),
            (false, None) => format!("{}/diffs", page.url),
            (false, Some(sha)) => format!("{}/diffs?commit_id={sha}", page.url),
        };
        let what = match commit {
            Some(sha) => format!("{}, commit {}", page.what, format::short(sha)),
            None => page.what,
        };
        Link { url, what }
    }

    /// Branch, tag or commit `name` as the host has it, its remote dropped; why not when it is a
    /// branch only on this Mac.
    fn on_host(&self, name: &str) -> Result<String, String> {
        let Some(branch) = self.branches.iter().find(|b| b.name == name) else {
            return Ok(name.to_owned());
        };
        if branch.kind == RefKind::Remote {
            return Ok(name.split_once('/').map_or(name, |(_, n)| n).to_owned());
        }
        let pushed = self.branches.iter().any(|b| {
            b.kind == RefKind::Remote && b.name.split_once('/').is_some_and(|(_, n)| n == name)
        });
        if pushed {
            Ok(name.to_owned())
        } else {
            Err(format!("{name} is only on this Mac: push it for a link"))
        }
    }

    /// Branch `name`'s page on the host.
    pub(super) fn branch_link(&self, name: &str) -> Result<Link, String> {
        let web = self.web_or_why()?;
        let name = self.on_host(name)?;
        Ok(Link {
            url: web.tree(&name, ""),
            what: format!("branch {name}"),
        })
    }

    /// Whether the host has commit `sha`: a remote branch or a request it fetched has it.
    fn pushed(&self, sha: git2::Oid) -> bool {
        let Some(repo) = self
            .root
            .as_ref()
            .and_then(|r| crate::git::Repo::open(r).ok())
        else {
            return true;
        };
        self.branches
            .iter()
            .filter(|b| b.kind != RefKind::Local)
            .any(|b| repo.contains(b.tip, sha))
    }

    /// Commit `sha`'s page on the host; why not when only this Mac has it.
    fn pushed_commit_link(&self, sha: git2::Oid) -> Result<Link, String> {
        let web = self.web_or_why()?;
        if self.pushed(sha) {
            Ok(Self::commit_link(web, sha))
        } else {
            Err(format!(
                "Commit {} is only on this Mac: push it for a link",
                format::short(sha)
            ))
        }
    }

    fn commit_link(web: &WebRemote, sha: git2::Oid) -> Link {
        Link {
            url: web.commit(&sha.to_string()),
            what: format!("commit {}", format::short(sha)),
        }
    }

    /// The commits from `oldest` to `newest` (a run picked with shift) as one diff on the host:
    /// GitHub's pull request shows a run among its files; GitLab's has no such view, so its
    /// comparison of the commit before the run with the newest shows the same diff.
    fn run_link(
        &self,
        web: &WebRemote,
        iid: Option<u64>,
        newest: git2::Oid,
        oldest: git2::Oid,
    ) -> Result<Link, String> {
        let range = format!("{}..{}", format::short(oldest), format::short(newest));
        if !self.pushed(newest) {
            return Err(format!(
                "Commits {range} are only on this Mac: push them for a link"
            ));
        }
        let before = self
            .root
            .as_ref()
            .and_then(|r| crate::git::Repo::open(r).ok())
            .and_then(|repo| repo.parent_of(oldest).ok())
            .ok_or_else(|| format!("Commits {range} start the history: no comparison has them"))?;
        let compare = web.compare(&before.to_string(), &newest.to_string(), true);
        Ok(match iid {
            Some(iid) => {
                let request = self.request_link(web, iid);
                Link {
                    url: if web.github {
                        format!("{}/files/{before}..{newest}", request.url)
                    } else {
                        compare
                    },
                    what: format!("{}, commits {range}", request.what),
                }
            }
            None => Link {
                url: compare,
                what: format!("commits {range}"),
            },
        })
    }

    /// Versions `from` and `to` of request `iid` compared on the host: GitLab's Compare versions
    /// (the newer named by its id, the older by its head); GitHub's comparison of the two heads,
    /// as its force-push events link it.
    fn versions_link(&self, web: &WebRemote, iid: u64, from: usize, to: usize) -> Link {
        let request = self.request_link(web, iid);
        let (Some(a), Some(b)) = (self.versions.get(from), self.versions.get(to)) else {
            return request;
        };
        let (a, b) = if a.number <= b.number { (a, b) } else { (b, a) };
        let what = format!("{}, v{} → v{}", request.what, a.number, b.number);
        if web.github {
            return Link {
                url: web.compare(&a.tip.to_string(), &b.tip.to_string(), true),
                what,
            };
        }
        let id = self
            .requests
            .version_ids
            .as_ref()
            .filter(|(of, _)| *of == iid)
            .and_then(|(_, ids)| ids.iter().find(|(head, _)| *head == b.tip))
            .map(|(_, id)| *id)
            .filter(|id| *id > 0);
        let url = match id {
            Some(id) if a.number < b.number => {
                format!("{}/diffs?diff_id={id}&start_sha={}", request.url, a.tip)
            }
            Some(id) => format!("{}/diffs?diff_id={id}", request.url),
            None => format!("{}/diffs", request.url),
        };
        Link { url, what }
    }

    /// What `head` changed against `base` on the host.
    fn compare_link(&self, base: &str, head: &str, direct: bool) -> Result<Link, String> {
        let web = self.web_or_why()?;
        let (base, head) = (self.on_host(base)?, self.on_host(head)?);
        let dots = if direct { ".." } else { "..." };
        Ok(Link {
            url: web.compare(&base, &head, direct),
            what: format!("{base}{dots}{head}"),
        })
    }

    /// The page on screen as a link, or why it has none: ⌘K's and ⌥⇧⌘C's.
    pub(super) fn page_link(&self) -> Result<Link, String> {
        let web = self.web_or_why()?;
        match self.nav.space {
            Space::Inbox => match self.inbox_request() {
                Some(iid) => Ok(self.request_link(web, iid)),
                None => Err("Nothing here has a link: pick a notice of a merge request".into()),
            },
            Space::Code => {
                let rev = match &self.code.rev {
                    Some((name, sha)) => self.on_host(name).unwrap_or_else(|_| sha.to_string()),
                    None => {
                        let head = self.branches.iter().find(|b| b.is_head);
                        let Some(head) = head else {
                            return Err("No branch is checked out: no link to read it at".into());
                        };
                        self.on_host(&head.name)?
                    }
                };
                Ok(match self.nav_page() {
                    Some(Page::File(path)) => Link {
                        url: web.file(&rev, path),
                        what: format!("{path} at {rev}"),
                    },
                    Some(Page::Folder(path)) => Link {
                        url: web.tree(&rev, path),
                        what: format!("{path} at {rev}"),
                    },
                    _ => Link {
                        url: web.tree(&rev, ""),
                        what: format!("the project at {rev}"),
                    },
                })
            }
            Space::Branches | Space::Requests => match self.nav_page() {
                Some(page) => self.link_of(web, page),
                None => Err("Nothing here has a link: open a branch or a merge request".into()),
            },
        }
    }

    /// Page `page` of Branches or Merge requests as a link.
    fn link_of(&self, web: &WebRemote, page: &Page) -> Result<Link, String> {
        match page {
            Page::Request(iid) => Ok(self.request_link(web, *iid)),
            Page::RequestCommit(iid, sha) => Ok(self.request_diff_link(web, *iid, Some(*sha))),
            Page::RequestCommits(iid, newest, oldest) => {
                self.run_link(web, Some(*iid), *newest, *oldest)
            }
            Page::RequestVersions(iid, from, to) => Ok(self.versions_link(web, *iid, *from, *to)),
            Page::Branch(refname) => {
                let branch = self.branches.iter().find(|b| b.refname == *refname);
                let name = branch.map_or(refname.as_str(), |b| b.name.as_str());
                self.branch_link(name)
            }
            Page::BranchCommit(_, sha) => self.pushed_commit_link(*sha),
            Page::BranchCommits(_, newest, oldest) => self.run_link(web, None, *newest, *oldest),
            // What the branch was before a rewrite is this Mac's alone: the host keeps the newest.
            Page::Versions(..) => {
                Err("A branch's earlier versions are only on this Mac: no link".into())
            }
            Page::Compare((base, _), (head, _), direct) => self.compare_link(base, head, *direct),
            Page::WorkingTree => Err("What is not committed is only on this Mac".into()),
            Page::File(_) | Page::Folder(_) => Err("Open it in Code for its link".into()),
        }
    }

    /// What the page on screen is, for ⌘K's "Copy link to this …".
    pub(super) fn page_kind(&self) -> &'static str {
        match (self.nav.space, self.nav_page()) {
            (Space::Inbox, _) => "this merge request",
            (Space::Code, Some(Page::File(_))) => "this file",
            (Space::Code, Some(Page::Folder(_))) => "this folder",
            (Space::Code, _) => "this project",
            (_, Some(Page::Request(_))) => "this merge request",
            (_, Some(Page::RequestCommit(..) | Page::BranchCommit(..))) => "this commit",
            (_, Some(Page::RequestCommits(..) | Page::BranchCommits(..))) => "these commits",
            (_, Some(Page::RequestVersions(..) | Page::Versions(..))) => "these versions",
            (_, Some(Page::Branch(_))) => "this branch",
            (_, Some(Page::Compare(..))) => "this comparison",
            _ => "this page",
        }
    }

    /// ⌘K's and ⌥⇧⌘C's: the page on screen's link copied, or why it has none.
    pub(super) fn copy_page_link(&mut self, cx: &mut Context<Self>) {
        match self.page_link() {
            Ok(link) => self.run_act(link.act(), cx),
            Err(why) => self.toast_info(why.into(), cx),
        }
    }

    /// The request the Inbox shows: the one of the notice picked.
    fn inbox_request(&self) -> Option<u64> {
        match self.inbox_current()? {
            super::inbox::InboxRow::Notice(ix) => self.notices.list.get(ix).map(|n| n.iid),
            super::inbox::InboxRow::Agent(_) => None,
        }
    }

    /// A request's right click, wherever its row is: its link, its number, the host.
    pub(super) fn request_context(&mut self, iid: u64, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(web) = self.web.clone() else {
            return;
        };
        let link = self.request_link(&web, iid);
        let number = link.what.clone();
        let url = link.url.clone();
        let groups = vec![
            vec![
                Entry::new("Copy link", link.act()).key("↗"),
                Entry::new(
                    format!("Copy {number}"),
                    Act::CopySaid {
                        text: number.clone(),
                        said: number,
                    },
                ),
            ],
            vec![Entry::new(
                format!("Open in {} ↗", web.name()),
                Act::OpenUrl(url),
            )],
        ];
        self.open_menu(at, groups, cx);
    }

    /// An open row's right click under Branches or Merge requests: its link.
    pub(super) fn item_context(&mut self, item: &Item, at: Point<Pixels>, cx: &mut Context<Self>) {
        let link = match item {
            Item::Request(iid) => return self.request_context(*iid, at, cx),
            Item::Branch(refname) => {
                if let Some(ix) = self.branches.iter().position(|b| b.refname == *refname) {
                    self.branch_context(ix, at, cx);
                }
                return;
            }
            Item::Compare(..) => match self.nav.opened(item).map(|o| &o.page) {
                Some(Page::Compare((base, _), (head, _), direct)) => {
                    self.compare_link(base, head, *direct)
                }
                _ => return,
            },
            Item::WorkingTree => return,
        };
        let groups = match (link, &self.web) {
            (Ok(link), Some(web)) => {
                let open = Entry::new(
                    format!("Open in {} ↗", web.name()),
                    Act::OpenUrl(link.url.clone()),
                );
                vec![
                    vec![Entry::new("Copy link", link.act()).key("↗")],
                    vec![open],
                ]
            }
            (Err(_), Some(_)) => vec![vec![Entry::off("No link: only on this Mac")]],
            _ => return,
        };
        self.open_menu(at, groups, cx);
    }

    /// The link of file `path` in the diff on screen, where the host anchors its files.
    pub(super) fn diff_file_link(&self, path: &str) -> Option<Link> {
        let web = self.web.as_ref()?;
        let page = match self.loaded_page()? {
            Page::Request(iid) => self.request_diff_link(web, iid, None),
            page @ (Page::RequestCommit(..)
            | Page::RequestCommits(..)
            | Page::RequestVersions(..)
            | Page::BranchCommit(..)
            | Page::BranchCommits(..)
            | Page::Compare(..)) => self.link_of(web, &page).ok()?,
            _ => return None,
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        Some(page.at(&web.file_anchor(path), name))
    }

    /// A commit's link entry, for its right click: Copy link, or why there is none.
    pub(super) fn commit_link_entry(&self, sha: git2::Oid) -> Option<Entry> {
        self.web.as_ref()?;
        Some(match self.pushed_commit_link(sha) {
            Ok(link) => Entry::new("Copy link", link.act()).key("↗"),
            Err(_) => Entry::off("No link: only on this Mac"),
        })
    }

    /// A branch's link entry, for its right click: Copy link, or why there is none.
    pub(super) fn branch_link_entry(&self, name: &str) -> Option<Entry> {
        self.web.as_ref()?;
        Some(match self.branch_link(name) {
            Ok(link) => Entry::new("Copy link", link.act()).key("↗"),
            Err(_) => Entry::off("No link: only on this Mac"),
        })
    }
}
