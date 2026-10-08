---
name: agent-rules
description: Use for the whole lifecycle of a repository's agent rules — creating, extending, auditing and reconfiguring them — the entry point (AGENTS.md/CLAUDE.md), a session-rule router, a git-ignored scratch workspace, an instructions layer, comment discipline and two-way skill discovery for a second agent runtime; routing a new rule to the tier it belongs in; auditing for dead router rows, drifted config, stale repo maps and outgrown rules. Triggers, in any language — "set up agent rules here", "this repo has no AGENTS.md", "where should this rule go", "add a rule about X", "audit the agent rules", "bu kural nereye yazılmalı", "kurallar eskimiş". Detects what it can, asks the rest, records choices in .agents/workspace.md. Not for writing architecture, module or domain rules themselves, nor for editing one rule whose home is already settled.
---

# Agent rules

This skill owns the *workflow* half of a repository's agent guidance: where
work-in-progress lives, how a task is written down, how commits and delivery are
handled, what an agent must never touch, and how a session finds the rule that
applies to it. It does not write the architecture half — that comes from reading
the codebase, and a template that guesses at it is worse than an empty section.

## Placement

What this skill writes into a consuming project. A project overrides any row in
its `## Artifact placement` section; this skill reads the override and never
hardcodes a path.

| Kind | Default root | Visibility | Ownership |
|---|---|---|---|
| Agent entry point | repository root | shared | own |
| Workspace configuration | `.agents/workspace.md`, `<scratch>/workspace.md`, or split | shared | own |
| Workflow rules | `.agents/` | shared | own |
| Instructions | `<scratch>/instructions/` | local | own |
| Skill sync script | the project's script directory | shared | own |

Defaults lean local: promotion to `shared` is a commit, and a commit is not
undone by deleting the file. The entry point is the exception — something has to
be findable.

## Which stage is this?

| The ask | Stage |
|---|---|
| No agent rules yet, or only an entry point with nothing behind it | **1 — Setup** |
| A new rule exists and needs a home; or a session keeps re-explaining something | **2 — Extend** |
| Rules are suspected stale, contradictory, or unread | **3 — Audit** |
| A recorded setting must change — scratch root, task naming, an optional file | **4 — Reconfigure** |

Stages 2–4 all begin by reading `.agents/workspace.md` and the existing rule
files. Never re-run setup over a configured repository to get at one of them.

Two properties matter more than the file list:

1. **Detected, not asked.** Most settings are already decided by the repository —
   `.gitignore` says what is ignored, `git log` says what a commit message looks
   like, `githooks/` may enforce a ticket prefix. Read them and ask the developer
   to *confirm*, one line each. Blind-asking ten questions that the repo already
   answers is the failure mode this skill exists to avoid.
2. **One recorded config.** Every resolved choice lands in a table at
   `.agents/workspace.md` (or, at the minimal tier, a section of `AGENTS.md`).
   That table is the contract: other skills read it instead of re-interviewing,
   and re-running this skill re-reads it instead of starting over.

## Stage 1 — Setup

### Step 0 — decide whether a hub is warranted at all

A router across a dozen files pays for itself in a repository with several
distinct workflows and a module graph worth documenting. In a repository with
exactly one workflow it costs more than it saves — every session loads a routing
table whose every row points back to the same place.

Pick the tier, and say which you picked and why:

| Tier | Shape | When |
|---|---|---|
| **minimal** | one `AGENTS.md`, conventions inlined under headings | one dominant workflow; few contributors; no second agent runtime; no module graph to map |
| **full** | `AGENTS.md` entry point + `.agents/` hub with router and `session/*.md` | several distinct task types; architecture worth its own map; more than one agent runtime; rules that need to be cited individually |

If the repository already has a working `AGENTS.md` or `CLAUDE.md`, do not
replace it. Read it first, report what it already covers, and offer to *extend*
it — the gaps this skill fills are usually the scratch workspace, the task
convention, and delivery discipline, not the entry point.

### Step 1 — detect

Run the detection pass in `references/detection.md` before asking anything. It
returns a proposed value for: scratch root, task naming, remote tracker, ticket
key format, commit subject convention, protected paths, second agent runtime,
review culture, and documentation language.

Present the results as a confirmation list, not as questions:

```
Detected — correct me on any line:
  Scratch root      .misc/  (NOT git-ignored yet — see question 1)
  Commit subjects   conventional commits (feat:/fix:/…), 47 of last 50
  Ticket keys       none found in branch or commit history
  Protected paths   build/, .gradle/, .idea/, local.properties  (from .gitignore)
  Second runtime    none (.agents/, .codex/ absent)
  Review culture    GitLab remote, .gitlab/merge_request_templates/ present
  Doc language      English
```

