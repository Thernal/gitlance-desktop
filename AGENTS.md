# GitLance — rules for agents

What GitLance is and the MVP: `README.md`.
Feature specs, UI mockups and design decisions: the sibling repo `../Design` — a new feature starts there.

- **Everything written here is English** — code, comments, docs, commits.
- **Rust, stable toolchain.** `cargo fmt` and `cargo clippy -- -D warnings` clean, `cargo test` green before
  a commit to `main`.
- **UI first.** GitLance is the app; the binary takes at most an optional repository path. A real CLI is
  outside the MVP — ask first.
- **Read-only towards the reviewed repository.** GitLance reads git state and never changes the index, the
  working tree, refs or reflogs; it writes nothing there. Review comments are kept in its application support directory and copied to the clipboard; a `.misc/review.md` file or an MCP server for agents is a later, separate decision. The one named exception is the opt-in background fetch (`ui/fetch.rs`, the `git` CLI): it writes remote-tracking refs and objects, nothing else. A feature that would
  stage or commit is outside scope — ask first.
- **No worktrees, no checkouts.** Branch and range diffs are read from objects, not from a second working tree.
- **GPUI is pre-1.0.** Pin it to an exact version (or Zed commit) in `Cargo.toml`; an upgrade is its own
  change. It builds with `runtime_shaders`, so full Xcode is not needed.
- **One Dark** is the only theme, for the UI and for code.
- **Checking the UI** needs no screen access: `GITLANCE_SNAPSHOT=<file.png> cargo run --features snapshot -- <repo>`
  renders the window offscreen to a PNG and quits, without taking focus. `GITLANCE_SNAPSHOT_VIEW=unified,lines,words,structural,underlined,wrap,all-lines,whitespace,settings,menu,notice,comments,viewmenu,linemenu`
  turns view options (or the Settings page) on, unsaved; `GITLANCE_SNAPSHOT_SEARCH=<text>` fills the commit search and `GITLANCE_SNAPSHOT_FIND=<text>` the find bar of the diff; `GITLANCE_SNAPSHOT_COMPARE=<base>,<head>` shows a comparison, `GITLANCE_SNAPSHOT_PALETTE=<text>` opens the ⌘K palette (`compare:<text>` the compare picker); `GITLANCE_SNAPSHOT_VERSIONS=1` (with VIEW `interdiff`: the first modified commit pair) compares the first and last versions.
  Never drive the real mouse or keyboard for a check: the developer is using the machine.
- Layout: `src/git/` (read-only git layer, tested on temp repos), `src/highlight.rs` (syntect, One Dark), `src/structural.rs` (difftastic), `src/worddiff.rs` (changed words), `src/search.rs` (commit search),
  `src/ui/` (GPUI views), `src/storage.rs` (recent repos, pane sizes, view options, Settings).
  Icons are Lucide, embedded in `src/ui/icons.rs` (one set, drawn in the text colour; set `.text_color` on the svg itself).
  `src/ui/` splits into `find.rs` (search field), `watch.rs` (auto-refresh: polls `.git`, read-only), `settings.rs`, `palette.rs` (⌘K palette and the compare picker), `shell.rs` (repository tabs: ⌘T ⌘W ⌘⇧T ⌘1…9; the hide-island shortcuts are ⌥⌘1 ⌥⌘2; snapshot env `GITLANCE_SNAPSHOT_TABS=<path>`), `fetch.rs` (opt-in background fetch), `working.rs` (working tree against HEAD, polled every 2 s; VIEW token `worktree`). `./run.sh` builds and opens it.
- Commits: Conventional Commits — `<type>(<scope>): <subject>`, imperative, lowercase, no trailing period.
- Agent scratch files go in `.misc/` (ignored).
