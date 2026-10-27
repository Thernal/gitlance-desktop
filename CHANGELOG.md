# Changelog

What is new in each release of GitLance. A release's notes on GitHub are its section here, then its commits
(`.github/release-notes.sh`); a release tag whose version has no section here does not build. Write what changed
for someone using the app, not for someone reading the code; the commits follow anyway.

## Unreleased

- **The background fetch runs from the start.** A repository just opened showed Fetch in the background off and
  fetched nothing until it loaded again; it is on from the first moment now, as it has been meant to be since 1.0.1.
- **A merge request shows its own commits.** It was compared from its target branch as this clone last fetched it,
  so a request rebased onto newer commits showed every target commit since as its own, and one merged by a
  fast-forward showed no changes at all. It is now compared from the base and head GitLab or GitHub gives it; a
  commit of them that is not here yet is fetched once.
- **A comparison stays as you left it.** Its Swap, Direct and Use origin/… were undone when you came back to it
  from another space, and its side panel listed the commits and versions of the branch or merge request opened
  before it. It now comes back as it was, with only its own changes beside it; the working tree too.
- **The menu scrolls.** With many merge requests or branches open under it, the rows that did not fit were cut off
  at the window's bottom; the menu now scrolls, and brings the page on screen into view when you go to it.
- **v opens the file it moves to.** A generated or lock file it went to stayed folded, and the file it marked
  opened again when you had folded it; the next file now opens at its first lines, and the one marked stays as
  you left it unless View → Fold reviewed files folds it.
- **Send one pending comment now.** A comment kept for the review (⇧⌘↵) can go on its own, the others still
  pending: Send this now beside its Delete, and on its row in Discussions and Submit review. On GitLab; GitHub
  sends pending comments only with the review.
- **An agent can open GitLance.** Its MCP server's new `open_gitlance` starts GitLance on the agent's repository,
  or opens the repository in a tab behind the one you are on when GitLance runs without it, with a note saying
  which agent did. Agents are told to do it only when their work needs you to look.
- **An agent can review a merge request with you.** Through GitLance's MCP server it reads the request as GitLab
  or GitHub compares it and leaves pending comments on its lines, with GitLance's own token; a line not in the
  diff is refused. They show at once, marked with the agent's name, and a note offers Review, which opens the
  first. They stay pending, only yours to send, delete or submit.
- **An agent can gather the merge requests it reviewed.** They come in quietly under Merge requests, in its
  order, each with the agent's mark, how many of its comments wait there and its line on what it did; opened
  from there, a request starts at the agent's first comment.
- **Comparisons and the working tree stay in the menu.** One you open with ⌥⌘C, or the working tree, becomes a
  row under Branches as a branch does, kept until you close it (× or ⌘W); Swap and Direct keep the same row. An
  agent can open its changes there too, as a comparison or the working tree, with a note offering Show.
- **What you change on a merge request shows at once.** A comment, a reply, an edit, a deletion, a resolve, a
  pending comment, Send this now, an approval or a submitted review used to read the whole request again, its
  discussions, pipeline and Overview going back to loading and the page jumping. It now shows where you made it,
  faded and saying what it waits for, and becomes the real one when GitLab or GitHub answers; nothing else
  reloads. If the host refuses, it is taken back and says why, your text back in its field. The Inbox's reply,
  resolve and Mark done do the same.

## 1.1.0 — 2026-10-26

- **A newer release is announced.** GitLance asks GitHub for its releases when it starts and every 6 hours, and
  when one is out, a card says what is new in it and in every release in between, with its commits. Download
  opens the release page; Skip this version keeps quiet until the next one. After that, a mark in the title bar
  opens the card again. Settings → General turns it off; GitLance → Check for Updates… asks right away. Nothing
  is downloaded or installed by itself.