### Step 2 — interview, only for what detection cannot answer

Ask these, with a recommendation attached to each. Use `AskUserQuestion` when
the session supports it. Never ask a question whose answer is in the table above.

1. **Scratch root, when `.misc/` is not ignored.** Recommend adding `.misc/` to
   `.gitignore`. If the developer says that is not possible — a shared
   `.gitignore` they do not control, a policy against touching it — fall back:
   list the directories git *already* ignores (`references/detection.md` has the
   command) and let them pick one to nest under, e.g. `.idea/.misc/`. Do not
   invent a third option, and do not proceed with an un-ignored scratch root:
   task files must never be committable by accident.
   **Editing `.gitignore` is a repository change — propose the exact line and
   get approval before writing it.**
2. **Remote task management.** Jira / Linear / GitHub Issues / GitLab Issues /
   none. This decides task-directory naming, and nothing else:
   - tracker configured **and** tasks arrive with a ticket key →
     `<TICKET>-<slug>` (`DCT-12345-payment-retry`). The key already sorts and
     already identifies; a date in front of it is noise.
   - no tracker, or work that arrives without a key → `YYYY-MM-DD-<slug>`.
   A repository can use both: the rule is per task, not per repository — a task
   with a key uses the key, one without uses the date.
3. **`merge_request.md` in the task directory?** Yes when the project delivers
   through MRs/PRs and the description is worth drafting before opening one.
   No for a repository pushed to directly. This is reversible — say so, and
   re-running this skill when the project moves to a review flow turns it on.
4. **`assets/` in the task directory?** Yes when tasks routinely carry
   screenshots, design exports, or captured reference files. No otherwise.
5. **Commit attribution.** Whether Claude Code's co-author trailer and session
   URL appear in commits and PRs is a **harness setting, not a prose rule** —
   `attribution` in `.claude/settings.json` (which deprecates the older
   `includeCoAuthoredBy` boolean; `attribution.sessionUrl` controls the session
   link separately). Ask the developer what they want, then **read the current
   schema and write it through the `update-config` skill rather than hardcoding
   keys from this file** — this setting has already been renamed once. Record
   the outcome in `workspace.md` so a later session does not re-litigate it, and
   note in the generated commit rule that the harness default is overridden
   here, so a session that sees an attribution instruction knows which wins.
6. **Second agent runtime.** If Codex (`.agents/skills/`) or another runtime
   that cannot load `.claude/skills/` is in use, offer two-way skill discovery
   (Step 7). Skip entirely otherwise — an unused sync script is one more thing
   to keep working.
7. **Instructions layer.** Instructions are the agent-facing middle tier: too
   long for rules every session loads, too repository-specific to be a skill.
   Ask two things — **where** they live, and **whether they are committed**:
   - committed (`docs/instructions/`) — teammates and every runtime get them,
     and they become a review surface. A rule can be cited from them.
   - local-only (under the scratch root) — they stay with this developer. A
     teammate's session does not have them, and they can never be cited as a
     shared repository rule.
   Neither is the default; some repositories deliberately keep instructions out
   of the remote. Then propose *candidates* — see Step 6 — rather than creating
   empty files.

Questions about commit subjects, branch naming, placeholder conventions,
integration strategy, and attribution belong to the `git-delivery` skill. If it
is installed, let it resolve those rows; do not ask them here.

### Step 3 — write the configuration first

Before generating any rule file, write the resolved table. It is what this skill
and every other workspace-aware skill read to answer "is this configured, and
with what?".

Where it goes is itself a placement decision (Step 3b): `.agents/workspace.md`
when shared, `<scratch>/workspace.md` when local, both when split. Whichever
exists, the entry point names it, so a session that has never seen this
repository can find it in one hop.

```markdown
# Workspace Configuration

Written by `agent-rules`. Re-run that skill to change a value; editing a
row by hand is fine, but the rule files below it are what actually take effect,
so change both or re-run.

| Setting | Value |
|---|---|
| Hub tier | full |
| Scratch root | `.idea/.misc/` (ignored via `.idea/`) |
| Instructions | `.idea/.misc/instructions/` (local-only, not committed) |
| Task directory | `.idea/.misc/tasks/` |
| Task naming | `<TICKET>-<slug>`, `YYYY-MM-DD-<slug>` without a key |
| Task register | `tasks/INDEX.md` |
| Remote tracker | Jira (`[A-Z]{2,}-[0-9]+`) |
| `merge_request.md` | yes |
| `assets/` | yes |
| Agent discovery | `.claude/skills` ⇄ `.agents/skills`, symlink |
| Protected paths | `build/`, `.gradle/`, `.idea/`, `local.properties` |
| Never staged | scratch root, `CLAUDE.local.md`, `.claude/settings.local.json` |
| Doc language | English |
```

