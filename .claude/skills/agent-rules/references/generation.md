# Generated file specification

What to write, per file, per tier. Every entry says whether its content comes
from **spec** (this file supplies it, adapted to the config table), from **repo**
(it must be read out of the codebase), or is **conditional** on a row in
`workspace.md`.

Keep every generated file short. These are read at the start of sessions; a rule
nobody finishes is a rule nobody follows. If a file grows past roughly a screen
and a half, the excess belongs in a skill or in `docs/`, not here.

---

## Minimal tier

One file: `AGENTS.md`, with these sections in order.

| Section | Source |
|---|---|
| One-paragraph statement of what the repository is and its single workflow | repo |
| `## Workspace Configuration` — the config table | spec |
| `## Artifact placement` — kind → root, visibility, ownership | spec |
| `## Scratch workspace` — the `misc-workspace` content, condensed to its table | spec |
| `## Task workspace` — layout, required order, and a pointer to the `task-workspace` skill | pointer |
| `## Commits and delivery` — `working-tree` content, plus a pointer to the `git-delivery` skill | pointer |
| `## Protected files` | repo |
| `## Code comments` | spec |
| `## Do not` | spec + repo |

No router, no `.agents/` directory. The config table lives in the
`## Workspace Configuration` section rather than its own file; companion skills
read it from there. Add `CLAUDE.md` only if it does not exist: four lines
pointing at `AGENTS.md`.

A minimal-tier repository rarely warrants an instructions layer. Propose one
only if a subsystem already generates repeated procedural questions.

---

## Full tier

### `AGENTS.md` (repository root) — spec

The entry point, and nothing else. Names the hub and the mandatory reading
order, and stops. Around fifteen lines.

```markdown
# <Project> Agent Entry Point

Repository rules live in `.agents/`.

Before modifying code, read:

1. `.agents/AGENTS.md`
2. `.agents/repo-map.md`
3. `.agents/checklists.md`
4. `.agents/session-rules.md`

Then read only the task-specific guide or skill routed by those files.
```

**The entry point names where the configuration lives** — shared file, local
file, or both — so a session that has never seen the repository finds it in one
hop instead of running a discovery ladder.

Append a line per conditional rule that must be read *before* a class of work
(the skill-sync rule is the usual one).

### `CLAUDE.md` — spec, only if absent

A pointer to `AGENTS.md`, plus anything Claude-specific: the skill discovery
path, and the fact that rules are shared with other runtimes. Never a second
copy of the rules. If a `CLAUDE.md` already exists with real content, leave it
and report the overlap instead.

### `.agents/README.md` — spec

A map of the hub itself: one line per file saying what belongs in it, including
each half of a split configuration and which one wins for values. Its job is
to answer "where does this new rule go?" so that rules stop landing in whichever
file was open. Include a line for the instructions layer with its location and
whether it is committed, and a line for each installed companion skill. Close
with the routing principle:

> Repository-wide invariants in `.agents/AGENTS.md`, workflow behavior in
> `.agents/session/`, per-subsystem procedure in the instructions layer,
> cross-repository procedure in skills, developer-facing documentation in
> `docs/`, and task-local material in the scratch workspace.

### `.agents/AGENTS.md` — mixed

The repository-wide invariants. Sections:

- `## Task router` — **spec structure, repo content.** A table mapping a kind of
  task to the file, instruction, or skill that governs it. Rows exist only for
  guidance that actually exists; a row pointing at a file this skill did not
  create and the repository does not have is a dead link. Instructions created
  in Step 6 each get a row here, and so does each installed companion skill.
- `## Project context` — **repo.** What an agent must know before touching
  anything: what the applications are, how modules are organized, which
  frameworks are settled. Read it out of the codebase. If this session did not
  read enough of the repository to write it, leave `TODO` and say so.
- `## Mandatory first steps` — **spec.** Inspect `git status --short --branch`
  and preserve unrelated changes; read the session rule the task routes to; read
  the task register if one exists; identify ownership before creating files;
  prefer the smallest coherent change.
- `## Architecture invariants` — **deliberately empty.** Heading plus:
  `> Unpopulated. Fill from the codebase in a dedicated pass — do not guess.`
- `## Code and security` — **spec + repo.** Documentation language; no secrets,
  tokens, signing material, or local SDK paths committed; no logging of auth
  data or PII. Repository-specific security rules come from the codebase.
- `## Verification` — **repo.** The real commands, narrowest first, from the
  detection pass. Plus the spec rule: report checks that were skipped and why.
- `## Do not` — **spec + repo.** Never modify generated output; never revert,
  reformat, stage, or discard unrelated user changes; never create commits
  unless asked.

### `.agents/repo-map.md` — repo

Headings from spec, every line under them from the repository:

