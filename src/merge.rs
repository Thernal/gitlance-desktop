//! Merging a request from GitLance (#164 B): what stands in the way — the host's word with what
//! the page knows already (approvals, discussions, the pipeline) —, the ways the host allows with
//! the project's defaults, and what a merge sends: the head that was reviewed, so that a push
//! nobody has seen is refused rather than merged. The calls are `mr::merge_state` / `mr::merge`
//! (GitLab) and their GitHub twins.

use crate::mr::{Approvals, Pipeline};

/// What the host says of merging the request, read with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Host {
    pub github: bool,
    /// GitLab's `detailed_merge_status`, GitHub's `mergeable_state`.
    pub status: String,
    /// The token's user may merge into the target.
    pub can: bool,
    pub conflicts: bool,
    /// Commits the target has that the branch has not (GitLab's `diverged_commits_count`).
    pub behind: u32,
    /// The project wants the pipeline to pass first, and every discussion resolved.
    pub pipeline_must_pass: bool,
    pub discussions_must_resolve: bool,
    /// Set to merge when the pipeline succeeds already.
    pub auto_set: bool,
    /// Merging when the pipeline succeeds can be set: GitLab always, GitHub where the
    /// repository has auto-merge on.
    pub auto_allowed: bool,
    pub squash: Squash,
    /// Delete the source branch after, as the request or the project has it.
    pub delete_branch: bool,
    /// GitHub deletes the branch itself after a merge (`delete_branch_on_merge`).
    pub deletes_itself: bool,
    /// GitHub: the ways the repository allows, its default first.
    pub methods: Vec<Method>,
    /// GitHub's node id of the pull request, for auto-merge's mutations.
    pub node: String,
}

/// Squashing the request's commits into one, as the project allows it (GitLab's `squash_option`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Squash {
    #[default]
    Never,
    Always,
    /// The merger chooses; the project's default.
    Choose(bool),
}

/// How GitHub puts a pull request into its base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Merge,
    Squash,
    Rebase,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::Merge => "Merge commit",
            Method::Squash => "Squash",
            Method::Rebase => "Rebase",
        }
    }

    /// The REST API's `merge_method`.
    pub fn key(self) -> &'static str {
        match self {
            Method::Merge => "merge",
            Method::Squash => "squash",
            Method::Rebase => "rebase",
        }
    }
}

/// Where a reason it cannot merge is fixed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fix {
    /// Nothing to go to: someone has to act (another approval).
    Nowhere,
    /// The request's discussions, on its Overview.
    Discussions,
    /// A page on the host: the pipeline, the request itself.
    Page(String),
}

/// One thing in the way: what, the words, where it is fixed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub what: &'static str,
    pub detail: String,
    pub fix: Fix,
}

/// What the page knows besides the host's word: fresher than it after an approval or a
/// discussion resolved here.
pub struct Known<'a> {
    pub draft: bool,
    pub approvals: Option<&'a Approvals>,
    /// Discussions that can be resolved and are not.
    pub open_threads: usize,
    pub pipeline: Option<&'a Pipeline>,
    /// The request's page on the host.
    pub url: &'a str,
    pub target: &'a str,
}

/// Where merging stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stand {
    /// Set to merge when the pipeline succeeds.
    Auto,
    InTheWay(Vec<Block>),
    /// The host is still working it out.
    Checking,
    /// Behind its target by this many commits, and the host can rebase it (GitLab).
    Rebase(u32),
    /// The pipeline runs: merging can be set for when it succeeds.
    Running,
    Ready,
}

/// GitLab's statuses that `stand` says from what the page knows, or that are no reason at all.
const GITLAB_KNOWN: [&str; 14] = [
    "mergeable",
    "not_approved",
    "discussions_not_resolved",
    "ci_must_pass",
    "ci_still_running",
    "draft_status",
    "conflict",
    "need_rebase",
    "checking",
    "unchecked",
    "preparing",
    "approvals_syncing",
    "requested_changes",
    "not_open",
];

/// The words for a GitLab status `stand` does not say otherwise.
fn gitlab_words(status: &str) -> (&'static str, String) {
    let words = |what, detail: &str| (what, detail.to_owned());
    match status {
        "blocked_status" | "merge_request_blocked" => words("Blocked", "by another merge request"),
        "jira_association_missing" => words("Jira", "the title or description needs its issue"),
        "external_status_checks" | "status_checks_must_pass" => {
            words("Status checks", "the external checks must pass")
        }
        "commits_status" => words("Commits", "a commit breaks the project's push rules"),
        "merge_time" => words("Merge time", "not before the time set for it"),
        "security_policy_violations" => words("Security", "a security policy is not met"),
        "locked_paths" | "locked_lfs_files" => words("Locked", "a path it changes is locked"),
        "title_regex" => words("Title", "it does not match the project's rule"),
        other => ("GitLab", other.replace('_', " ")),
    }
}

