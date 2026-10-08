---
name: task-workspace
description: Use whenever a task is large enough to earn a written plan — starting, planning, resuming, handing over, or closing one. Maintains a per-task directory of local, never-committed files — what was asked, the plan, where the work stands right now, the decisions that were the developer's rather than the agent's, and the reviewer description. Triggers, in any language, include "start this task", "plan this before implementing", "resume the X task", "where did we leave off", "what is still open", "bu taskı planla", "kaldığımız yerden devam", "açık tasklar neler". Do NOT use for a read-only question, an explanation, a review with no implementation, or a change small enough that writing the plan costs more than making it — a one-line fix does not get a directory. Do NOT use it to store durable team documentation — everything it writes is git-ignored and is never cited as the source of a repository rule.
---

# Task workspace

A task's directory is its memory: what was asked, how it will be done, where it
stands, and which decisions were the developer's rather than the agent's. It
exists because the session that finishes a task is usually not the session that
started it.

Two rules shape everything below, and every specific rule here follows from one
of them:

- **A task delivers one merge request.** When it starts delivering several, it
  is an epic, and the unit is the merge request. See *When a task becomes an
  epic*.
- **Files are split by lifetime, not by topic.** What is read on every resume
  lives apart from what is read once a month. Splitting a file because it covers
  a different subject only makes the same content harder to find.

This skill is the single source for that protocol. A repository whose rules
mention a task workspace points here rather than restating it.

## Placement

| Kind | Default root | Visibility | Ownership |
|---|---|---|---|
| Task workspace | `<scratch>/tasks/` | local | own |
| Task register | `<scratch>/tasks/<register>` | local | own |
| Workspace configuration | `<scratch>/workspace.md` | local | own |

Everything this skill writes is **local**: a task directory holds unfinished
judgment, a developer's open questions, and a machine's working state. None of
it is ever committed, and none of it is ever cited as the source of a
repository rule.

The configuration row is a default, not a claim: where a project keeps its
configuration is that project's decision, recorded in its own placement section.
This skill creates the local one only when nothing else exists.

## Finding the workspace

**Never ask where tasks go until you have looked.** A repository that already
has a scratch convention gets its own convention used, not a second one created
beside it. Work down this ladder and stop at the first step that answers:

1. **The configuration**, wherever the project keeps it — `.agents/workspace.md`
   when shared, `<scratch>/workspace.md` when local, both when split. Read both
   if both exist: **the local table wins on a conflicting value**, because where
   a directory lives on this machine is this machine's business. The entry point
   usually names it.
2. **A `## Workspace Configuration` section in `AGENTS.md`** — the same table at
   the minimal tier.
3. **The repository's own rule files.** Grep them for a scratch path before
   assuming there is none:
   ```sh
   grep -rniE '\.misc|scratch|\.idea/\.misc|tasks/' \
        AGENTS.md CLAUDE.md .agents .cursor 2>/dev/null | head -20
   ```
4. **An existing `tasks/` directory on disk.** If one exists, read the
   directories in it: their naming *is* the convention, whether or not anything
   wrote it down.
   ```sh
   find . -maxdepth 4 -type d -name tasks -not -path '*/node_modules/*' 2>/dev/null
   ```
5. **A scratch-shaped ignored directory** — `.misc`, `.scratch`, `.local`,
   `.notes`, `.idea/.misc`, or whatever the repository ignores:
   ```sh
   git status --porcelain --ignored=matching -- . | awk '$1=="!!"{print $2}'
   ```
6. **Only now, ask** — the three questions below.

Whatever the ladder returns, **verify it before writing into it**:

```sh
git check-ignore -v <path>
```

A directory git tracks is not a scratch root, however scratch-shaped its name
is. If the verification fails, say so and fall back to the question rather than
creating a task directory git will commit.

