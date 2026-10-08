# Workspace configuration (shared)

Local rows (scratch root, task naming) live in `.misc/workspace.md`; on a conflicting value the local table wins.

## Git delivery

| Row | Value |
|---|---|
| Commit subject | Conventional Commits — `<type>(<scope>): <subject>`, imperative, lowercase, no trailing period (`AGENTS.md`) |
| Subject enforcement | none — no hook |
| Placeholder branch | `draft/<slug>` — local only, never pushed |
| Placeholder subject | `chore(draft): <description>` |
| Integration | rebase |
| Delivered shape | preserved commits |
| Protected branches | `main` |
| Commit attribution | on — `Co-Authored-By` trailer, as in the first commit |
| Never staged | `.misc/`, `CLAUDE.local.md`, `.claude/settings.local.json` |
