# Detection pass

Run before asking the developer anything. Each block gives the command, and what
to conclude from its output. Where a block cannot conclude, that setting becomes
an interview question — not a guess.

Everything here is read-only.

## Existing agent guidance

```sh
ls -a | grep -iE '^(AGENTS|CLAUDE)\.md$|^\.(agents|claude|codex|cursor|github)$'
find . -maxdepth 3 -name 'AGENTS.md' -o -maxdepth 3 -name 'CLAUDE.md' | grep -v node_modules
```

Anything found is read in full before proposing a tier. An existing entry point
is extended, never replaced. If it already routes to a hub, this run is a
*reconfiguration* — go to the skill's "Reconfiguring" section.

## Scratch root

```sh
git check-ignore -v .misc 2>/dev/null || echo "NOT ignored"
```

- ignored → scratch root is `.misc/`, no question needed.
- not ignored → question 1. To offer the fallback list, show what git already
  ignores at the top level:

```sh
git status --porcelain --ignored=matching -- . | awk '$1=="!!"{print $2}' | \
  awk -F/ '{print $1}' | sort -u
```

Prefer a directory that is ignored *and* already exists *and* is not a build
output directory that tooling wipes — an IDE settings directory (`.idea/`,
`.vscode/`) is the usual survivor. Nesting under a build output is not an
option; say why if the developer suggests one.

## Task naming and remote tracker

```sh
git log --format='%s' -n 100
git branch -r --format='%(refname:short)' | head -50
ls githooks/ .githooks/ 2>/dev/null
grep -rhoE '\[A-Z\]\{[0-9],\}-\[0-9\]' githooks/* .githooks/* 2>/dev/null
```

- A ticket-key shape (`ABC-123`) recurring in branch names or commit subjects
  means a tracker is in use. Record the shape as a regex.
- A `commit-msg` hook enforcing that shape is decisive — it is not a
  convention, it is a constraint, and the generated commit rule must state it.
- No key anywhere → propose `YYYY-MM-DD-<slug>` naming, and still ask question 2
  in case the tracker exists but is not reflected in git.

## Commit subject convention

*Owned by the `git-delivery` skill when it is installed — run this block only to
fill the confirmation list, and let that skill write the rows.*


From the same `git log` output, classify the last 50 subjects:

- `type:` or `type(scope):` prefixes → Conventional Commits.
- `<KEY> : text` or `<KEY>: text` → ticket-prefixed.
- neither dominant → no enforced convention; the generated rule says "match
  surrounding history" rather than inventing one.

Report the count ("47 of 50"), not just the label — a 30/50 split is worth the
developer's attention.

## Protected and generated paths

```sh
cat .gitignore 2>/dev/null
ls -d */build build 2>/dev/null
```

Take the build-output and machine-owned entries (`build/`, `.gradle/`, `dist/`,
`target/`, `node_modules/`, `.idea/`, `local.properties`, lockfiles owned by
tooling). Ignore entries that are merely secret files — those belong to a
security rule, not the protected-files rule. Anything ambiguous goes to the
developer as one line in the confirmation list.

## Second agent runtime

```sh
ls -d .agents .codex .cursor .gemini 2>/dev/null
ls -d .agents/skills .claude/skills 2>/dev/null
```

Real directories on both sides for the same skill name mean a mirror already
exists — flag it: the two-way symlink model replaces a hand-maintained mirror,
and reconciling them is a decision, not an automatic conversion.

## Skills and commands the repository already has

```sh
ls -d .claude/skills/*/ .agents/skills/*/ .claude/commands/*.md 2>/dev/null
head -4 .claude/skills/*/SKILL.md 2>/dev/null
```

Read each one's `description` frontmatter — that is the trigger it already
advertises — and the first section of its body. This is not bookkeeping: the
router has to carry a row for each of them, and any hub section covering the
same ground has to be cut rather than written (`generation.md`, the
`session-rules.md` spec).

Getting this wrong during a *rebuild* is worse than during setup. A repository
that already routes to its own skills loses those rows if generation only
accounts for the files it just wrote, and the skills go quietly unreachable
from the hub.

## Review culture

```sh
git remote -v
ls -d .github/pull_request_template.md .github/PULL_REQUEST_TEMPLATE* \
      .gitlab/merge_request_templates 2>/dev/null
```

A template directory is strong evidence that MR/PR descriptions are written by
hand and that `merge_request.md` in the task workspace will be used. No remote,
or a remote pushed to directly, is evidence against.

## Documentation language

Read the existing `README.md` and any `docs/`. Generated rule files follow the
language the repository already documents in; English is the default when the
repository has no documentation yet. This is independent of the language the
session is conducted in.

## Build and verification commands

```sh
ls Makefile justfile Taskfile.yml package.json build.gradle.kts pom.xml \
   Cargo.toml pyproject.toml 2>/dev/null
```

Whatever exists supplies the real verification commands for `repo-map.md`'s
"Common commands" section. **Read the file for the actual task/script names** —
do not assume `npm test` or `./gradlew test` exists because the manifest does.
A command that does not exist in the repository must not appear in a rule file.

## Instruction candidates

Instructions are the agent-facing middle tier: procedural detail for one class
of task, too long to load every session and too repository-specific to publish
as a skill. Detect *candidates*; do not create files.

```sh
ls docs/ docs/instructions 2>/dev/null
git log --format='%s' -n 300 | sort | uniq -c | sort -rn | head -20
find . -maxdepth 2 -name 'README.md' -not -path './node_modules/*' | head -20
```

A subsystem is a candidate when it shows up as its own recurring area in the
repository's structure or history — navigation, networking and error handling,
testing conventions, build conventions, the design system, persistence and
migrations, localization, release configuration. Propose only the ones this
repository actually has, one line each: what the instruction would cover, and
the evidence that suggested it.

Where instructions already exist, read them: they may already answer questions
that would otherwise be asked, and their location tells you whether the project
commits them or keeps them local.