When the answer came from steps 3–5 rather than from a config table, **record
it** so the next session skips the ladder: extend the existing configuration if
there is one, or create the local one with the rows this skill owns. Discovery that is not
written down is repeated.

## Configuration

The rows this skill uses:

| Row | Used for |
|---|---|
| Scratch root | where `tasks/` lives |
| Task naming | `<TICKET>-<slug>` or `YYYY-MM-DD-<slug>` |
| Task register | the register filename, or none |
| `merge_request.md` | whether the reviewer description is drafted here |
| `assets/` | whether the task directory holds supporting files |
| Remote tracker | whether a ticket description is fetched and preserved |

Ask only what the ladder could not answer:

1. **Scratch root**, when nothing ignored exists. Propose adding `.misc/` to
   `.gitignore` — show the exact line and get approval. If that is refused, list
   what git already ignores and nest under one of those, preferring an IDE
   directory over a build output that tooling wipes.
2. **Ticket keys.** If tasks arrive with a tracker key, naming is
   `<TICKET>-<slug>`; otherwise `YYYY-MM-DD-<slug>`. This is decided per task,
   not per repository — a repository can use both.
3. **`merge_request.md` and `assets/`** — on or off, both reversible later.

When the repository has a rule hub, `agent-rules` writes that table instead, and
this skill only reads it.

Whatever this skill records goes in the table the project's placement section
points at — never a second config file beside an existing one.

## Layout

```text
<scratch>/tasks/<TICKET>-<slug>/        # or <scratch>/tasks/YYYY-MM-DD-<slug>/
  task.md            # what was asked, and what "done" means
  plan.md            # how it will be done
  progress.md        # where the work stands right now — the only mandatory read
  questions.md       # only while a developer decision is open
  decisions.md       # questions once answered, one line each
  findings.md        # only when review or device passes produce a defect list
  journal/YYYY-MM.md # what happened, in order — never read on resume
  merge_request.md   # only if enabled
  assets/            # only if enabled
```

The slug names the goal, not the module: `payment-retry-backoff`, not
`payments`. A date, once written, never changes — not when the task is resumed,
not when it is renamed.

Read frequency is the point of this split, and it is the reason none of these
becomes a directory of its own. `progress.md` is read every time; `journal/`
essentially never. Sharding a file that should be capped at 120 lines only turns
one read into three.

`references/templates.md` holds the minimum skeleton for each file.

## Required order

1. **`task.md` first** — before any plan, any exploration write-up, and any code
   change. A task without it is not started.
2. **`plan.md` second** — after `task.md`, and after reading the repository
   guidance the task routes to. Never before the goal is recorded.
3. **`progress.md` as soon as work starts**, and updated from then on.
4. **`questions.md` last, and only when needed** — after the plan exists, when a
   question survives it and its answer would change the work.

The rest — `decisions.md`, `findings.md`, `journal/` — are created the first
time there is something to put in them, never pre-created empty.

## `task.md`

- A header naming what this task is part of: `epic:` when it belongs to one, and
  `prev:` naming the task it follows. See *The chain is the invariant*.
- `Request` — what the developer asked, in the agent's own words, with anything
  implied made explicit. Where a tracker supplies the description, preserve it
  verbatim; never invent missing ticket content, and say so when the tracker is
  unreachable.
- `Goal` — the end state, in one or two sentences.
- `Scope` — what is in, and *explicitly* what is out.
- `Done when` — the verification that proves the task finished: commands,
  checks, artifacts.
- `Status` — `Planning` / `Awaiting answers` / `In progress` / `Done` /
  `Abandoned`, kept current.

## `plan.md`

- Ordered phases, easiest to hardest. Each states what changes, why, which
  files, and how it is verified.
- `Risk` and `Rollback` for anything touching build logic, routing, networking,
  persistence, or release configuration.
- `Decisions` — the answers that shaped the plan, mirrored from `decisions.md`
  where they changed the approach. **The plan, not the question file, is where a
  decision is read from later.**
