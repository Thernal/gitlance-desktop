# File skeletons

The minimum structure for each file. Sections that do not apply are removed, not
left empty — an empty heading reads as an unanswered question.

## `task.md`

```markdown
# <TICKET or date> — <goal in a phrase>

**Status:** Planning
**Epic:** <epic directory, or omit when this task stands alone>
**Prev:** <the task this one follows, or omit for the first>

## Request

<what the developer asked, in your own words, with anything implied made
explicit. Where a tracker supplies the description, paste it verbatim under a
"Source" subheading and keep your restatement separate.>

## Goal

<the end state, one or two sentences>

## Scope

**In:** <…>

**Out:** <what this task deliberately does not do>

## Done when

- [ ] <command or check that proves it>
- [ ] <…>
```

## `plan.md`

```markdown
# Plan — <goal>

## Phase 1 — <name>

**Changes:** <what>
**Why:** <why this before the rest>
**Files:** <paths>
**Verified by:** <command or check>

## Phase 2 — <name>

…

## Risk

<what could break, and what would signal it>

## Rollback

<how to undo this if it lands badly>

## Sequencing

<only when another open task depends on this one, or it on another>

## Delivery

One merge request until evidence says otherwise.

## Decisions

- **YYYY-MM-DD — <question>:** <the answer, and what it changes in this plan.>

## Model and effort

- **Planning:** <model available in this runtime>, <effort> — <why>
- **Implementing:** <model>, <effort> — <why>
- **Escalate if:** <the condition that should send implementation back up>
```

The model names come from the runtime actually executing the task, not from
memory. This section is metadata; it does not switch the active model.

`Delivery` is a stance, not a forecast. It is not an estimate of how many merge
requests the work will need — leave that line exactly as it is until the work
itself contradicts it.

## `progress.md`

Five headings, no sixth, none of them dated. Overwritten in place, capped at
120 lines.

```markdown
# Progress

**Last updated:** YYYY-MM-DD
**Plan:** `plan.md` · **Open questions:** `questions.md` · **History:** `journal/`

## State

work: branch <name> @ <sha> · pushed: <yes|no> · tree: <clean|dirty>
phase: <n> of <total>

## Phases

| Phase | State | Note |
|---|---|---|
| 1 — <name> | done | |
| 2 — <name> | in progress | <where exactly> |
| 3 — <name> | not started | blocked on question 2 |

## Verification

| Check (from `Done when`) | Reports today |
|---|---|
| `<command>` | <actual result, not the expected one> |

## Next action

<the single thing the next session picks up>

## Traps

- <inherited breakage, an approach already ruled out and why — one line each>
```

`work:` takes whichever form matches where the work physically is:

```text
work: branch <name> @ <sha> · pushed: <yes|no> · tree: <clean|dirty>
work: stash@{0} "<KEY> : <what>" · base <branch> @ <sha>
work: uncommitted on <branch> · no branch cut yet
work: worktree <path> @ <branch>
```

## `journal/YYYY-MM.md`

Append-only, newest at the bottom, three to five lines per entry.

```markdown
## YYYY-MM-DD — <what happened, in a phrase>

<what changed and why, in a sentence or two.> <resulting sha or artifact.>
```

No git operation narrative, no verification numbers a later commit invalidated,
no correction stacked on the entry it corrects.

## `questions.md`

Open questions only. An answered one moves to `decisions.md`.

```markdown
# Open questions

## 1. <question>

**Status:** Open

**Context:** <why this is undecided, and what it blocks>

**Options:**
- **A —** <…>
- **B —** <…>

**Recommendation:** <A or B, and why>

**Answer:**
```

## `decisions.md`

One line per settled decision, in the order they settled.

```markdown
# Decisions

- **D<n> · YYYY-MM-DD — <what was decided>.** <the reason, in a clause.>
  (was question <n>)
```

## `findings.md`

Only when a review or device pass produces a defect list. Ids are assigned once
and never reused; a finding that reappears reopens its existing id.

```markdown
# Findings

- **D<n> · open · <HIGH|MEDIUM|LOW>** — <the defect, in one line>
- **D<n> · fixed YYYY-MM-DD · <severity>** — <the defect, in one line>
```

## `merge_request.md`

One merge request. Two headings only when a single ticket on a single branch is
split for review size.

```markdown
# <MR title>

Added:
- <one short line per item>

Changed:
- <…>

Fixed:
- <…>

Removed:
- <…>
```

## `subtasks/NN-<slug>/subtask.md`

A bounded unit of work inside one merge request. Never a ticket, never a branch.

```markdown
# Subtask NN — <name>

- **Status:** proposed | in progress | done | superseded
- **Depends on:** <NN, or none>
- **Estimated diff / actual:** <lines>

## Scope

<what changes, and what it deliberately leaves alone>

## Validation

<the command or check that proves it>

## Result

<what actually happened, once it is done>
```

A superseded subtask keeps its file and gains a note at the top saying when and
why it was dropped — the gap in the numbering is otherwise re-investigated.

## `epic.md`

Only when a task has been promoted. The rationale that spans merge requests.

```markdown
# <epic name>

## Goal

<the end state the whole series delivers>

## The split

| # | Key | Task directory | Delivers |
|---|---|---|---|
| 1 | <KEY> | `tasks/<KEY>-<slug>/` | <one phrase> |
| 2 | <KEY or pending> | … | … |

## Why this shape

<the merge order and whether it is strict; trade-offs the developer accepted;
dependencies that carry forward between requests>
```

## `INDEX.md` (register)

Two lines per task. A pointer, not a report.

```markdown
# Tasks

## Open

- `<dir>` — <one-phrase goal>
  keys: <TICKET, …> · next: <what it waits on>

## Closed

- `<dir>` — <one-phrase goal> — closed YYYY-MM-DD
```
