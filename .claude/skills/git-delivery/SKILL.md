---
name: git-delivery
description: Use when work is about to leave the working tree — committing, splitting a changelist, naming or switching a branch, rebasing, squashing, pushing, opening a merge/pull request — and when deciding what must never be staged. Covers commit subject format and its hook, the placeholder for work without a ticket, rebase versus merge, when a lease-protected force push is right, verifying once before the commit, and attribution. Triggers, in any language — "commit this", "split these changes", "push the branch", "prepare the MR", "squash before delivery", "branch aç", "push et", "MR hazırla". On first use it records the repository's conventions in .agents/workspace.md. Not for read-only git questions, and never a reason to commit on its own — it governs how delivery happens once the developer asks for it.
---

# Git delivery

Delivery rules are project-specific in ways that matter — a repository with an
enforcing `commit-msg` hook and a rebase policy needs different behavior from
one with neither. So this skill resolves the project's actual conventions once,
records them, and then applies them.

## Placement

| Kind | Default root | Visibility | Ownership |
|---|---|---|---|
| Workspace configuration rows | the project's existing configuration | shared | own |

This skill writes no artifacts of its own — it records its rows in whatever
configuration the project already keeps, and creates the local one only when
none exists.

It **reads** the project's `## Artifact placement` section for one thing that
matters at commit time: **every kind whose visibility is `local` is never
staged.** That single rule replaces a hand-maintained exclusion list and cannot
drift away from it — add a local kind, and it is excluded from commits from that
moment. The `Never staged` config row supplements it for anything no kind covers
(`CLAUDE.local.md`, `.claude/settings.local.json`).

## 1. Read or resolve the configuration

Read `.agents/workspace.md`. The rows this skill owns:

| Row | Meaning |
|---|---|
| Commit subject | the format a subject must take |
| Subject enforcement | the hook and its pattern, or none |
| Placeholder branch | prefix for work that must never reach the remote |
| Placeholder subject | the token used until a real key or description exists |
| Integration | `rebase` (preferred) or `merge` |
| Delivered shape | one squashed commit, or preserved commits |
| Protected branches | never rewritten, never force-pushed |
| Commit attribution | on/off, and where it is configured |
| Never staged | paths that must not enter a commit |

If those rows are absent, look before asking: a `## Workspace Configuration`
section in `AGENTS.md`, then the repository's own rule files
(`grep -rniE 'commit|branch|force|attribution' AGENTS.md CLAUDE.md .agents`),
then git itself. Only then run the detection below, ask what it cannot answer,
and write the table — creating `.agents/workspace.md` if the repository has no
rule hub yet. Do this **before** the first commit, not after.

A convention the repository already follows wins over a derived one, even when
nothing wrote it down: git history is a record of a decision.

### Detect

```sh
git log --format='%s' -n 100          # subject convention, and how consistent
git log --oneline --merges -n 50      # merge commits on the mainline -> merge flow
ls githooks .githooks .husky 2>/dev/null
grep -rhoE '\^?\[?A-Z.*\{[0-9],\}' githooks/* .githooks/* .husky/* 2>/dev/null
git branch -r --format='%(refname:short)'   # existing prefixes, protected names
git remote show origin | sed -n 's/.*HEAD branch: //p'
```

Report counts, not just labels: "47 of the last 50 subjects are
`<KEY> : text`" tells the developer something a bare classification does not.
A hook that enforces a pattern is decisive — it is a constraint, not a
convention, and every rule below bends to it.

### Ask, with a recommendation

1. **Integration strategy.** Recommend rebase; merge commits on the mainline are
   evidence of a merge flow. This choice decides everything in section 4.
2. **Delivered shape** — one squashed commit per task, or preserved commits.
3. **Placeholder convention** — confirm the derivation in section 3.
4. **Attribution** — see section 5.
5. **Protected branches** — confirm the detected list.

## 2. Committing

Only when the developer has explicitly asked for a commit.

- Split by coherent intent, not by file type. Present the proposed split before
  committing a large change.
- Keep unrelated and generated output out. Preserve a buildable tree between
  commits where practical.
- **Inspect the staged path list before every commit** and unstage anything that
  belongs to a `local` kind or sits on the `Never staged` row — the scratch
  workspace, task material, local-only rules and instructions, local settings.
  These are the paths a `git add -A` sweeps up silently.
- Name the branch and the commit after what actually changed, in plain words. No
  invented umbrella term that hides the change from a reader.
- **Never bypass a hook.** `--no-verify` is not a way past a failing check.

### Verify once, at the end