- `Sequencing` when another open task depends on this one, or it on another.
- `Delivery` — one line, and it is a **stance, not a forecast**: *one merge
  request until evidence says otherwise*. Do not estimate how many merge
  requests the work will need. That number is not knowable at planning time, it
  arrives with the work, and a wrong estimate written down early is worse than
  none because the split is then measured against it.
- `Model and effort` — close the plan with it, see below.

### Model and effort

End `plan.md` with a recommended model, a reasoning effort, and one or two lines
of justification drawn from three axes: how mechanical the work is, how much
ambiguity survives the plan, and how much a mistake would cost.

- Bounded, mechanical, fully specified → the cheaper model, lower effort.
- Ordinary feature or defect work → the middle tier, medium to high effort.
- Cross-module migration, security-sensitive change, or work where the plan
  itself is uncertain → the strongest model, high effort.
- Recommend separately for **planning** and for **implementing** when they
  differ, and name the condition that should escalate implementation back up —
  usually "execution exposed ambiguity the plan did not have".

Two constraints, and both matter more than the recommendation itself:

- **Name identifiers that exist in the runtime actually executing this task.**
  Read them from the runtime; a remembered "latest" alias is wrong more often
  than it is right, and a plan naming a model that does not exist is worse than
  a plan naming none.
- **This is planning metadata, not an action.** Do not switch the active model
  as a side effect of writing it down. The developer changes runtimes and
  models themselves.

Revisit the recommendation when execution contradicts it, and record the change
with its reason — a plan whose model line was never revisited after the work
turned out to be twice as ambiguous taught the next task nothing.

## `progress.md`

The handover surface, written for a session that has none of the current
conversation. It is the **only** file a resuming session is required to read;
everything else it points at.

Exactly five headings, and never a sixth:

```text
## State   ## Phases   ## Verification   ## Next action   ## Traps
```

**Never add a dated section.** `## Status (2026-09-04, later)` is how this file
turns into a journal, and a journal cannot be trusted to say where the work
stands — the reader has to diff four of them to find out.

**It is overwritten, not appended.** A new session replaces `State`, `Phases`,
`Verification` and `Next action` with what is true now. Nothing is kept for
context: narrative goes to `journal/`, defects to `findings.md`, settled
questions to `decisions.md`. A file with two `Next action` sections has already
failed at its one job.

### `State` is machine-verifiable, and that is what makes overwriting safe

Appending is a hedge: the file may be stale, but the older truth is still in it.
Overwriting removes that hedge, so the current state must instead be *checkable*
in one command. Every field is something git can confirm:

```text
work: branch feature/DCT-59416-viewmodel-paging @ 96758bed02 · pushed: no · tree: clean
phase: 3 of 5
```

`work:` records where the work physically is, because git alone cannot say which
task a change belongs to. One of:

```text
work: branch <name> @ <sha> · pushed: <yes|no> · tree: <clean|dirty>
work: stash@{0} "<KEY> : <what>" · base <branch> @ <sha>
work: uncommitted on <branch> · no branch cut yet
work: worktree <path> @ <branch>
```

The workspace is the source of truth for `work:`; git is the verifier, never the
other way round. Branches get renamed, stashes get popped, backup branches
multiply — the file records the intent, and the resuming session checks it.

Where a stash holds the work, write the stash message as `<KEY> : <what>` so
`git stash list` resolves to a task on its own.

### The 120-line cap, and what a breach means

`progress.md` stays under 120 lines. When it will not fit, ask **what is
overflowing** — the answer picks the fix, and getting this backwards makes the
skill propose restructuring at every long file:

- **The journal is overflowing** — dated narrative, superseded verification
  numbers, git operation history. Trim it into `journal/`. This is the usual
  case.
- **The state itself is overflowing** — the `Phases` table alone does not fit,
  because the task is tracking several merge requests. That is structural: read
  *When a task becomes an epic*.