`## Sources of truth` (the files that decide versions, module discovery, build
config) · `## Applications` · `## Modules` (or the repository's own top-level
unit) · `## Build logic` · `## Common commands` · `## High-risk areas`.

`## Common commands` must contain commands that were verified to exist. `##
High-risk areas` is the section with the most value and the least template
content: authentication and session lifecycle, routing, network interception,
persistence migrations, release and signing configuration — but only the ones
this repository actually has.

### `.agents/checklists.md` — spec skeleton, repo content

Numbered lists an agent walks before reporting done. Start with three:
`## Implementation`, `## Review`, `## Security review`. The spec supplies the
universal steps — confirm ownership, inspect `git status`, reuse an existing
contract before adding an abstraction, add focused tests for branching and error
behavior, update the documentation a change invalidates, run the narrowest
verification then broaden, report risks and skipped checks. Repository-specific
steps (dependency direction, layer boundaries, generated-code regeneration) are
appended from the codebase or left out.

### `.agents/session-rules.md` — spec

The router. A two-column table, `Situation` → `Read`, with one row per
`session/*.md` file that was actually generated — **plus one row for every
skill and slash command the repository already has** (`detection.md`). Word
those rows from the task angle like the rest, naming the skill and its path.

Two rules keep the router honest:

- **A topic an existing skill already covers is cut from the hub, not written
  twice.** A hub section and a skill covering the same ground is the same
  competing-copies failure as two rule files, just between a doc and a skill.
- **A rebuild keeps the rows it did not write.** Generation accounts for the
  files it just produced; the skills were there before and stay reachable.
  Dropping their rows is how a repository silently loses access to its own
  tooling.

Ends with the standing rule:

> When a documented rule is stale, or a repeatable mistake could have been
> prevented by documentation, propose a concrete documentation update in the
> final report.

---

## `.agents/session/` files

### `task-workspace.md` — pointer

Written only when the `task-workspace` skill is installed. It is a router entry,
not a protocol — the skill is the single source, and a local copy of it goes
stale the moment the skill is updated.

```markdown
# Task Workspace

Any task that earns a written plan owns one directory under
`<scratch>/tasks/`, holding `task.md`, `plan.md`, `progress.md`, and
`questions.md` — plus `merge_request.md` and `assets/` where enabled.

The protocol — required order, what each file contains, the question and
handover rules, and the register — is the `task-workspace` skill. Read it
before creating or resuming a task directory.

Settings it reads from `.agents/workspace.md`: scratch root, task naming,
task register, `merge_request.md`, `assets/`, remote tracker.
```

### `misc-workspace.md` — spec, parameterized

States that `<scratch>` is git-ignored (naming *how* it is ignored, from the
table) and is used **by default, without being asked**, whenever a task produces
something that is not a finished deliverable. Then the routing table — keep only
the rows the project will plausibly use:

| Material | Directory | Naming |
|---|---|---|
| Task brief, plan, progress, questions | `tasks/` | per the `task-workspace` skill |
| Per-subsystem procedural guides, when the config table puts them here | `instructions/` | topic-named |
| Review, audit, or architecture report | `reports/` | `YYYY-MM-DD-topic-review.md` |
| Crash or incident notes | `incidents/` | `YYYY-MM-DD-topic.md` |
| External reference material that must not be committed | `references/` | topic-named |
| Logs and diagnostic dumps | `logs/` | dated, topic-named |
| Generated patches, builds, screenshots | `artifacts/` | task-named subdirectory |

**The routing table is also a lookup table.** When the developer refers to
something without giving a path — "look at the crash", "check that spec", "the
log from yesterday", "read the review report" — search the scratch workspace
before asking where it is. The kind names the directory, and a dated,
topic-named file is usually one `ls` away. Ask only after looking.

Close with: durable team documentation does not go here — it goes to `docs/` or
`.agents/`; and the scratch workspace is never cited as the only source of a
repository rule. Use one date format (`YYYY-MM-DD`) for everything new.

### `working-tree.md` — spec

The developer's working tree and the shape of the final response. Delivery
mechanics are not here — they belong to the `git-delivery` skill.

Preserve unrelated user changes. Do not stage, commit, revert, or reformat the
developer's work without explicit authorization. Leave changes in the working
tree unless commits were asked for. Match the developer's language in the final
response while keeping code and repository documentation in the documentation
language from the table. For code changes, report summary, key changed files,
verification performed, and remaining risks or skipped checks. State material
assumptions before implementing when the repository cannot answer them.

Include the honesty clause verbatim in substance: never imply verification that
was not performed — "checked" means run or read, not reasoned about. Write it
with more weight in a repository the detection step found to have no test suite
or no build, because there the developer has nothing but the session's word.

### `commit-workflow.md` — pointer

Written only when the `git-delivery` skill is installed.