- Every release now says what is new in it, followed by its commits by kind.
- **⌥↓ goes on down the list.** The next merge request is the first one below the open one that is not reviewed
  and not yours, counted from where the open one was — not from Reviewed, where reviewing it to the end moved it;
  with none below, it starts again at the top, once round.
- A request's Overview lists its reviewers beside the description, and nothing else: the discussions are in the
  Discussions window on the right.

## 1.0.1 — 2026-10-24

- **The background fetch is on by default.** Every repository with a remote is fetched in the background, so one
  opened for the first time sees other people's pushes and force pushes as versions. Settings → Repository turns
  it off for one; repositories turned on before stay on. A plain folder or a repository without a remote does not
  fetch.

## 1.0.0 — 2026-10-24

GitLance 1.0 is a new window, built around what a review is made of.

- **Four spaces** in the menu on the left — Inbox, Code, Branches and Merge requests (⌃⌘1…4). Each keeps the page
  it was left on, with a bar over it; esc goes one page up, ⌘[ ⌘] go back and forward across spaces.
- **Inbox**: mentions, review requests, replies and discussions resolved by someone else, and what your agents
  answered — each answerable in place.
- **Code**: the project's tree at the working tree, `HEAD`, a branch or a commit; files and folders open as tabs;
  a note for your agent on a line, a file or a folder. A folder that is not a git repository opens too.
- **Merge requests** grouped by what each needs from you; a request as Overview · Commits · Changes; merged and
  closed ones too; what changed since the version you viewed. A branch off the main line is a page of its own.
- **GitHub pull requests**, at parity with GitLab merge requests.
- **Creating a merge request** in place of the diff: the target's template, reviewers from CODEOWNERS, labels,
  milestone — or the quick way from ⌘K.
- **Reviewing**: every file of a diff in one scroll, reviewed marks that move on, zen with one file at a time at a
  reading width, regular expressions in find, a changed Markdown file read as a page.
- **Discussions**: mentions with people's names and pictures, reactions, editing and deleting your own notes,
  completion of people and emoji, an outdated comment followed line by line across versions.
- **Agents**: several chats at once, each in its own worktree; a note goes to the chat on its branch; the agent can
  ask back with a form; a file another chat is changing says so. Codex chats are listened to too (limited).
- **Look and feel**: five themes and your own, with an editor that checks contrast; tree-sitter highlighting for
  the main languages; IntelliJ's file icons; motion; narrow windows that fold; key presets — GitLance, Android
  Studio, VS Code.
- A welcome window that connects GitLab or GitHub and clones from a URL; a project's own name and colour.
- **Report a problem** from the app (⌥⌘/).

## 0.2.0 — 2026-10-14

- **Comments for your agent go to its chat and come back answered.** GitLance is an MCP server for Claude Code and
  Codex: send the purple comments to a chat, the agent changes the code and answers each one, and you resolve or
  reopen them in GitLance. Settings → Agents connects Claude Code.
- On Windows and Linux the shortcuts use Ctrl, not the Windows key.

## 0.1.0 — 2026-10-14

The first release.

- Browse a repository's branches and commits, the working tree against `HEAD`, and compare any two revisions.
- **Versions of a rewritten branch**: its earlier versions from the reflogs, any two of them compared — a plain
  diff on the same base, the two patches on a moved one.
- Diff options: split or unified; lines, words or structural (difftastic); wrap, all lines, hide whitespace;
  changed images shown as images.
- Review aids: reviewed marks per file, find in the diff and in all of its files, who wrote each line, the
  functions a file changes, bookmarks, recent places, generated files folded, zen, the keyboard card.
- **GitLab merge requests**: discussions on the lines, replies, resolving, pending comments, approving and
  submitting a review, the pipeline, versions, creating one; GitLab links open in the app; a bell for mentions,
  review requests and replies.
- Comments for an agent, copied as markdown.
- Repository tabs, a command palette, rebindable keys, ⌘+ ⌘− ⌘0 for the whole interface.
- Builds for macOS (universal), Windows and Linux.
