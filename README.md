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
- Five themes (One Dark, Graphite, Darcula, Night, Paper) and your own as JSON, with an editor that checks contrast; the code can follow the window or use another theme.
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
- Line comments for an agent: written in the diff, kept on this Mac, copied as markdown (built). Later: `.misc/review.md` or an MCP server so an agent can read and resolve them itself.
- A command-line launcher (`gitlance [path]`), if opening from a terminal turns out to be wanted.

Staging and committing stay out of GitLance.

## Stack

Rust end to end.

- **UI:** [GPUI](https://www.gpui.rs) — Zed's GPU-accelerated framework, Metal on macOS; pre-1.0, so it is
  pinned to the `gpui-pre` snapshot. [`gpui-component`](https://github.com/longbridge/gpui-component) joins
  when a widget (resizable panes, selects) earns it.
- **Git:** `git2` (libgit2, vendored), read-only; the rebase-aware version diff merges in memory.
- **Highlighting:** `tree-sitter` for Kotlin, Java, Rust, Swift, TypeScript, JavaScript, Go, Python, JSON, YAML and Markdown — a declared function from a call, a parameter from a local, fields, constants — and `syntect` with bat's syntaxes (`two-face`) for the rest, in the theme's code colours.
- **Structural diff:** difftastic's JSON output (`difft --display json`), optional.

## Why not an existing app

Fork, Sublime Merge and Zed's git panel show diffs, but none compares the versions of a force-pushed
branch, tracks what was reviewed, or sends comments back to an agent.

## Merge and pull requests

With a token, the requests of the repository's GitLab project or GitHub repository open in the app:
the list, the discussions on the lines, replying, resolving, pending comments, Submit review (comment,
approve, request changes), the pipeline or checks, versions, the bell. GitLab: a token with the `api`
scope (Settings → Merge requests, or `~/.config/gitlab-token`). GitHub (github.com or Enterprise): a
classic token with the `repo` scope, `GITHUB_TOKEN`, or the GitHub CLI's (`gh auth login`).

## Agents

Comments for your coding agent (purple) are drafts until you send them: *Send to agent* (⇧⌘A) lists the
repository's agent chats — those listening first — and a new chat. The agent changes the code, answers
each comment, and you resolve or reopen it in GitLance. Connect Claude Code once in Settings → Agents
(it adds GitLance as an MCP server to every Claude Code profile); a chat then listens by itself. Codex
gets the same tools (`codex mcp add gitlance -- <GitLance> mcp`); its chats are not listed yet.

## Building

Needs a Rust toolchain (`rustup`, stable) and Xcode's command-line tools; full Xcode is not required.

```sh
./run.sh               # release build; opens the repository it is run from, else the last one
./run.sh <path>        # opens <path>
./run.sh --debug       # a debug build, faster to compile
```

Drag the edges between panes to resize them; a double click restores the default size. Sizes and recent
repositories are kept in `~/Library/Application Support/GitLance/` (Windows `%APPDATA%\GitLance`, Linux
`~/.local/share/gitlance`).

`./bundle.sh` wraps the release build in `target/GitLance.app`.

On Linux the build needs the X11, Wayland, Vulkan and font headers: `.github/linux-deps.sh` installs
them on Debian and Ubuntu.

## Releases

CI (`.github/workflows/ci.yml`) runs fmt, clippy and the tests on macOS, Windows and Linux for every
push. A `release/v*` tag whose version is `Cargo.toml`'s runs `release.yml`, which attaches these to a GitHub
release:

| | |
|---|---|
| macOS | `GitLance-<v>-macos.dmg` and `.zip` — a universal `GitLance.app` (Apple silicon and Intel) |
| Windows | `GitLance-<v>-windows-x64.zip` — `GitLance.exe` |
| Linux | `GitLance-<v>-x86_64.AppImage`, and `gitlance-<v>-linux-x86_64.tar.gz` with a desktop file |

Run by hand from the Actions tab, `release.yml` only builds; the files stay on the run.

The builds are not signed or notarized. On macOS the first open is refused: right-click the app →
Open, or System Settings → Privacy & Security → Open Anyway (or `xattr -dr com.apple.quarantine
/Applications/GitLance.app`). On Windows, SmartScreen asks: More info → Run anyway.