```markdown
# Commits and Delivery

Commit splitting, subject format, branch naming, the placeholder convention for
work with no ticket yet, integration strategy, when a lease-protected force push
is appropriate, and commit/PR attribution are the `git-delivery` skill. Read it
before committing, branching, rebasing, or pushing.

Settings it reads from `.agents/workspace.md`: commit subject, subject
enforcement, placeholder branch and subject, integration, delivered shape,
protected branches, commit attribution, never staged.
```

When the skill is **not** installed, write the minimum locally instead: commits
only when explicitly asked; split by coherent intent; inspect the staged path
list and unstage anything on the "never staged" row; never bypass a hook; never
bare `--force`; never rewrite a protected branch.

### `change-strategy.md` — spec

Follow established patterns before introducing abstractions. Before adding a
module, confirm it has independent ownership, consumers, or build value. Before
adding a dependency, check whether the existing stack solves the problem; if one
is still needed, record rationale, alternatives considered, risk, and binary or
build impact. Do not add an interface solely to mirror one stateless
implementation — put seams at ownership or test boundaries. Prefer
constructor-provided collaborators over service locators or hidden singletons.
Before adding a wrapper or helper, check whether an existing API handles the
case with a parameter or call-site change. Avoid magic strings and numbers. When
a repository-wide choice is unsettled, present options and tradeoffs before
implementing rather than settling it as a side effect of a feature task.

### `code-comments.md` — spec

Language-agnostic; ships as written.

- Comment why, not what. Self-explanatory code carries no comment; a name that
  needs a comment usually needs a better name instead.
- Do not document every parameter, property, or function. Reserve a comment for
  what the reader cannot see: a non-obvious edge case, an external constraint,
  or the reason an option was rejected.
- Never explain baseline language or framework knowledge a competent developer
  on this stack already has. A comment justifies a project-specific choice or
  constraint; it does not teach the platform.
- Never repeat what the code or a nearby comment already states — including a
  call site restating a rationale already documented on the declaration it
  calls. Send the reader to the declaration instead of copying its reasoning to
  every use.
- Delete or correct a comment when the code around it changes; a stale comment
  is worse than none.
- A capability with no established usage pattern earns a short usage comment on
  its public surface, where a consumer reads it — not on the implementation.

### `risk-levels.md` — spec skeleton, repo content

Three levels with repository-specific examples: **Low** (documentation,
comments, isolated presentational change), **Medium** (feature state, data
mapping, dependency changes, routing destinations), **High** (authentication and
session lifecycle, root routing, network auth interception, release and signing
configuration, persistence migration). Fill each from the actual codebase.

Then the gates for high-risk work: write a plan in the task workspace first;
identify rollback or migration behavior; add focused automated tests where
practical; run the broader build when shared code is affected; report the manual
verification steps.

### `protected-files.md` — repo

The generated and machine-owned paths from detection, as a list. Then: an
exception requires a task that explicitly owns the generated artifact (a schema
migration is the usual one). Never commit local SDK paths or secrets.

### `doc-feedback-loop.md` — spec

Propose a specific documentation update when a documented rule no longer matches
the code, when a repeatable mistake could have been prevented by a rule, or when
a concrete repository-wide workflow improvement becomes clear. Do not create
shared rules for one-off task details, and do not duplicate existing guidance.

Route updates with the four-way test:

| Where it belongs | Test |
|---|---|
| `.agents/AGENTS.md` | every session must respect it, whatever the task |
| an instruction | needed only for one class of task, procedural, specific to this repository |
| a skill | a procedure that would still be valid in a different repository |
| `docs/` | the audience is a developer, not an agent |

Workflow behavior goes to `.agents/session/*.md`; task-only analysis stays in
the task workspace. When not explicitly authorized to change shared guidance,
present the exact proposed edit for confirmation. When the config table says
instructions are local-only, an instruction never becomes a committed file as a
side effect of this loop.

### `skill-sync.md` — conditional, spec

Only when the config table says two-way discovery is on.

- One real directory per skill and one symlinked discovery path. The real
  directory may originate under either root; editing through either path edits
  the same files. Never two real copies.
- Links point at the whole skill directory, not just its `SKILL.md`.
- Before a rename, move, or delete, resolve the symlink and operate on the real
  directory.
- Run the sync script after every create, rename, move, or delete, and
  `--check` before reporting completion. Editing existing content needs no
  relink, but the final `--check` is not optional. **Running the script is
  required where this rule requires it; naming the command without executing it
  does not satisfy the rule.**
- Keep any per-runtime descriptor file consistent with `SKILL.md`.
- If both paths hold real directories for the same skill, stop — that is a
  conflict requiring manual reconciliation.
- If discovery paths change, update this rule, the entry points, and the script
  together.