`Traps` is the one section that accumulates, and it accumulates slowly: one line
per trap, for things a resuming session would otherwise rediscover the hard way
— inherited breakage, approaches already ruled out and why.

Update the file whenever the answer to "where does this stand" changes — a phase
finishes, a decision lands, commits are made, a verification result moves — and
before ending a session with work still open. **A stale `progress.md` is worse
than none, because the next session trusts it.**

## `journal/`

Append-only, and the counterweight that lets `progress.md` be overwritten
without losing history. One file per month, `journal/YYYY-MM.md`, newest entry
at the bottom.

An entry is **three to five lines**: the date, what changed, the resulting sha
or artifact, and nothing else. It is a record, not a report.

What does **not** go here, because it is already recorded somewhere better:

- Git operation narrative — rebases, backups, squashes, which branch was cut
  from which. `git log`, `git reflog` and the backup branches themselves already
  hold it, and hold it more accurately.
- Verification results that a later commit has already invalidated. Only the
  current numbers matter, and they live in `progress.md`'s `Verification`.
- Anything a later entry supersedes. Correct the record; do not stack a
  correction on top of it.

**The journal is never read on resume.** It is read when someone asks when or
why something happened, and then usually only its last entry.

## `questions.md` and `decisions.md`

Two files because they have opposite lifetimes: an open question is read on
every resume, an answered one almost never.

`questions.md` holds **only open questions**. Write one when the answer changes
the work *and* cannot be settled from the repository's rules, its documentation,
or its code. Routine judgment calls are made in the plan under a stated
assumption instead — a question file full of things the agent could have decided
is a way of not working.

One block per question:

```markdown
## 1. <question>

**Status:** Open

**Context:** why this is undecided and what it blocks.

**Options:** A — … / B — … (with the agent's recommendation and its reason)

**Answer:**
```

Agent side: leave `Answer` empty, never guess it, never edit a developer's
answer. Set `task.md` to `Status: Awaiting answers`, report the file path, and
**keep working on every part of the plan the answer does not block**.

Developer side: answer in place under each question, then say the answers are
in. An unanswered question stays `Open` and its part of the work stays untouched.

When the developer reports back, re-read the whole file, then for each answered
question: copy what it decided into `plan.md`'s `Decisions` if it changed the
approach, append one line to `decisions.md`, and remove the block from
`questions.md`. **Moved, never deleted** — the decision survives in a form that
costs one line to carry:

```markdown
- **D12 · 2026-09-04 — picker view model gets `SavedStateHandle`.** Project
  convention; this module was the outlier. (was question 12)
```

## `findings.md`

Only when a review pass or a device pass produces a defect list. One line per
finding, with a **stable id** and a state:

```markdown
- **D12 · open · HIGH** — currency popup row width wrong against `item_currency.xml`.
- **D13 · fixed 2026-09-02 · LOW** — balance badge should animate in.
```

The id is assigned once and never reused. A finding that reappears in a later
pass **reopens its existing id** — it does not get a new one. Renumbering the
same defect on every pass is how one issue becomes three entries that no one can
reconcile.

## `merge_request.md`

Only when enabled, and it describes **one** merge request — the one this task
delivers. Sections `Added:` / `Changed:` / `Fixed:` / `Removed:`, only the ones
that apply, each a bullet list where every item is one short line, no wrapped
sentences.

A change a reviewer sees at a glance in the diff (a mechanical rename, a type
swap rippling through call sites, a parameter-count change) gets a word or a
phrase, never a bullet per touched file. Spend the fuller sentence only on what
the diff alone will not tell a reviewer: a behavior change, an added or removed
case, a visibility change with a consumer-facing effect.

Write it once the change set is settled, before opening the request.

**One file may hold two requests, and only in one case**: a single ticket on a
single branch split into two for review size. Then two headings in this file are
correct. Several requests with *different ticket keys* are not that case — they
are an epic, and each belongs to its own task directory. A file accumulating
request after request is the same failure as a `progress.md` accumulating status
sections: no rule said when the unit ended.