Run the repository's compile and lint flow **once, immediately before creating
the commit** — not after every individual edit during iterative fixing. Batch
the work, validate at the end of the batch.

Fix every finding the task introduced. Do not regenerate a lint baseline and do
not add a suppression to make new code pass — a baseline exists for debt that
predates the task, not for debt the task is adding.

## 3. Work with no ticket yet

A branch whose work has no ticket or no final description still needs commits,
and those commits must be impossible to deliver by accident.

**Derive the placeholder from what the repository enforces**, then show the
derivation and let the developer override — an already-established token wins
over a derived one:

| Enforced subject rule | Placeholder branch | Placeholder subject |
|---|---|---|
| none | `draft/<slug>` | `draft: <description>` |
| Conventional Commits | `draft/<slug>` | `chore(draft): <description>` |
| ticket key, e.g. `[A-Z]{2,}-[0-9]+ : ` | `draft/<slug>` | `DRAFT-0 : <description>` |
| some other hook pattern | `draft/<slug>` | the shortest token that satisfies the pattern and is obviously not a real one |

The placeholder has three jobs, and a candidate that fails any one of them is
the wrong token: it must **satisfy the hook** (a rejected commit is not a
workflow), it must be **obviously not real** so it cannot pass review unnoticed,
and it must be a **single fixed string** so `git log` can be grepped for it.

Rules:

- A placeholder branch is **local-only and is never pushed**, regardless of any
  delivery authorization already given. If its work must reach the remote,
  branch from it and push the new branch — and only when the developer asks for
  that specific push.
- **Before pushing any branch, scan its history for placeholder subjects**
  (`git log <base>..HEAD`). They travel: a branch cut from a draft branch
  carries them along. If the real key or description is now known, reword every
  one of them before pushing. If it is not, stop and ask — never push a
  placeholder subject, and never invent a key to replace it.

## 4. Integration and pushing

**Never rewrite a protected or shared branch.** That holds under every strategy
below, and no authorization overrides it.

### Rebase flow (preferred)

Rewriting *your own* task branch is normal here — that is what the strategy is
for.

- `git fetch` immediately before pushing, so the lease is measured against what
  the remote actually holds.
- Push with `--force-with-lease`. **Never bare `--force`.**
- If the lease fails, the remote moved: **stop and report it.** Do not escalate
  to `--force`, and do not retry until you know what moved and why.
- Amend and squash freely among the task's own commits; preserve base history
  and anything that is not the task's.

### Merge flow

A task branch that is merged is not rewritten after it is shared, so **there is
nothing to force-push**. Amend and squash before the first push, and after that
add commits rather than rewriting them.

If a force push looks necessary in a merge flow, that is a signal something else
went wrong — a wrong base, an accidental rewrite, a diverged branch. Stop and
report it rather than forcing.

### Delivered shape

When the configuration says one squashed commit per task, squash the task's own
commits before delivery — rewording any placeholder subjects as part of that
squash — and leave unrelated history untouched. When it says preserve, deliver
the commits as they are; a readable sequence is the point of that policy.

Stop if the worktree holds overlapping user changes, or the remote branch moved
unexpectedly.

## 5. Attribution

Whether a co-author trailer and a session URL appear in commits and merge
requests is a **harness setting, not a prose rule**: `attribution` in
`.claude/settings.json` (which deprecates the older `includeCoAuthoredBy`
boolean; `attribution.sessionUrl` controls the session link separately).

Ask the developer once, then read the current schema and write it — through the
`update-config` skill where available — rather than hardcoding keys from this
file; this setting has already been renamed once. Record the outcome in the
config table.

When attribution is off, say so plainly in the configuration and note that **it
overrides the harness default**, so a session carrying an attribution
instruction knows which one wins. Merge request descriptions follow the same
setting; the reviewer-facing write-up belongs in the task workspace's
`merge_request.md`, not repeated in the commit.

## 6. Reporting

For any code change, report: summary, key changed files, verification actually
performed, and remaining risks or skipped checks. Say which checks were skipped
and why — a silent omission reads as a pass.

## Do not

- Do not stage, commit, revert, or reformat the developer's work without
  explicit authorization; leave changes in the working tree until asked.
- Do not commit local agent, task, or settings files.
- Do not use `--no-verify`, or edit a hook, to get a commit through.
- Do not use bare `--force`, on any branch, for any reason.
- Do not force-push in a merge flow.
- Do not push a branch carrying a placeholder subject.
- Do not push a placeholder branch itself, under any authorization.
- Do not invent a ticket key, a base branch, or a target branch — resolve it
  from the repository or ask.
