---
name: knowledge-lookup
description: Consults the user's personal knowledge corpus (github.com/Thernal/knowledge) — notes they already read and synthesized, and their kits (own reusable code repos such as nav-kit) — from any project. Use it BEFORE an architecture, layering, modularization, auth or mobile-infrastructure decision, so the choice matches what the user already concluded; BEFORE building something they may already have built (navigation, design system, auth plumbing) — it reads a kit's code, docs and history live; and when they ask what they read, saved or decided ("what did we decide about X", "do I have anything on Y", "daha önce ne karar vermiştik", "nav-kit bunu necə edir"), or where a claim came from. One round trip over SSH, no token. Not for adding a source to the corpus, installing or updating skills and kits (skill-manager), or questions the user's reading has no bearing on.
---

# Knowledge lookup

Reads a personal knowledge corpus from any project. Every command is a git
operation against a repository the machine already has SSH access to.

```
sh <this-skill-dir>/knowledgectl.sh <command>
```

**Dependencies: `git` and a POSIX shell. That is the whole list.** No runtime, no
token, no API — git is already required to reach the corpus at all.

## Placement

**This skill writes nothing into the project being worked in.** Its only artifact
is a read cache outside every repository:

| Kind | Default root | Visibility | Ownership |
|---|---|---|---|
| Corpus read cache | `${XDG_CACHE_HOME:-~/.cache}/knowledge-corpus/` | local | foreign — the corpus repo, pinned to a commit |
| Kit mirrors | `${XDG_CACHE_HOME:-~/.cache}/knowledge-corpus/kits/` | local | foreign — each kit repo, at its HEAD |

The cache is one copy per machine rather than per project, because the corpus is
the same for all of them. Override with `KNOWLEDGE_CACHE`; point at a different
corpus with `KNOWLEDGE_REPO`. Nothing is committed anywhere, so there is no
placement row for a consuming project to override.

## What the corpus holds, and which one to ask

Three layers, and picking the wrong one is the usual waste:

| Want | Ask | Costs |
|---|---|---|
| Does the corpus cover this subject? | `search <terms>` | ~100 tokens |
| What has the corpus *concluded*? | `threads`, then `thread <slug>` | ~400 tokens |
| The reasoning and the specifics | `get <slug> --section takeaways` | ~300 tokens |
| The whole argument | `get <slug>` | ~1300 tokens |

A **topic** is a subject a note is about. A **thread** is a claim the corpus
itself holds — something no single source said, supported by several notes from
authors who never met, with its qualifications recorded. For a broad design
question ("design a navigation architecture we haven't tried"), threads are the
answer and a pile of note summaries is not.

## Commands

| Want | Command |
|---|---|
| Find anything on a subject | `knowledgectl.sh search <terms...>` |
| The subject vocabulary | `knowledgectl.sh topics [<term>]` |
| What the corpus concluded | `knowledgectl.sh threads` |
| One claim, with evidence | `knowledgectl.sh thread <slug>` |
| One note, or one section | `knowledgectl.sh get <slug> [--section <name>]` |
| Where a claim came from | `knowledgectl.sh cite <slug>` |
| The user's reusable kits | `knowledgectl.sh kits` |
| One kit: what, when, when not | `knowledgectl.sh kit <name>` |
| A kit's file, live | `knowledgectl.sh kit <name> read <path>` |
| Its files · commits · a search | `kit <name> tree [<path>]` · `log [<n>] [<path>]` · `grep <pattern>` |
| Cache state / force refresh | `knowledgectl.sh status` · `sync` |

`search` matches topic **aliases** as well as names, so a near-miss word still
hits — searching "MVI" reaches the state-management topic even with no note named
for it.

## Kits: the user's own code

`search` also lists **kits** — repositories of the user's reusable code that apps
copy (nav-kit: Compose Multiplatform navigation). A kit hit means *they already
built this*: before designing navigation, guards or deep links from scratch in a
project that fits the kit, say that the kit exists and what its card says it is
for and not for. Taking it into the project is skill-manager's `kit install`, not
this skill's.

`kit <name> read|tree|log|grep` reads the kit repository itself, live — the
answer to "how does nav-kit do X" is in its `navigation/api/README.md` and its
code, and the reason is usually in `log`. Only what is read is transferred.

## Prefer `--section`

A whole note is roughly 1300 tokens and most of it is the argument for a
conclusion you can read in 300. Sections are addressed by heading slug
(`takeaways`, `key-idea`, `example`, `connections`, `source`) or by an explicit
anchor id that the note declares. Read the whole note only when the reasoning is
what you actually need — usually when the corpus disagrees with what you were
about to do.

## Freshness, and working offline

Every command first asks the remote for its default-branch commit — one
`git ls-remote`, no clone. If it matches the cached copy, nothing is fetched and
the command is local. This is deliberately *not* the per-skill version-ref scheme
`skill-manager` uses: a corpus only needs "is my copy current", which `HEAD`
answers with nothing to publish, and therefore nothing anyone can forget to
publish.

When the remote is unreachable and a cache exists, the command answers from cache
and says so on stderr. It never silently serves a stale answer, and it never
fails a question it could still answer.

The fetch is shallow, blobless and sparse, restricted to `notes/` and `kits/`. The corpus's
`raw/` directory is its audit trail of full source captures and is never needed
to answer a question, so it is never transferred — which is what keeps the cost
flat as the corpus grows.

## When the corpus has nothing

Say so and carry on with the task. A miss means either the corpus genuinely does
not cover this, or it uses different words for it — and the second case is worth
reporting to the corpus so the wording becomes an alias. Do not treat a miss as a
reason to stop, and do not pad an answer with corpus material that only loosely
matches; the value of this corpus is that its claims were actually read and
judged, and diluting them with near-misses spends that.

## Citing it

Notes are syntheses, not sources. When a claim from here ends up in a commit
message, a PR description or a document, cite the original with
`knowledgectl.sh cite <slug>` — the note names the publisher URL, and merged
notes list every source they absorbed.
