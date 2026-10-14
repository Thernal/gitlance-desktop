//! GitLab and GitHub links read as places in the app: a merge or pull request (its discussion, or a
//! line of its diff), a commit, a branch. Pure parsing; `ui::palette` and comment links act on what
//! this finds.

/// What a link points at inside its project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Request {
        iid: u64,
        /// `#note_<id>`: a comment of one of its discussions.
        note: Option<u64>,
        /// `diffs#<sha1 of the path>_<old>_<new>`: a line of its diff.
        line: Option<DiffLine>,
    },
    Commit(String),
    Branch(String),
}

/// A line of a diff as GitLab writes it in a link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    /// SHA-1 of the file's path, in hex.
    pub file: String,
    pub old: Option<u32>,
    pub new: Option<u32>,
}

/// A link into a project: `https://host/group/project` and what it points at there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub base: String,
    pub target: Target,
    /// A GitHub link: its requests are written `#12`.
    pub github: bool,
}

impl Link {
    /// The project as people say it: `group/project`.
    pub fn project(&self) -> &str {
        self.base
            .split_once("://")
            .and_then(|(_, rest)| rest.split_once('/'))
            .map_or(self.base.as_str(), |(_, path)| path)
    }

    /// What it is, in a few words: `!12`, `!12, a comment`, `commit 1a2b3c4`, `branch feat/x`.
    pub fn describe(&self) -> String {
        let sigil = if self.github { '#' } else { '!' };
        match &self.target {
            Target::Request { iid, note, line } => match (note, line) {
                (Some(_), _) => format!("{sigil}{iid}, a comment"),
                (None, Some(l)) => match l.new.or(l.old) {
                    Some(n) => format!("{sigil}{iid}, line {n}"),
                    None => format!("{sigil}{iid}"),
                },
                (None, None) => format!("{sigil}{iid}"),
            },
            Target::Commit(sha) => format!("commit {}", &sha[..sha.len().min(7)]),
            Target::Branch(name) => format!("branch {name}"),
        }
    }
}

/// Reads a GitLab link (`…/-/merge_requests/12`, `…/-/commit/<sha>`, `…/-/tree/<branch>`); `None`
/// for anything else.
pub fn parse(url: &str) -> Option<Link> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    let (url, fragment) = match url.split_once('#') {
        Some((u, f)) => (u, Some(f)),
        None => (url, None),
    };
    let url = url.split('?').next().unwrap_or(url);
    let Some((base, rest)) = url.split_once("/-/") else {
        return github(url, fragment);
    };
    let mut parts = rest.trim_end_matches('/').split('/');
    let target = match parts.next()? {
        "merge_requests" => {
            let iid = parts.next()?.parse().ok()?;
            let note = fragment
                .and_then(|f| f.strip_prefix("note_"))
                .and_then(|n| n.parse().ok());
            let line = fragment.and_then(diff_line);
            Target::Request { iid, note, line }
        }
        "commit" => {
            let sha = parts.next()?;
            (sha.len() >= 7 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| Target::Commit(sha.to_owned()))?
        }
        "tree" | "commits" => {
            let name: Vec<&str> = parts.collect();
            (!name.is_empty()).then(|| Target::Branch(name.join("/")))?
        }
        _ => return None,
    };
    Some(Link {
        base: base.trim_end_matches('/').to_owned(),
        target,
        github: false,
    })
}

/// A GitHub link: `https://host/owner/repo/pull/12[/files][#discussion_r<id>|#issuecomment-<id>]`,
/// `…/commit/<sha>`, `…/tree/<branch>`.
fn github(url: &str, fragment: Option<&str>) -> Option<Link> {
    let (scheme, rest) = url.split_once("://")?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let (host, owner, repo) = (parts.next()?, parts.next()?, parts.next()?);
    let target = match parts.next()? {
        "pull" => {
            let iid = parts.next()?.parse().ok()?;
            let note = fragment.and_then(|f| {
                f.strip_prefix("discussion_r")
                    .or_else(|| f.strip_prefix("issuecomment-"))
                    .or_else(|| f.strip_prefix('r'))
                    .and_then(|n| n.parse().ok())
            });
            Target::Request {
                iid,
                note,
                line: None,
            }
        }
        "commit" => {
            let sha = parts.next()?;
            (sha.len() >= 7 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| Target::Commit(sha.to_owned()))?
        }
        "tree" | "commits" => {
            let name: Vec<&str> = parts.collect();
            (!name.is_empty()).then(|| Target::Branch(name.join("/")))?
        }
        _ => return None,
    };
    Some(Link {
        base: format!("{scheme}://{host}/{owner}/{repo}"),
        target,
        github: true,
    })
}