/// Whether the pipeline is still on its way.
fn running(p: Option<&Pipeline>) -> bool {
    p.is_some_and(|p| {
        matches!(
            p.status.as_str(),
            "running" | "pending" | "created" | "waiting_for_resource" | "preparing" | "scheduled"
        )
    })
}

/// Where merging stands: every reason in the way, or what can be done.
pub fn stand(host: &Host, known: &Known) -> Stand {
    if host.auto_set {
        return Stand::Auto;
    }
    let status = host.status.as_str();
    let page = || Fix::Page(known.url.to_owned());
    let mut blocks = Vec::new();
    let mut block = |what, detail: String, fix| blocks.push(Block { what, detail, fix });
    if !host.can {
        block(
            "Permission",
            format!("you may not merge into {}", known.target),
            Fix::Nowhere,
        );
    }
    if known.draft || status == "draft_status" || (host.github && status == "draft") {
        block("Draft", "marked as a draft: ready it first".into(), page());
    }
    if let Some(a) = known.approvals.filter(|a| a.left > 0) {
        let have = a.required.saturating_sub(a.left);
        let more = match a.left {
            1 => "one more is needed".to_owned(),
            n => format!("{n} more are needed"),
        };
        block(
            "Approvals",
            format!("{have} of {}: {more}", a.required),
            Fix::Nowhere,
        );
    }
    if status == "requested_changes" {
        block(
            "Changes requested",
            "a reviewer asked for changes".into(),
            Fix::Nowhere,
        );
    }
    if known.open_threads > 0
        && (host.discussions_must_resolve || status == "discussions_not_resolved")
    {
        block(
            "Discussions",
            format!("{} not resolved", known.open_threads),
            Fix::Discussions,
        );
    }
    if let Some(p) = known.pipeline
        && matches!(p.status.as_str(), "failed" | "canceled" | "skipped")
        && (host.pipeline_must_pass || status == "ci_must_pass")
    {
        let what = match (p.status.as_str(), p.failed.first()) {
            ("failed", Some((job, _))) => format!("{job} failed"),
            (word, _) => word.to_owned(),
        };
        block(
            "Pipeline",
            format!("{what}, and it must pass"),
            Fix::Page(p.url.clone()),
        );
    }
    if host.conflicts || status == "conflict" || (host.github && status == "dirty") {
        block(
            "Conflicts",
            format!("with {}: resolve them on the host", known.target),
            page(),
        );
    }
    if host.github && status == "behind" {
        block(
            "Behind",
            format!("{} moved on: update the branch", known.target),
            page(),
        );
    }
    if !host.github && !status.is_empty() && !GITLAB_KNOWN.contains(&status) {
        let (what, detail) = gitlab_words(status);
        block(what, detail, page());
    }
    let running = running(known.pipeline);
    // GitHub says only "blocked": a check still running, or a rule of the branch.
    if host.github && status == "blocked" && blocks.is_empty() && !(running && host.auto_allowed) {
        let (what, detail) = match running {
            true => ("Checks", "still running, and they must pass".to_owned()),
            false => (
                "Branch rules",
                format!("{}'s rules on GitHub block it", known.target),
            ),
        };
        blocks.push(Block {
            what,
            detail,
            fix: page(),
        });
    }
    if !blocks.is_empty() {
        return Stand::InTheWay(blocks);
    }
    let checking = match host.github {
        true => status == "unknown",
        false => matches!(
            status,
            "checking" | "unchecked" | "preparing" | "approvals_syncing"
        ),
    };
    if checking {
        return Stand::Checking;
    }
    if status == "need_rebase" {
        return Stand::Rebase(host.behind.max(1));
    }
    let must_wait = host.pipeline_must_pass
        || status == "ci_still_running"
        || (host.github && status == "blocked");
    if running && must_wait && host.auto_allowed {
        return Stand::Running;
    }
    Stand::Ready
}

/// What a merge sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merge {
    /// The head that was reviewed: the host refuses when the branch moved on since.
    pub sha: String,
    pub squash: bool,
    pub delete_branch: bool,
    /// GitHub's way.
    pub method: Method,
    /// The commit's message: its first line is GitHub's title.
    pub message: String,
    /// Set to merge when the pipeline succeeds, rather than now.
    pub when_pipeline_succeeds: bool,
}

impl Merge {
    /// The card's start: the project's defaults, the title and the reference as the message.
    pub fn defaults(host: &Host, sha: &str, message: String, when_pipeline_succeeds: bool) -> Self {
        let method = host.methods.first().copied().unwrap_or(Method::Merge);
        Merge {
            sha: sha.to_owned(),
            squash: match host.squash {
                Squash::Never => false,
                Squash::Always => true,
                Squash::Choose(on) => on,
            },
            delete_branch: host.delete_branch,
            method,
            message,
            when_pipeline_succeeds,
        }
    }
}