Rows owned by companion skills — write them only when that skill is not
installed, and let it own them when it is:

| Row | Owner |
|---|---|
| Task naming, register, `merge_request.md`, `assets/` | `task-workspace` |
| Commit subject, subject enforcement, placeholder branch and subject, integration, delivered shape, protected branches, commit attribution | `git-delivery` |

At the **minimal** tier this table is a `## Workspace Configuration` section of
`AGENTS.md` instead of its own file. Same rows, same meaning.

### Step 3b — write the placement section

The config table says *what* was decided. `## Artifact placement` says **where
each kind of artifact goes, who may see it, and who may edit it** — the section
every other skill resolves against before writing anything.

```markdown
## Artifact placement

Defaults: **local** unless promotion is deliberate; **own** unless a source of
truth is named. A skill resolves: its own default → this table → the local
table, last one winning.

| Kind | Root | Visibility | Ownership |
|---|---|---|---|
| Agent entry point | repository root | shared | own |
| Workspace configuration | `.agents/workspace.md` + `<scratch>/workspace.md` | split | own |
| Workflow rules | `.agents/` | shared | own |
| Instructions | `<scratch>/instructions/` | local | own |
| Mirrored standards | `.claude/rules/` | shared | foreign — <source>, <revision> |
| Project skills | `.claude/skills/` | shared | own |
| Vendored skills | `.claude/skills/` | shared | foreign — <repo>@<commit> |
| Task workspace | `<scratch>/tasks/` | local | own |
| Reports, logs, drafts | `<scratch>/` | local | own |
| External reference documents | `<scratch>/references/` | local | foreign |
```

Two properties, independent of each other:

- **Visibility** — `shared` (committed, a review surface, teammates' sessions
  get it) or `local` (git-ignored, this machine only, never citable as a shared
  rule). All four combinations with ownership occur; treating them as one axis
  is the usual mistake.
- **Ownership** — `own` (authored here) or `foreign` (a mirror or vendored copy
  whose source of truth is elsewhere). A `foreign` row **names its source and
  pinned revision**, is never edited in place, and a disagreement with it is
  reported rather than resolved locally.

Ask about a kind only where the repository does not already answer it. The two
that usually need asking are **Instructions** (Step 2.7) and the **configuration
itself**: it may be shared (the contract is published; local paths are visible
in the remote), local (nothing leaks; every machine re-runs discovery), or
**split** — shared rows for the team contract, local rows for what must not
leave the machine. Split is the right answer more often than either extreme;
recommend it where the project keeps anything local at all.

**Precedence runs in opposite directions for the two kinds of content**, and
getting it backwards is how a mirrored standard quietly loses to someone's local
preference:

- **Config values** — paths, naming, on/off: local overrides shared.
- **Invariants and rules**: shared or foreign wins. A local rule adds detail
  where the shared one is silent; it never relaxes it.

### Step 4 — install the companion skills

Two areas have their own skills, and those skills are the single source for
them. This skill does not restate their content:

| Area | Skill |
|---|---|
| Task directories, plans, progress, open questions | `task-workspace` |
| Commits, branches, placeholder conventions, integration, pushing | `git-delivery` |

Offer to install them (`skillctl.sh install task-workspace`,
`skillctl.sh install git-delivery`). Where one is installed, the generated rule
file for that area is a **pointer**: what the area covers, which skill governs
it, and which config rows apply — a few lines, not a copy. Where one is
declined, write no pointer for it and say so in the report; a rule file routing
to something that is not there is worse than a missing rule.

### Step 5 — generate the rule files

`references/generation.md` gives the per-file specification: which files exist at
each tier, what each one must contain, and — importantly — which sections must be
written from the repository rather than filled from a template.

Three rules govern the writing:

- **Every generated rule reflects a decision from `workspace.md`.** Do not emit
  a rule about `merge_request.md` when the table says no, or a skill-sync rule
  when the table says no second runtime. A scaffold full of inapplicable rules
  teaches sessions to skim.
