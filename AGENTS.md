# GitLance — rules for agents

What GitLance is and the MVP: `README.md`.

- **Everything written here is English** — code, comments, docs, commits.
- **Rust, stable toolchain.** `cargo fmt` and `cargo clippy -- -D warnings` clean, `cargo test` green before
  a commit to `main`.
- **UI first.** GitLance is the app; the binary takes at most an optional repository path. A real CLI is
  outside the MVP — ask first.
- **Read-only towards the reviewed repository.** GitLance reads git state and never changes the index, the
  working tree, refs or reflogs; its only planned write there is `.misc/review.md`. A feature that would
  stage or commit is outside scope — ask first.
- **No worktrees, no checkouts.** Branch and range diffs are read from objects, not from a second working tree.
- **GPUI is pre-1.0.** Pin it to an exact version (or Zed commit) in `Cargo.toml`; an upgrade is its own
  change. It builds with `runtime_shaders`, so full Xcode is not needed.
- **One Dark** is the only theme, for the UI and for code.
- **Checking the UI** needs no screen access: `GITLANCE_SNAPSHOT=<file.png> cargo run --features snapshot -- <repo>`
  renders the window to a PNG and quits (`GITLANCE_SNAPSHOT_VERSIONS=1` first compares the first and last
  versions of the checked-out branch).
- Layout: `src/git/` (read-only git layer, tested on temp repos), `src/highlight.rs` (syntect, One Dark),
  `src/ui/` (GPUI views), `src/storage.rs` (recent repos, pane sizes). `./run.sh` builds and opens it.
- Commits: Conventional Commits — `<type>(<scope>): <subject>`, imperative, lowercase, no trailing period.
- Agent scratch files go in `.misc/` (ignored).