/// The host refused because the branch moved on since the head that was sent.
pub fn moved_on(error: &str) -> bool {
    let moved = ["SHA does not match", "Head branch was modified", "head sha"];
    moved
        .iter()
        .any(|m| error.to_lowercase().contains(&m.to_lowercase()))
}

/// A refusal in the reviewer's words: a head that moved on since the review is the one to say
/// plainly.
pub fn refusal(error: &str) -> String {
    if moved_on(error) {
        return "A push came after the version you reviewed: it was not merged. Open the new version first."
            .into();
    }
    error.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known<'a>(approvals: Option<&'a Approvals>, pipeline: Option<&'a Pipeline>) -> Known<'a> {
        Known {
            draft: false,
            approvals,
            open_threads: 2,
            pipeline,
            url: "https://gitlab.example.com/p/-/merge_requests/7",
            target: "main",
        }
    }

    fn pipeline(status: &str) -> Pipeline {
        Pipeline {
            status: status.into(),
            url: "https://ci/1".into(),
            updated: 0,
            failed: vec![("lint".into(), "https://ci/job".into())],
        }
    }

    fn gitlab(status: &str) -> Host {
        Host {
            status: status.into(),
            can: true,
            auto_allowed: true,
            ..Host::default()
        }
    }

    #[test]
    fn every_reason_in_the_way_is_listed_with_where_it_is_fixed() {
        let approvals = Approvals {
            required: 2,
            left: 1,
            ..Approvals::default()
        };
        let failed = pipeline("failed");
        let host = Host {
            pipeline_must_pass: true,
            discussions_must_resolve: true,
            ..gitlab("not_approved")
        };
        let Stand::InTheWay(blocks) = stand(&host, &known(Some(&approvals), Some(&failed))) else {
            panic!("not in the way");
        };
        let said: Vec<(&str, &str)> = blocks.iter().map(|b| (b.what, b.detail.as_str())).collect();
        assert_eq!(
            said,
            [
                ("Approvals", "1 of 2: one more is needed"),
                ("Discussions", "2 not resolved"),
                ("Pipeline", "lint failed, and it must pass"),
            ]
        );
        assert_eq!(blocks[1].fix, Fix::Discussions);
        assert_eq!(blocks[2].fix, Fix::Page("https://ci/1".into()));
    }

    #[test]
    fn the_page_knows_better_than_a_status_read_before_an_approval() {
        // GitLab said not_approved; the approval given since leaves none missing.
        let approvals = Approvals::default();
        let host = gitlab("not_approved");
        assert_eq!(stand(&host, &known(Some(&approvals), None)), Stand::Ready);
    }

    #[test]
    fn a_running_pipeline_a_rebase_and_auto_merge_are_offered_as_such() {
        let running = pipeline("running");
        let must = Host {
            pipeline_must_pass: true,
            ..gitlab("ci_still_running")
        };
        assert_eq!(stand(&must, &known(None, Some(&running))), Stand::Running);
        let behind = Host {
            behind: 4,
            ..gitlab("need_rebase")
        };
        assert_eq!(stand(&behind, &known(None, None)), Stand::Rebase(4));
        let set = Host {
            auto_set: true,
            ..gitlab("ci_still_running")
        };
        assert_eq!(stand(&set, &known(None, None)), Stand::Auto);
        assert_eq!(
            stand(&gitlab("checking"), &known(None, None)),
            Stand::Checking
        );
        let Stand::InTheWay(other) = stand(&gitlab("merge_time"), &known(None, None)) else {
            panic!("a status of its own is in the way");
        };
        assert_eq!(other[0].what, "Merge time");
    }

    #[test]
    fn github_says_blocked_for_a_rule_or_a_check_on_its_way() {
        let github = |status: &str, auto| Host {
            github: true,
            auto_allowed: auto,
            ..gitlab(status)
        };
        let running = pipeline("pending");
        assert_eq!(
            stand(&github("blocked", true), &known(None, Some(&running))),
            Stand::Running
        );
        let Stand::InTheWay(rules) = stand(&github("blocked", false), &known(None, None)) else {
            panic!("a branch rule is in the way");
        };
        assert_eq!(rules[0].what, "Branch rules");
        let Stand::InTheWay(dirty) = stand(&github("dirty", false), &known(None, None)) else {
            panic!("conflicts are in the way");
        };
        assert_eq!(dirty[0].what, "Conflicts");
    }

    #[test]
    fn a_push_after_the_review_is_said_plainly() {
        assert!(
            refusal("GitLab refused it: SHA does not match HEAD of source branch")
                .starts_with("A push came after")
        );
        assert!(moved_on(
            "Head branch was modified. Review and try the merge again."
        ));
        assert_eq!(
            refusal("Branch cannot be merged"),
            "Branch cannot be merged"
        );
    }
}
