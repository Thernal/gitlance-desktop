# GitLance — rules for agents

What GitLance is and the MVP: `README.md`.
Feature specs, UI mockups and design decisions: the sibling repo `../Design` — a new feature starts there.

- **Everything written here is English** — code, comments, docs, commits.
- **Rust, stable toolchain.** `cargo fmt` and `cargo clippy -- -D warnings` clean, `cargo test` green before
  a commit to `main`.
- **UI first.** GitLance is the app; the binary takes at most an optional repository path. A real CLI is
  outside the MVP — ask first.
- **Read-only towards the reviewed repository.** GitLance reads git state and never changes the index, the
  working tree, refs or reflogs; it writes nothing there. Review comments are kept in its application support directory and copied to the clipboard; a `.misc/review.md` file or an MCP server for agents is a later, separate decision. The named exceptions: the opt-in background fetch (`ui/fetch.rs`, the `git` CLI) writes remote-tracking refs and objects; and a comment the user posts to a merge request with ⌘↵ (`mr::post_discussion`) goes to GitLab, not to the repository. Comments for an agent (purple) stay on the Mac; comments on a merge request (orange) are public. A feature that would
  stage or commit is outside scope — ask first.
- **No worktrees, no checkouts.** Branch and range diffs are read from objects, not from a second working tree.
- **GPUI is pre-1.0.** Pin it to an exact version (or Zed commit) in `Cargo.toml`; an upgrade is its own
  change. It builds with `runtime_shaders`, so full Xcode is not needed.
- **One Dark** is the only theme, for the UI and for code.
- **Checking the UI** needs no screen access: `GITLANCE_SNAPSHOT=<file.png> cargo run --features snapshot -- <repo>`
  renders the window offscreen to a PNG and quits, without taking focus. `GITLANCE_SNAPSHOT_VIEW=unified,lines,words,structural,underlined,wrap,all-lines,whitespace,settings,menu,notice,comments,viewmenu,linemenu`
  turns view options (or the Settings page) on, unsaved; `GITLANCE_SNAPSHOT_SEARCH=<text>` fills the commit search and `GITLANCE_SNAPSHOT_FIND=<text>` the find bar of the diff; `GITLANCE_SNAPSHOT_KEYS="right cmd-t"` sends keystrokes to the window (not the keyboard); `GITLANCE_SNAPSHOT_COMPARE=<base>,<head>` shows a comparison, `GITLANCE_SNAPSHOT_PALETTE=<text>` opens the ⌘K palette (`compare:<text>` the compare picker); `GITLANCE_SNAPSHOT_VERSIONS=1` (with VIEW `interdiff`: the first modified commit pair) compares the first and last versions.
  Never drive the real mouse or keyboard for a check: the developer is using the machine.
- Layout: `src/git/` (read-only git layer, tested on temp repos), `src/highlight.rs` (syntect, One Dark), `src/structural.rs` (difftastic), `src/worddiff.rs` (changed words), `src/search.rs` (commit search),
  `src/ui/` (GPUI views), `src/storage.rs` (recent repos, pane sizes, view options, Settings).
  Icons are Lucide, embedded in `src/ui/icons.rs` (one set, drawn in the text colour; set `.text_color` on the svg itself).
  `src/ui/` splits into `find.rs` (search field), `watch.rs` (auto-refresh: polls `.git`, read-only), `settings.rs`, `palette.rs` (⌘K palette and the compare picker), `src/mr.rs` (GitLab merge requests and discussions through curl and `~/.config/gitlab-token`; `GITLANCE_MR_FIXTURE=<dir>` reads `mrs.json` and `discussions-<iid>.json` instead), `requests.rs` (the merge-request list, titles, discussions in the review panel; snapshot VIEW `requests`, `GITLANCE_SNAPSHOT_MR=<iid>` opens it; with `GITLANCE_SNAPSHOT_KEYS="c 1 enter h i tab cmd-enter"` it comments on line 1 and the fixture records the post in `posted.txt`), `shell.rs` (repository tabs: ⌘T ⌘W ⌘⇧T ⌘1…9; the hide-island shortcuts are ⌥⌘1 ⌥⌘2, and ⌥⌘3 or the rail's third button (with the request count) swaps the sidebar's top island between branches and merge requests; `c` shows a marker on the diff that follows the typed line number; snapshot env `GITLANCE_SNAPSHOT_TABS=<path>`), `fetch.rs` (opt-in background fetch), `working.rs` (working tree against HEAD, polled every 2 s; VIEW token `worktree`). `./run.sh` builds and opens it.
- **Keyboard**: four focus zones (branches, commits, files, diff; `ui/zones.rs`) — the one with the keyboard has an
  outlined island. Tab / ⇧Tab or ⌃1…4 move between them; ↑ ↓ (j k) act in the zone, ← → (h l) step files (in the file
  tree they close and open folders), ↵ goes one step in, Space / PageUp / PageDown / Home / End move the diff. A new
  list needs its zone arm in `zone_vertical`, and an island border from `zone_border`.
- Commits: Conventional Commits — `<type>(<scope>): <subject>`, imperative, lowercase, no trailing period.
- Agent scratch files go in `.misc/` (ignored).