/// `<40 hex>_<old>_<new>`, either number possibly empty.
fn diff_line(fragment: &str) -> Option<DiffLine> {
    let mut parts = fragment.split('_');
    let file = parts.next()?;
    if file.len() != 40 || !file.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let number = |p: Option<&str>| p.and_then(|p| p.parse().ok());
    Some(DiffLine {
        file: file.to_ascii_lowercase(),
        old: number(parts.next()),
        new: number(parts.next()),
    })
}

/// The SHA-1 GitLab names a diff's file by.
pub fn path_hash(path: &str) -> String {
    sha1_smol::Sha1::from(path).digest().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_links() {
        let link = parse("https://github.com/acme/widgets/pull/12/files#discussion_r501").unwrap();
        assert_eq!(link.base, "https://github.com/acme/widgets");
        assert!(link.github);
        assert_eq!(
            link.target,
            Target::Request {
                iid: 12,
                note: Some(501),
                line: None
            }
        );
        assert_eq!(link.describe(), "#12, a comment");
        let c = parse("https://github.com/acme/widgets/pull/12#issuecomment-9").unwrap();
        assert!(matches!(c.target, Target::Request { note: Some(9), .. }));
        assert_eq!(
            parse("https://github.com/acme/widgets/commit/1a2b3c4d")
                .unwrap()
                .target,
            Target::Commit("1a2b3c4d".into())
        );
        assert_eq!(
            parse("https://github.com/acme/widgets/tree/feat/x")
                .unwrap()
                .target,
            Target::Branch("feat/x".into())
        );
        assert!(parse("https://github.com/acme/widgets/issues/3").is_none());
        assert!(parse("https://github.com/acme").is_none());
    }

    #[test]
    fn a_merge_request_link_with_a_comment() {
        let link = parse("https://git.x.io/group/sub/app/-/merge_requests/412#note_9981").unwrap();
        assert_eq!(link.base, "https://git.x.io/group/sub/app");
        assert_eq!(link.project(), "group/sub/app");
        assert_eq!(
            link.target,
            Target::Request {
                iid: 412,
                note: Some(9981),
                line: None
            }
        );
        assert_eq!(link.describe(), "!412, a comment");
    }

    #[test]
    fn a_line_of_a_merge_requests_diff() {
        let hash = path_hash("src/app.rs");
        let link = parse(&format!(
            "https://git.x.io/g/app/-/merge_requests/7/diffs?commit_id=1#{hash}_10_12"
        ))
        .unwrap();
        assert_eq!(
            link.target,
            Target::Request {
                iid: 7,
                note: None,
                line: Some(DiffLine {
                    file: hash,
                    old: Some(10),
                    new: Some(12)
                })
            }
        );
    }

    #[test]
    fn commits_and_branches() {
        assert_eq!(
            parse("https://git.x.io/g/app/-/commit/1a2b3c4d5e")
                .unwrap()
                .target,
            Target::Commit("1a2b3c4d5e".into())
        );
        assert_eq!(
            parse("https://git.x.io/g/app/-/tree/feat/login")
                .unwrap()
                .target,
            Target::Branch("feat/login".into())
        );
        assert_eq!(parse("https://git.x.io/g/app/-/issues/3"), None);
        assert_eq!(parse("not a link"), None);
    }

    #[test]
    fn the_path_hash_is_plain_sha1() {
        assert_eq!(path_hash("a"), "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8");
    }
}