- **`repo-map.md` is read, not templated.** Its section headings come from this
  skill; every line under them comes from the actual repository — module list,
  real build commands, the areas that are genuinely high-risk. If the repository
  cannot be mapped in this session, write the headings with an explicit
  `TODO: fill from the codebase` and say so in the report, rather than inventing
  plausible-looking structure.
- **Architecture invariants stay empty.** `.agents/AGENTS.md` gets its
  `## Architecture invariants` heading and a one-line note that it is
  unpopulated. Filling it is a separate task by someone who has read the code.

`skill-assets/agent-rules` in the knowledge repository holds a worked example of
a filled hub. Fetch it only if the developer wants to see the target shape
before committing to it:

```
skillctl.sh asset-install agent-rules --dest <somewhere-throwaway>
```

It is a reference for the *shape*, not a source to copy: its module names,
verification commands, and risk areas belong to another project.

### Step 6 — propose instruction candidates

Do not create an empty `instructions/` tree. Instead, name the topics that
*should* become instructions in this repository and let the developer pick.

The four-way routing test — which is also what `doc-feedback-loop.md` teaches
the repository to apply later:

| Where it belongs | Test |
|---|---|
| `.agents/AGENTS.md` | every session must respect it, whatever the task |
| an instruction | needed only for one class of task, procedural, and specific to this repository |
| a skill | a procedure that would still be valid in a different repository |
| `docs/` | the audience is a developer, not an agent |

A candidate is a subsystem where sessions repeatedly need procedural detail that
is too long to load every time and too local to publish as a skill — navigation,
networking and error handling, testing conventions, build conventions, the
design system, persistence and migrations, localization, release configuration.
Propose only the ones this repository actually has, each with one line saying
what the instruction would cover and which detected evidence suggests it.

Create only what the developer picks, as a heading skeleton plus
`TODO: fill from the codebase`. Writing the content is a separate task by
someone who has read that subsystem — the same rule as the architecture
invariants. Then add a router row in `.agents/AGENTS.md` for each file created,
and record the location and visibility in the config table.

### Step 7 — two-way skill discovery (only if Step 2.6 said yes)

The model: **one real directory per skill, one symlink**. A skill's real
directory may live under either `.claude/skills/<name>/` or
`.agents/skills/<name>/`; the other path holds a link to it. Editing through
either path edits the same files, so there is never a mirror to keep in sync by
hand and never two copies that silently drift apart.

Install `scripts/sync-agent-skills.sh` (ships with this skill) into the
project's script directory, then generate `session/skill-sync.md` per
`references/generation.md`. The script:

- creates the missing link for every real skill directory on either side;
- removes only *managed* stale links (those pointing into the two skill roots)
  and never a real directory;
- refuses when both paths hold real content for the same skill — that is a
  conflict a human resolves;
- `--check` reports without writing and exits non-zero, so it works in a
  pre-commit hook or CI.

Run it once at the end of setup, then `--check`, and report the result.

## Stage 2 — Extend: route a new rule

A rule arrives — from a repeated mistake, from the `doc-feedback-loop`, or
because the developer says so. The work is deciding **which tier it belongs to**,
not writing it down wherever the conversation happens to be.

Apply the four-way test:

| Where it belongs | Test |
|---|---|
| `.agents/AGENTS.md` | every session must respect it, whatever the task |
| an instruction | needed only for one class of task, procedural, specific to this repository |
| a skill | a procedure that would still be valid in a different repository |
| `docs/` | the audience is a developer, not an agent |

Then:

1. **If it belongs to an installed companion skill's area, do not write it
   locally.** A local rule about commit subjects or task files diverges from the
   skill the moment either changes. Report it upstream instead
   (`skillctl.sh report <skill> --title … --body …`) and say that is what you
   did.
2. Write it in the file the test chose. Add a `session-rules.md` row only when a
   new `session/*.md` file was created; add an `.agents/AGENTS.md` task-router
   row only when a new instruction was created.
3. If the rule implies a setting — a path, a naming convention, an on/off
   choice — add the row to the config table too. A rule whose parameter lives
   only in prose cannot be read by a skill.
4. **If it introduces a new kind of artifact, add a `## Artifact placement`
   row** before anything writes one: root, visibility, ownership. A kind that
   gets written before it is placed lands wherever the first session guessed,
   and a `foreign` kind written without provenance cannot be updated later.
   Put the row in the shared table when it is part of the team contract, in the
   local one when the path must not leave the machine.