The rationale that spans requests — why the split is shaped this way, what order
they must merge in, which trade-offs the developer accepted — is not a request
description. It belongs in `epic.md`.

## Subtasks

Rare, and smaller than they sound. A subtask is a bounded unit of work *inside
one merge request*. It is not a merge request, and never carries a ticket key.

`subtasks/NN-<slug>/subtask.md`, one file, about 40 lines: `Status`,
`Depends on`, `Scope`, `Validation`, `Result`. When one is dropped, keep the file
and head it with why and when — a superseded subtask explains a gap in the
numbering that would otherwise be re-investigated.

A subtask never owns a branch, a merge request, or a `progress.md` of its own.
Its state is one row in the task's `Phases` table. A second progress file is a
second answer to "where does this stand", and the two will disagree.

If subtasks start acquiring ticket keys or branches of their own, they are merge
requests wearing the wrong name — read the next section.

## When a task becomes an epic

A task delivers one merge request. When it starts delivering several, each with
its own ticket key, the unit is no longer the task — it is the merge request.

```text
<scratch>/tasks/<EPIC-KEY or EPIC-local>-<slug>/
  epic.md            # the umbrella goal, the split, and its rationale
  progress.md        # the routing table: one row per request — key, branch, state
  decisions.md       # decisions that span requests
  findings.md
  journal/YYYY-MM.md
  tasks/<TICKET>-<slug>/     # each request is an ordinary task directory
```

Shared material lives at the epic; each request gets a plain task directory with
its own `progress.md`, its own single `merge_request.md`, and a `prev:` pointer.
Use the tracker's epic key when there is one, `EPIC-local-<slug>` when there is
not.

### Fire at the boundary, not at a threshold

**Do not predict the split at planning time.** It is not knowable then: keys
arrive late, and a split is typically rebuilt at least once before it settles.

Instead, promote at the next natural boundary — a new ticket key arrives, a new
branch is cut, a new request is about to start. This matters because the cost of
promoting is not constant. At the boundary it is nearly free: nothing that
exists has to move, only the *new* unit is placed differently. After six merged
requests it is a rewrite of everything already written.

The thresholds only tell you a boundary is worth using, and are counted from the
directory itself:

```sh
grep -rhoE '\b[A-Z]{2,}-[0-9]+\b' <task>/ | sort -u     # two or more distinct keys
```

Two or more distinct ticket keys is on its own enough — a second real key in one
task directory already contradicts one task, one request. Everything else (two
non-backup branches, a `Phases` table that will not fit the cap, more than about
six subtasks) is a soft signal: worth acting on at the next boundary, not worth
acting on alone.

Guard against splitting too eagerly: a second key whose scope does not earn its
own plan — a ten-line follow-up — stays in the task it belongs to.

### Never migrate backwards

Promotion places new units. It does **not** move existing files. The current
directory becomes the epic root exactly as it stands; the next request is born
in `tasks/<KEY>-<slug>/`. Paths that already exist keep working, and accepting
the promotion costs seconds rather than an afternoon — which is the only reason
it will actually get accepted.

### The agent splits; it does not ask

The workspace is git-ignored, local, and a wrong placement is one `mv` away from
undone. Placing a new directory is a routine judgment call, not a
restructuring — make it, then **say what was done in one line** and record it in
`progress.md` and the register. A notification, not a question: the developer can
still reverse it, without a blocking round trip.

Approval is required for exactly one thing: **moving files that already exist**.
That changes paths under the developer's own editor and muscle memory, so it is
proposed with the counted evidence and waits. If it is declined, record the
decline and the numbers it was declined at — and ask again only when the evidence
materially worsens, never on the same numbers.

### The chain is the invariant

