use super::*;
use git2::{Signature, Time};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    repo: Repository,
    clock: i64,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        Self {
            _dir: dir,
            repo,
            clock: 1_700_000_000,
        }
    }

    fn open(&self) -> Repo {
        Repo::open(self.repo.workdir().unwrap()).unwrap()
    }

    /// A commit of flat `files` on `parents`; the ref is moved to it with `reason` in the reflog.
    fn commit(
        &mut self,
        refname: &str,
        parents: &[Oid],
        files: &[(&str, &str)],
        reason: &str,
    ) -> Oid {
        let mut builder = self.repo.treebuilder(None).unwrap();
        for (name, content) in files {
            let blob = self.repo.blob(content.as_bytes()).unwrap();
            builder.insert(name, blob, 0o100644).unwrap();
        }
        let tree = self.repo.find_tree(builder.write().unwrap()).unwrap();
        self.clock += 60;
        let sig = Signature::new("Test", "test@example.com", &Time::new(self.clock, 0)).unwrap();
        let parents: Vec<_> = parents
            .iter()
            .map(|p| self.repo.find_commit(*p).unwrap())
            .collect();
        let parents: Vec<_> = parents.iter().collect();
        let id = self
            .repo
            .commit(None, &sig, &sig, reason, &tree, &parents)
            .unwrap();
        self.repo.reference(refname, id, true, reason).unwrap();
        id
    }

    fn loose_objects(&self) -> usize {
        let objects = self.repo.path().join("objects");
        std::fs::read_dir(objects)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().len() == 2)
            .map(|e| std::fs::read_dir(e.path()).unwrap().count())
            .sum()
    }
}

fn paths(files: &[FileDiff]) -> Vec<&str> {
    let mut paths: Vec<_> = files.iter().map(FileDiff::path).collect();
    paths.sort();
    paths
}

#[test]
fn log_lists_newest_first() {
    let mut fx = Fixture::new();
    let a = fx.commit("refs/heads/main", &[], &[("a.txt", "1\n")], "first");
    let b = fx.commit("refs/heads/main", &[a], &[("a.txt", "2\n")], "second");
    let log = fx.open().log(b, 10).unwrap();
    assert_eq!(log.iter().map(|c| c.id).collect::<Vec<_>>(), [b, a]);
    assert_eq!(log[0].summary, "second");
}

#[test]
fn commit_diff_reports_changes_and_lines() {
    let mut fx = Fixture::new();
    let content = "fn main() {\n    println!(\"hello\");\n}\n// a long enough tail\n// so rename detection\n// has something to match\n";
    let a = fx.commit(
        "refs/heads/main",
        &[],
        &[
            ("a.txt", "1\n2\n3\n"),
            ("gone.txt", "x\n"),
            ("old.rs", content),
        ],
        "first",
    );
    let b = fx.commit(
        "refs/heads/main",
        &[a],
        &[
            ("a.txt", "1\nTWO\n3\n"),
            ("new.txt", "n\n"),
            ("new.rs", content),
        ],
        "second",
    );
    let files = fx.open().commit_diff(b, DiffSettings::default()).unwrap();
    let kind = |p: &str| files.iter().find(|f| f.path() == p).unwrap().change;
    assert_eq!(kind("a.txt"), ChangeKind::Modified);
    assert_eq!(kind("gone.txt"), ChangeKind::Deleted);
    assert_eq!(kind("new.txt"), ChangeKind::Added);
    assert_eq!(kind("new.rs"), ChangeKind::Renamed);

    let a_txt = files.iter().find(|f| f.path() == "a.txt").unwrap();
    assert_eq!((a_txt.added, a_txt.removed), (1, 1));
    let changed: Vec<_> = a_txt.hunks[0]
        .lines
        .iter()
        .filter(|l| l.kind != LineKind::Context)
        .map(|l| (l.kind, l.old_line, l.new_line))
        .collect();
    assert_eq!(
        changed,
        [
            (LineKind::Removed, Some(2), None),
            (LineKind::Added, None, Some(2))
        ]
    );
    assert_eq!(a_txt.new_text.as_deref(), Some("1\nTWO\n3\n"));
}