5. **Keep the always-loaded set short.** `AGENTS.md` plus `.agents/AGENTS.md`
   are paid for by every session. When they grow, the excess moves down a
   tier — that is a normal outcome of this stage, not a failure.

## Stage 3 — Audit

Read everything, report findings, fix only what the developer approves. The
checks, in the order they are worth running:

1. **Dead router rows.** Every `session-rules.md` and task-router target
   resolves to a file that exists.
2. **Pointer integrity.** A pointer file exists for each installed companion
   skill and for no uninstalled one. `ls .claude/skills` settles it.
3. **Config drift.** Every row in the table is reflected by the rules, and no
   rule contradicts a row. Common drift: the scratch root moved, optional task
   files were turned on in practice but never recorded, attribution was changed
   in settings but not in the table.
4. **Placement integrity.** For every `## Artifact placement` row: a `local`
   root is still git-ignored (`git check-ignore -v`), a `shared` root has not
   drifted into `.gitignore`, and nothing of a `local` kind is tracked
   (`git ls-files <root>`). **A tracked local artifact is the one finding worth
   interrupting for** — task notes and machine paths are already in the remote
   by the time anyone notices.
5. **Foreign artifacts.** Each `foreign` row names a source and a pinned
   revision, and its files are unmodified since that revision. One edited in
   place is a conflict to report, never something to silently re-mirror.
6. **Unplaced kinds.** Directories on disk that no row covers — invented by some
   session along the way. Propose the row, or propose removing the directory.
7. **`repo-map.md` against reality.** Modules that no longer exist, modules
   missing, and — the usual one — commands that no longer run. Verify the
   commands rather than reading them.
8. **Unfilled `TODO`s**, especially the architecture invariants. Report them as
   open work, not as failures; filling them is a separate task.
9. **Instruction candidates that accumulated.** A subsystem explained inline in
   `.agents/AGENTS.md`, or re-explained across several sessions, has outgrown
   the always-loaded tier.
10. **Skill sync**, where enabled: `scripts/sync-agent-skills.sh --check`.
11. **Size.** If the always-loaded files no longer fit a quick read, say so with
    the line counts and propose what moves down a tier.

## Stage 4 — Reconfigure

Changing a recorded setting is this stage, not a fresh setup. It must:

1. read the existing `workspace.md` and present it as the current state;
2. ask only about what is changing;
3. rewrite the table and **only the rule files affected by the changed rows**;
4. before overwriting any generated file, diff it against what this skill would
   have written — if it has been edited by hand since, show the difference and
   ask, rather than discarding someone's work.

The common cases are the ones called reversible above: a project that starts
without a tracker and later adopts one, or starts pushing directly and later
moves to merge requests.

## Report

**Setup:** the tier chosen and why, the table as written, the files created or
changed, which companion skills were installed and which pointers were therefore
skipped, the instruction candidates proposed and which were taken, anything left
as `TODO` (typically `repo-map.md` and the architecture invariants), and — if
`.gitignore` was touched — the exact line added.

**Extend:** which tier the rule landed in and why the other three were rejected,
plus anything that moved down a tier to make room.

**Audit:** findings in the order above, each with the evidence that produced it,
separated into what was fixed and what needs a decision. Say plainly when
nothing is wrong — an audit that invents findings to look useful is worse than
one that reports a clean result.

**Reconfigure:** the rows that changed, the files rewritten, and any hand-edited
file whose difference was preserved rather than overwritten.

## Do not

- Do not write rules the repository has not decided. An unpopulated section
  marked `TODO` is honest; a plausible invention is a rule someone will follow.
- Do not touch `.gitignore`, `.claude/settings.json`, or any existing rule file
  without showing the change and getting approval.
- Do not set up a scratch root that git does not ignore.
- Do not replace an existing `AGENTS.md`/`CLAUDE.md` — extend it.
- Do not copy the example asset's module names, commands, or risk areas into a
  different project.
- Do not install the sync script for a repository with only one agent runtime.
- Do not restate a companion skill's content in a generated rule file. If the
  skill is installed, point at it; if it is not, leave the area to the skill
  rather than writing a divergent local copy of its protocol.
- Do not create an empty instructions tree, and do not publish instructions to
  the remote when the developer chose to keep them local.
- Do not hardcode a path that a placement row should decide, and do not write an
  artifact of an unplaced kind — add the row first.
- Do not edit a `foreign` artifact in place, and do not record one without its
  source and pinned revision.
- Do not promote a `local` artifact to `shared` without the developer's
  approval. Promotion is a commit; it is not reversible by deleting the file.
