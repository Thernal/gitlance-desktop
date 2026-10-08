# GitLance

A standalone macOS app only for reviewing changes — uncommitted work and branches — without opening an
IDE and without creating git worktrees.

Review an agent's (or your own) changes in one place before they are committed: see what moved and what
really changed, mark what has been read, and leave line comments the agent picks up.

## Status

Not started: this repository holds the plan and an empty Rust binary. The MVP below is the first milestone.

## MVP

- One diff against `HEAD` that merges staged, unstaged and untracked files and detects renames.
- A file tree grouped by package.
- Side-by-side and structural diff (difftastic, `difft --display json`).
- Per-file "reviewed" marks that reset when the file changes.
- Line comments written to `.misc/review.md` in the reviewed repository, for the agent to pick up.

Staging hunks and committing stay out of the MVP.

## Usage (planned)

```sh
gitlance                 # the working tree against HEAD
gitlance main..HEAD      # a range
git gitlance             # the same, through a `git-gitlance` symlink on PATH
```

## Stack

Rust end to end.

- **UI:** [GPUI](https://www.gpui.rs) — Zed's GPU-accelerated framework, Metal on macOS; pre-1.0, its API
  moves with Zed — and [`gpui-component`](https://github.com/longbridge/gpui-component) for the code
  editor, virtualized lists and dock layouts.
- **Backend:** git through gitoxide / git2 or the `git` CLI; structural diffs through difftastic.

## Why not an existing app

Fork, Sublime Merge and Zed's git panel show diffs, but none merges renames and edits into one view,
tracks what was reviewed, or sends comments back to an agent.

## Building

Needs a Rust toolchain (`rustup`, stable) and Xcode's command-line tools.

```sh
cargo run
```
