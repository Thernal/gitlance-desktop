# GitLance — rules for agents

What GitLance is and the MVP: `README.md`.

- **Everything written here is English** — code, comments, docs, commits.
- **Rust, stable toolchain.** `cargo fmt` and `cargo clippy -- -D warnings` clean, `cargo test` green before
  a commit to `main`.
- **Read-only towards the reviewed repository.** GitLance reads git state and never changes the index, the
  working tree or refs; its only write there is `.misc/review.md`. A feature that would stage or commit is
  outside the MVP — ask first.
- **No worktrees, no checkouts.** Branch and range diffs are read from objects, not from a second working tree.
- **GPUI is pre-1.0.** Pin it to a Zed commit in `Cargo.toml`; an upgrade is its own change.
- Commits: Conventional Commits — `<type>(<scope>): <subject>`, imperative, lowercase, no trailing period.
- Agent scratch files go in `.misc/` (ignored).