#[test]
fn fast_forwards_stay_in_one_version_and_amends_start_new_ones() {
    let mut fx = Fixture::new();
    let base = fx.commit("refs/heads/main", &[], &[("a.txt", "1\n")], "base");
    let feature = "refs/heads/feature";
    let v1 = fx.commit(feature, &[base], &[("a.txt", "2\n")], "commit: change");
    let v2 = fx.commit(
        feature,
        &[base],
        &[("a.txt", "3\n")],
        "commit (amend): change",
    );
    let v2b = fx.commit(feature, &[v2], &[("a.txt", "4\n")], "commit: more");
    let v3 = fx.commit(
        feature,
        &[base],
        &[("a.txt", "5\n")],
        "commit (amend): squash",
    );

    let versions = fx.open().versions(feature).unwrap();
    // The ref's creation at `v1` and its later moves.
    let tips: Vec<_> = versions.iter().map(|v| v.tip).collect();
    assert_eq!(tips, [v1, v2b, v3]);
    assert_eq!(
        versions.iter().map(|v| v.number).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(versions.iter().all(|v| v.base == Some(base)));
    assert_eq!(versions[1].commits, 2);
    assert_eq!(versions[2].reason, "commit (amend): squash");
}

#[test]
fn version_diff_with_the_same_base_is_a_tree_diff() {
    let mut fx = Fixture::new();
    let base = fx.commit("refs/heads/main", &[], &[("a.txt", "1\n")], "base");
    let feature = "refs/heads/feature";
    fx.commit(feature, &[base], &[("a.txt", "2\n")], "commit");
    fx.commit(
        feature,
        &[base],
        &[("a.txt", "3\n"), ("b.txt", "b\n")],
        "amend",
    );

    let repo = fx.open();
    let versions = repo.versions(feature).unwrap();
    let diff = repo
        .version_diff(&versions[0], &versions[1], DiffSettings::default())
        .unwrap();
    assert!(!diff.rebased);
    assert_eq!(paths(&diff.files), ["a.txt", "b.txt"]);
    let a = diff.files.iter().find(|f| f.path() == "a.txt").unwrap();
    assert_eq!(a.old_text.as_deref(), Some("2\n"));
    assert_eq!(a.new_text.as_deref(), Some("3\n"));
}

#[test]
fn version_diff_after_a_rebase_hides_upstream_changes() {
    let mut fx = Fixture::new();
    let files = [("a.txt", "1\n2\n3\n"), ("up.txt", "old\n")];
    let m1 = fx.commit("refs/heads/main", &[], &files, "m1");
    let feature = "refs/heads/feature";
    fx.commit(
        feature,
        &[m1],
        &[("a.txt", "1\ntwo\n3\n"), ("up.txt", "old\n")],
        "commit",
    );
    let m2 = fx.commit(
        "refs/heads/main",
        &[m1],
        &[("a.txt", "1\n2\n3\n"), ("up.txt", "new\n")],
        "m2",
    );
    fx.commit(
        feature,
        &[m2],
        &[
            ("a.txt", "1\nTWO\n3\n"),
            ("up.txt", "new\n"),
            ("c.txt", "c\n"),
        ],
        "rebase (finish): refs/heads/feature onto m2",
    );

    let repo = fx.open();
    let versions = repo.versions(feature).unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!((versions[0].base, versions[1].base), (Some(m1), Some(m2)));

    let before = fx.loose_objects();
    let diff = repo
        .version_diff(&versions[0], &versions[1], DiffSettings::default())
        .unwrap();
    assert_eq!(
        fx.loose_objects(),
        before,
        "the reviewed repository gained objects"
    );

    assert!(diff.rebased);
    assert!(diff.conflicts.is_empty());
    assert_eq!(paths(&diff.files), ["a.txt", "c.txt"]);
    let a = diff.files.iter().find(|f| f.path() == "a.txt").unwrap();
    assert_eq!(a.old_text.as_deref(), Some("1\ntwo\n3\n"));
    assert_eq!(a.new_text.as_deref(), Some("1\nTWO\n3\n"));
}

#[test]
fn a_conflicting_rebase_falls_back_per_file() {
    let mut fx = Fixture::new();
    let m1 = fx.commit("refs/heads/main", &[], &[("a.txt", "1\n2\n3\n")], "m1");
    let feature = "refs/heads/feature";
    fx.commit(feature, &[m1], &[("a.txt", "1\nmine\n3\n")], "commit");
    let m2 = fx.commit(
        "refs/heads/main",
        &[m1],
        &[("a.txt", "1\ntheirs\n3\n")],
        "m2",
    );
    fx.commit(feature, &[m2], &[("a.txt", "1\nresolved\n3\n")], "rebase");

    let repo = fx.open();
    let versions = repo.versions(feature).unwrap();
    let diff = repo
        .version_diff(&versions[0], &versions[1], DiffSettings::default())
        .unwrap();
    assert_eq!(diff.conflicts, ["a.txt"]);
    let a = &diff.files[0];
    assert_eq!(a.old_text.as_deref(), Some("1\nmine\n3\n"));
    assert!(a.note.as_deref().unwrap().starts_with("Rebase conflict"));
}

#[test]
fn whitespace_only_changes_can_be_ignored() {
    let mut fx = Fixture::new();
    let a = fx.commit(
        "refs/heads/main",
        &[],
        &[("a.txt", "if x {\n  y\n}\n")],
        "first",
    );
    let b = fx.commit(
        "refs/heads/main",
        &[a],
        &[("a.txt", "if x {\n    y\n}\n")],
        "indent",
    );
    let repo = fx.open();
    let shown = repo.commit_diff(b, DiffSettings::default()).unwrap();
    assert_eq!((shown[0].added, shown[0].removed), (1, 1));
    let hidden = DiffSettings {
        ignore_whitespace: true,
    };
    let quiet = repo.commit_diff(b, hidden).unwrap();
    assert!(quiet.iter().all(|f| f.added + f.removed == 0));
}

#[test]
fn changed_paths_lists_what_each_commit_touched() {
    let mut fx = Fixture::new();
    let a = fx.commit("refs/heads/main", &[], &[("a.txt", "1\n")], "a");
    let b = fx.commit(
        "refs/heads/main",
        &[a],
        &[("a.txt", "1\n"), ("b.txt", "2\n")],
        "b",
    );
    let found = fx.open().changed_paths(&[a, b]);
    assert_eq!(found[0], (a, vec!["a.txt".to_owned(), "a.txt".to_owned()]));
    assert_eq!(found[1].1, ["b.txt", "b.txt"]);
}

#[test]
fn the_fingerprint_changes_with_refs_and_not_without_them() {
    let mut fx = Fixture::new();
    let a = fx.commit("refs/heads/main", &[], &[("a.txt", "1\n")], "a");
    let dir = fx.open().git_dir();
    let before = fingerprint(&dir);
    assert_eq!(fingerprint(&dir), before, "reading changes nothing");
    fx.commit("refs/heads/main", &[a], &[("a.txt", "2\n")], "amend");
    assert_ne!(fingerprint(&dir), before);
}

#[test]
fn clone_urls_become_web_addresses() {
    let gitlab = WebRemote::parse("git@gitlab.digigo.example:mobile/payments-service.git").unwrap();
    assert_eq!(
        gitlab.base,
        "https://gitlab.digigo.example/mobile/payments-service"
    );
    assert_eq!(
        gitlab.commit("abc"),
        "https://gitlab.digigo.example/mobile/payments-service/-/commit/abc"
    );
    let ssh = WebRemote::parse("ssh://git@host.example:2222/group/sub/proj.git").unwrap();
    assert_eq!(ssh.base, "https://host.example/group/sub/proj");
    let https =
        WebRemote::parse("https://user:token@github.com/Thernal/gitlance-desktop.git").unwrap();
    assert!(https.github);
    assert_eq!(
        https.commit("abc"),
        "https://github.com/Thernal/gitlance-desktop/commit/abc"
    );
    assert_eq!(
        https.branch("origin/feature/x"),
        "https://github.com/Thernal/gitlance-desktop/tree/feature/x"
    );
    assert_eq!(
        gitlab.branch("main"),
        "https://gitlab.digigo.example/mobile/payments-service/-/tree/main"
    );
    assert!(WebRemote::parse("/local/path/repo").is_none());
    assert!(WebRemote::parse("").is_none());
}

#[test]
fn decorations_label_branches_remote_branches_and_tags() {
    let mut fx = Fixture::new();
    let a = fx.commit("refs/heads/main", &[], &[("a.txt", "1\n")], "a");
    let b = fx.commit("refs/heads/main", &[a], &[("a.txt", "2\n")], "b");
    fx.repo.reference("refs/tags/v1", a, true, "tag").unwrap();
    fx.repo
        .reference("refs/remotes/origin/main", b, true, "fetch")
        .unwrap();
    fx.repo.set_head("refs/heads/main").unwrap();
    let decos = fx.open().decorations();
    let on_b: Vec<_> = decos[&b]
        .iter()
        .map(|d| (d.name.as_str(), d.kind))
        .collect();
    assert_eq!(
        on_b,
        [("main", DecoKind::Head), ("origin/main", DecoKind::Remote)]
    );
    assert_eq!(decos[&a][0].name, "v1");
    assert_eq!(decos[&a][0].kind, DecoKind::Tag);
}

#[test]
fn range_pairs_tell_unchanged_modified_new_and_dropped_commits() {
    use super::PairKind::*;
    let mut fx = Fixture::new();
    let base = fx.commit("refs/heads/main", &[], &[("base.txt", "0\n")], "base");
    let feature = "refs/heads/feature";
    let c1 = fx.commit(
        feature,
        &[base],
        &[("base.txt", "0\n"), ("a.txt", "1\n")],
        "commit: feat a",
    );
    let c2 = fx.commit(
        feature,
        &[c1],
        &[("base.txt", "0\n"), ("a.txt", "1\n"), ("b.txt", "1\n")],
        "commit: feat b",
    );
    fx.commit(
        feature,
        &[c2],
        &[
            ("base.txt", "0\n"),
            ("a.txt", "1\n"),
            ("b.txt", "1\n"),
            ("debug.txt", "x\n"),
        ],
        "commit: debug",
    );
    // The branch rewritten: a is edited, b is the same patch, debug is gone, a test is new.
    let d1 = fx.commit(
        feature,
        &[base],
        &[("base.txt", "0\n"), ("a.txt", "2\n")],
        "commit (amend): feat a",
    );
    let d2 = fx.commit(
        feature,
        &[d1],
        &[("base.txt", "0\n"), ("a.txt", "2\n"), ("b.txt", "1\n")],
        "commit: feat b",
    );
    fx.commit(
        feature,
        &[d2],
        &[
            ("base.txt", "0\n"),
            ("a.txt", "2\n"),
            ("b.txt", "1\n"),
            ("t.txt", "t\n"),
        ],
        "commit: test",
    );

    let repo = fx.open();
    let versions = repo.versions(feature).unwrap();
    assert_eq!(versions.len(), 2);
    let pairs = repo.range_pairs(&versions[0], &versions[1]).unwrap();
    let kinds: Vec<_> = pairs.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [Modified, Unchanged, Dropped, New]);
    assert_eq!(pairs[2].old.as_ref().unwrap().summary, "commit: debug");

    let (old, new) = (
        pairs[0].old.as_ref().unwrap(),
        pairs[0].new.as_ref().unwrap(),
    );
    let inter = repo
        .interdiff(old.id, new.id, DiffSettings::default())
        .unwrap();
    assert_eq!(paths(&inter.files), ["a.txt"]);
}

#[test]
fn compare_since_the_merge_base_shows_only_what_the_head_added() {
    let mut fx = Fixture::new();
    let root = fx.commit("refs/heads/main", &[], &[("a.txt", "1\n")], "root");
    let feature = fx.commit(
        "refs/heads/feature",
        &[root],
        &[("a.txt", "1\n"), ("f.txt", "f\n")],
        "commit: feature",
    );
    // main moved on after the fork.
    let main = fx.commit(
        "refs/heads/main",
        &[root],
        &[("a.txt", "1\n"), ("m.txt", "m\n")],
        "commit: main work",
    );
    let repo = fx.open();
    let since = repo
        .compare(main, feature, true, DiffSettings::default())
        .unwrap();
    assert_eq!(paths(&since.files), ["f.txt"]);
    assert_eq!((since.start, since.commits), (root, 1));
    let direct = repo
        .compare(main, feature, false, DiffSettings::default())
        .unwrap();
    assert_eq!(paths(&direct.files), ["f.txt", "m.txt"]);
    assert_eq!(repo.resolve("feature").unwrap(), feature);
    assert!(repo.resolve("nonexistent").is_err());
}