What makes free-hand splitting safe is not tidy placement, it is reachability. A
resuming session reads the newest surface and steps backwards only as far as the
question needs, so every unit names the one before it:

```markdown
epic: balance-dynamics-compose
prev: DCT-59362-transaction-list-search
```

So the rule that replaces the approval gate:

> **A new unit directory is not created without its `prev` pointer and its
> register line, written in the same breath.** A misplaced unit is still
> reachable; an unlinked one is orphaned, findable only by scanning
> modification times.

## Resuming

"Continue that task" has to resolve to one directory without guessing. Work down
this ladder; each step confirms the one before rather than replacing it:

1. **The developer named it.** Match it in the register and stop.
2. **Recency.** The most recently modified task directory is the candidate:
   ```sh
   ls -dt <scratch>/tasks/*/ | head -3
   ```
   This is the primary signal because it is the only one that survives every
   case — no branch cut yet, work stashed, detached HEAD, a separate worktree, a
   scratch branch whose name carries no key.
3. **Verify against git.** Read that candidate's `progress.md` `State` and check
   it: `git branch --show-current`, `git log -1`, `git status --porcelain`,
   `git stash list`. If `work:` matches, resume. **If it does not, do not
   proceed** — recency alone will silently resume the wrong task, and this step
   is the only thing that catches it.
4. **Ask.** Offer the top two candidates from the register. With a register
   capped at two lines per task, asking is cheap; resuming the wrong task is not.

Branch names are a verification signal, never the primary key: backup and scratch
branches outnumber real ones, several branches can belong to one task, and a
branch gets renamed the moment its ticket key arrives.

Once resolved, read `progress.md` and nothing else by default. Open `plan.md`
when the next action needs its detail, `task.md` when scope is in question,
`decisions.md` when something settled is being reopened, and the last journal
entry only when step 3 left something unexplained. Do not report a task's state
from memory when its progress file says otherwise — correct the file first.

## The register

When more than one task can be open, `tasks/` gets a register (the filename is
in the config table; `INDEX.md` and `README.md` are the usual choices) with an
`## Open` and a `## Closed` section.

**Two lines per task, and it is a pointer, not a report.** It carries the
directory, a one-phrase goal, the ticket keys it covers, and what it is waiting
on. Anything longer is progress duplicated into the file every session opens
first — the most expensive place in the workspace to put narrative.

```markdown
- `DCT-59416-viewmodel-paging` — screen contract and paging for balance dynamics
  keys: DCT-59416 · next: chart assembly (MR 6, unkeyed)
```

The keys are what let step 1 of the resume ladder match a branch name to a task
without reading anything else.

- A task joins `Open` when its directory is created.
- **Only the developer moves a task to `Closed`**, with the date. Never do it
  from your own reading of the work.
- It is the first file a session opens when it does not already know which task
  it is on, and the last file it updates when a task's state changes.
- If it disagrees with the directories on disk, reconcile it first and say what
  changed.

## Multiple open tasks

A dependency between two open tasks is recorded on **both** sides — the blocked
one in its `progress.md`, the blocking one in its `plan.md` `Sequencing`. A file
that two open tasks both edit is named in both plans. A task written before its
sibling existed gets the back-reference added when the sibling is created, not
when the collision is discovered.

## Rules

- Everything here is git-ignored. Task material is never committed and never
  cited as the source of a repository rule. A rule that turns out to be durable
  belongs in the repository's agent rules; durable documentation belongs in
  `docs/`.
- One task, one merge request, one directory. A follow-up is a new directory
  that links back with `prev:`.
- Findings that outlive the task — a mechanism traced, a library behavior
  root-caused — are not task state. They belong wherever the project keeps
  durable notes, and are moved there before the task closes rather than lost
  with it.
- The task directory holds the task's own files, not every by-product. Audits,
  logs, and generated reports follow the scratch workspace's own routing.
- Do not begin implementation while a material question is unanswered.
