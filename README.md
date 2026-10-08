# GitLance

A standalone macOS app for reviewing changes — commits and branches — without opening an IDE and without
creating git worktrees.

Browse a repository's history comfortably, and when a branch was amended, rebased or force-pushed, see
what changed between its versions — the way GitLab compares merge-request versions, but locally.

## Status

Early: the first milestone below is being built.

## MVP

- Open a repository from the app (folder picker, recent repositories).
- Branch list and commit list; a selected commit's side-by-side diff.
- **Versions of a rewritten branch.** When a branch — typically a single commit — was amended, rebased
  or force-pushed, list its earlier versions and diff any two of them:
  - an unchanged base: a plain tree diff between the two versions;
  - a moved base (rebase): the two patches are compared, so upstream changes do not show up as noise.
- Code styled in One Dark.
- Diff view options, in the toolbar and the View menu, remembered between runs:
  - **Split / Unified** (⌥U) — side by side, or one column with removals above additions.
  - **Line / Structural** (⌥D) — git's line diff, or [difftastic](https://difftastic.wilfred.me.uk)'s
    syntax-aware one: reformatting is not a change and changed tokens are marked. Needs `difft`
    (`brew install difftastic`); without it the line diff is shown and the header says why.
  - **Wrap** (⌥Z) — long lines wrap; off, they scroll horizontally (trackpad or shift + wheel).
  - **All lines** (⌥E) — every unchanged line; off, three around each change, and a click on
    "N unchanged lines" opens one run.
  - **Hide whitespace** (⌥W) — whitespace-only changes count as unchanged (`git diff -w`). Off by
    default: whitespace matters in Python, YAML or Makefiles.

Earlier versions come from the reflogs of the branch and of its remote-tracking ref, so GitLance sees the
versions this machine has seen — its own amends and rebases, and every fetch. A push made elsewhere and
overwritten before a fetch is not visible, and versions expire with the reflog (90 days by default).

## Later

- Uncommitted work against `HEAD`, merging staged, unstaged and untracked files and detecting renames.
- Per-file "reviewed" marks that reset when the file changes.
- Line comments written to `.misc/review.md` in the reviewed repository, for an agent to pick up.
- A command-line launcher (`gitlance [path]`), if opening from a terminal turns out to be wanted.

Staging and committing stay out of GitLance.

## Stack

Rust end to end.

- **UI:** [GPUI](https://www.gpui.rs) — Zed's GPU-accelerated framework, Metal on macOS; pre-1.0, so it is
  pinned to the `gpui-pre` snapshot. [`gpui-component`](https://github.com/longbridge/gpui-component) joins
  when a widget (resizable panes, selects) earns it.
- **Git:** `git2` (libgit2, vendored), read-only; the rebase-aware version diff merges in memory.
- **Highlighting:** `syntect` with bat's syntaxes (`two-face`), in One Dark.
- **Structural diff:** difftastic's JSON output (`difft --display json`), optional.

## Why not an existing app

Fork, Sublime Merge and Zed's git panel show diffs, but none compares the versions of a force-pushed
branch, tracks what was reviewed, or sends comments back to an agent.

## Building

Needs a Rust toolchain (`rustup`, stable) and Xcode's command-line tools; full Xcode is not required.

```sh
./run.sh               # release build; opens the repository it is run from, else the last one
./run.sh <path>        # opens <path>
./run.sh --debug       # a debug build, faster to compile
```

Drag the edges between panes to resize them; a double click restores the default size. Sizes and recent
repositories are kept in `~/Library/Application Support/GitLance/`.
