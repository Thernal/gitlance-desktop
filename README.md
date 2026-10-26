# GitLance

A desktop app for reviewing code — commits, branches, merge and pull requests, and what your coding agent
changed — without opening an IDE and without creating git worktrees. macOS first; Windows and Linux are built
too.

Browse a repository's history comfortably, review a merge request with its discussions, and when a branch was
amended, rebased or force-pushed, see what changed between its versions — the way GitLab compares
merge-request versions, but locally. Comments for your coding agent go to its chat and come back answered.

## Status

1.0.1 — downloads under [Releases](#releases), what is new in each in [CHANGELOG.md](CHANGELOG.md).

## The window

Four spaces in the menu on the left (⌃⌘1…4), each keeping the page it was left on:

- **Inbox** (⌘⇧N) — GitLab to-dos or GitHub notifications, new replies in discussions you wrote in or on your
  own requests, your discussions resolved by someone else, and what your agents answered; All · Mentions ·
  Agents · Reviews. The selected one shows the code its discussion is on and the whole discussion: reply
  (r), resolve, mark done (e).
- **Code** — the project's tree, read at the working tree, `HEAD`, a branch or a commit. Files and folders
  open as tabs; a file is read only, highlighted, a Markdown file rendered. A note for your agent goes on a
  line, a run of lines, a file or a folder.
- **Branches** — the working tree's changes first, then local branches with ahead / behind against the main
  line and their request; remotes folded. A branch off the main line opens as Commits · Changes since it
  forked.
- **Merge requests** — grouped by what each needs from you: Needs your review, Updated since your review,
  New, Reviewed, Yours. Merged and closed ones are under Open ▾. A request opens as Overview · Commits ·
  Changes, ⌥↓ goes to the next one waiting for you.

Several repositories open at once — as a rail of projects, tabs in the title bar, or projects in the menu
(⌘T, ⌘1…9, ⇧⌘W); the session comes back on the next start. A project can have a name and a colour of its
own (right click → Name and colour…). ⌘K searches everywhere: files ⌘P, commits ⌘⇧C, branches ⌘⇧B, requests
⌘⇧R, actions ⌘⇧P; a pasted GitLab or GitHub link opens in the app. ⌘[ ⌘] walk back and forward through every page
seen, esc goes one page up.

A folder that is not a git repository opens too, in Code. With no repository open, the welcome window opens
one, connects GitLab or GitHub, or clones from a URL.

## Reviewing a diff

- Every file of a diff in one scroll, or one file at a time (⌥A). **v** marks a file reviewed and moves to
  the next unread one; marks are kept per commit, range or request and reset when the file changes.
- View options, in the toolbar and the View menu, remembered between runs:
  - **Split / Unified** (⌥U) — side by side, or one column with removals above additions.
  - **Lines / Words / Structural** (⌥D) — git's line diff, the changed words marked, or
    [difftastic](https://difftastic.wilfred.me.uk)'s syntax-aware one: reformatting is not a change and
    changed tokens are marked. Structural needs `difft` (`brew install difftastic`); without it the line
    diff is shown and the header says why.
  - **Wrap** (⌥Z) — long lines wrap; off, they scroll horizontally (trackpad or shift + wheel).
  - **All lines** (⌥E) — every unchanged line; off, three around each change, and a click on
    "N unchanged lines" opens one run.
  - **Hide whitespace** (⌥W) — whitespace-only changes count as unchanged (`git diff -w`). Off by
    default: whitespace matters in Python, YAML or Makefiles.
- The working tree against `HEAD` — staged, unstaged and untracked files together, renames detected —
  refreshed when the file system says something changed.
- Any two revisions compared (⌥⌘C).
- Images as 2-up, Swipe or Onion skin; an SVG as image or source. A changed Markdown file rendered, its
  changed blocks marked (m switches Rendered | Source).
- **Zen** (⌘.): the diff alone. ⌥⌘. shows one file at a time at a reading width, with how far the review is.
- ⌘F finds in every file, ⌥⌘F in the whole diff's lines, both with regular expressions; ⌥⌘B shows who wrote
  each line (a line number's hover says it too); ⌘⇧O lists the declarations; F3 bookmarks a line; lock and
  generated files fold.

## Versions of a rewritten branch

When a branch — typically a single commit — was amended, rebased or force-pushed, GitLance lists its earlier
versions and diffs any two of them:

- an unchanged base: a plain tree diff between the two versions;
- a moved base (rebase): the two patches are compared, so upstream changes do not show up as noise.

Earlier versions come from the reflogs of the branch and of its remote-tracking ref, so GitLance sees the
versions this machine has seen — its own amends and rebases, and every fetch. A push made elsewhere and
overwritten before a fetch is not visible, and versions expire with the reflog (90 days by default). The
background fetch (below) is what brings other people's pushes in.

## Merge and pull requests

With a token, the requests of the repository's GitLab project or GitHub repository open in the app:

- discussions on the lines — replies, reactions, editing and deleting your own notes, resolving (⇧N ⇧P walk
  them, r replies, x resolves, e edits);
- pending comments and Submit review (⌘⇧↵): Comment, Approve (a), Request changes (GitLab 17+);
- the pipeline or checks, approvals and reviewers, labels and milestone;
- versions — what changed since the version you viewed, and a comment written on an earlier version drawn at
  its line now, with that line's history version by version;
- creating one (⌥⌘M): the target's template, reviewers from CODEOWNERS, labels, milestone, draft, squash.

While writing, `@` lists people and `:` lists emoji. Comments on a merge request (orange) are public;
comments for your agent (purple) stay on your Mac.

Tokens: GitLab needs one with the `api` scope; GitHub (github.com or Enterprise) a classic token with the
`repo` scope, `GITHUB_TOKEN` / `GH_TOKEN`, or the GitHub CLI's (`gh auth login`). Give it in the welcome
window's Connect GitLab or GitHub or in Settings → Merge requests; GitLab also reads `GITLAB_TOKEN` and
`~/.config/gitlab-token`.

## Agents

Comments for your coding agent (purple) are written on a diff line, a line in Code, a file, a folder, or
under a merge request's discussion (⇥ in a reply). They are drafts until you send them — Send all (⇧⌘↵),
Send this only (⌥⌘↵), or ⌥⌘A. A note goes to the chat listening on its page's branch, else to the project's
default chat; several chats can work at once, each in its own worktree, and a file another chat is changing
says so over its diff. The agent changes the code and answers each comment — or asks back with a form (pick
one, confirm, a plan, versions of code…), answered with a click or a key — and you resolve or reopen it in
GitLance. The Agents window (⇧⌘A) lists the chats and the notes; answers show in the Inbox too. A chat can be
given a name of its own.

- **Claude Code**: Settings → Agents → Connect adds GitLance's MCP server to every Claude Code profile. A chat
  then runs `GitLance agent-watch` in the background and is woken when comments are sent to it.
- **Codex** (limited): `codex mcp add gitlance -- <GitLance> mcp` gives it the same tools; a Codex running in
  a terminal in the project then listens by itself — the app finds its session and hands comments over with
  `codex queue`. Not seen: Codex in VS Code or the Codex app, nor a session before its first message.

## Report a problem

Help → Report a Problem… (⌥⌘/) writes a title and a description; the version and commit always go, the
system, the log's last 200 lines and a picture of the window are ticked to begin with, and the preview is the
issue as it will be filed. Home paths, hosts, project names, tokens and e-mail addresses are taken out before
it leaves. It goes through a relay (`relay/`) to a private repository; when the relay can't be reached,
GitHub's new-issue page of this project opens, filled in.

## Appearance and keys

- Five themes (One Dark, Graphite, Darcula, Night, Paper) and your own as JSON, with an editor that checks
  contrast; the code can follow the window or use another theme. Islands or Flat panes.
- File and folder icons in colour (IntelliJ's), replaced by your own in the data folder's `icons/`.
- ⌘+ ⌘− ⌘0 zoom the whole interface. A narrow window folds the menu to icons, then the side panel into a
  drawer; tool windows float.
- Folds and panes move on springs; Settings → Appearance → Animations, or macOS's Reduce motion, stops them.
- Every key can be rebound in Settings → Keyboard, starting from the GitLance, Android Studio or VS Code
  keys; ? shows the keyboard card. On Windows and Linux ⌘ is Ctrl (⌘1…9 is Alt+1…9).

## Read-only

GitLance reads git state; it never stages, commits, checks out or creates worktrees — staging and committing
stay out of it. The exceptions: the background fetch (on for every repository with a remote; Settings →
Repository turns it off for one) writes remote-tracking refs and objects; a request whose branch is not here
(merged and deleted, closed, a fork's) has its head fetched once to `refs/gitlance/requests/<n>`; a clone from
the welcome window runs your own `git clone`. A comment posted to a merge request goes to GitLab or GitHub,
not to the repository.

## Why not an existing app

Fork, Sublime Merge and Zed's git panel show diffs, but none compares the versions of a force-pushed
branch, tracks what was reviewed, or sends comments back to an agent.

## Stack

Rust end to end.

- **UI:** [GPUI](https://www.gpui.rs) — Zed's GPU-accelerated framework, Metal on macOS; pre-1.0, so it is
  pinned to the `gpui-pre` snapshot.
- **Git:** `git2` (libgit2, vendored), read-only; the rebase-aware version diff merges in memory. The `git`
  CLI only for fetching and cloning.
- **Highlighting:** `tree-sitter` for Kotlin, Java, Rust, Swift, TypeScript, JavaScript, Go, Python, JSON,
  YAML and Markdown — a declared function from a call, a parameter from a local, fields, constants — and
  `syntect` with bat's syntaxes (`two-face`) for the rest, in the theme's code colours.
- **Structural diff:** difftastic's JSON output (`difft --display json`), optional.
- **Merge and pull requests:** GitLab's REST API, GitHub's REST and GraphQL, through `curl`.
- **Agents:** an MCP server on stdio in the same binary (`GitLance mcp`).
- **Reports' relay:** a Cloudflare Worker in plain JavaScript (`relay/`, its own [README](relay/README.md)).

## Building

Needs a Rust toolchain (`rustup`, stable) and Xcode's command-line tools; full Xcode is not required.

```sh
./run.sh               # release build; opens the tabs of the last session
./run.sh <path>        # opens only <path>
./run.sh --debug       # a debug build, faster to compile
```

Drag the edges between panes to resize them; a double click restores the default size. Settings, sizes,
the session, themes and comments for agents are kept in `~/Library/Application Support/GitLance/` (Windows
`%APPDATA%\GitLance`, Linux `~/.local/share/gitlance`).

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

A release's notes are its section of `CHANGELOG.md` — written for someone using the app — then its commits by
kind and the compare link (`.github/release-notes.sh`); a tag whose version has no section fails before anything
builds. Run by hand from the Actions tab, `release.yml` only builds; the files stay on the run. `release-notes.yml`
(by hand) writes the notes of releases already made again from `CHANGELOG.md`.

The builds are not signed or notarized. On macOS the first open is refused: right-click the app →
Open, or System Settings → Privacy & Security → Open Anyway (or `xattr -dr com.apple.quarantine
/Applications/GitLance.app`). On Windows, SmartScreen asks: More info → Run anyway.
