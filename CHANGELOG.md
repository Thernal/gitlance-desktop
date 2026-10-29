# Changelog

What is new in each release of GitLance. A release's notes on GitHub are its section here, then its commits
(`.github/release-notes.sh`); a release tag whose version has no section here does not build. Write what changed
for someone using the app, not for someone reading the code; the commits follow anyway.

## Unreleased

- **Pin a tab in Code.** Pin tab — in a tab's right click, ⌘K or ⌥⌘P — keeps a file or a folder first among the
  tabs, a pin where its × was and a line after the last one. A preview is never put in its place, Close others,
  Close to the right and Close all leave it, ⌘W leaves it too, and it is there again after a restart. A click on
  its pin unpins it. Close to the right is new in a tab's right click.
- **What is beside a folder, from Code's path.** A click on a folder's name in the path over a file lists what
  is beside it — folders first, then files, the one you came through saying "here" — and typing filters it. ↑ ↓
  and ↵ open a folder's page or a file's tab; the first row opens the folder itself, as the click used to.
- **A comparison's commits.** Comparing two branches, tags or commits lists the commits it holds over its
  files, newest first. A click shows one commit's own changes, a ⇧-click a run of them, ↑ ↓ step through them;
  "a41f9c0 only" says so over the diff, and esc or its × bring the whole comparison back.
- **Staged and not staged, apart.** The working tree's files are in two groups, Staged and Not staged, each under
  a header that folds it and says how many; the filter is over both. A file partly staged is in each with its
  own part's lines, and every file's card says which part it is. Not staged only shows that group alone.
- **Pictures in comments, kept.** An image in a comment or a merge request's description — a screenshot uploaded
  to GitLab, a picture from the web — is drawn under its words, and a click opens it; a document's web images and
  badges are drawn too, not only named. Each is fetched once and kept on disk with people's pictures, so it shows at
  once the next time, offline too; a private project's uploads are asked with your GitLab token.
- **A field keeps what you wrote, not the keyboard.** Click outside a comment, a filter or the find bar, press esc
  in one that holds text, or go to another file or page, and it keeps its text, dimmed, while the keys go back to
  the diff: V marks the file you are on, j and k move. Click in it, press `c` beside an unfinished comment, or come
  back to its page, and you go on typing. A file with an unfinished comment shows a pencil in the files list; a
  filter left with text clears with a second esc.
- **Links without the browser.** A merge request's right click — in the list, the Inbox, its row in the menu, its
  page — copies its link or its number; a branch's, a commit's and a comparison's copy their page on the host, a
  run of commits or two versions of a request their comparison there, and a file's in a request's diff its place
  there. ⌥⇧⌘C, or Copy link to this … in ⌘K, copies the page on screen's. A toast says what was copied; a branch
  or a commit not pushed yet says it has no link instead of copying a dead one.
- **Every cut text gives the whole of it.** A name, a title, a path or a line cut short shows all of it on a long
  hover, everywhere — only when it was cut.
- **The size of the whole change.** Over the files, a row says how many there are and their lines added and
  removed, in the column of each file's own; with a path typed, of the files shown. The review line says only
  the numbers: 4/8.
- **An agent's work leaves a dot.** What an agent puts for you — its changes opened, a merge request drafted,
  requests gathered, pending comments, notes — marks its row with an orange dot until you open it, and the
  Branches or Merge requests row while their rows are folded. A row it left comments on comes under Merge
  requests by itself.
- **The New merge request card is easier to correct.** From can be changed to another branch, keeping what is
  written; From's and Into's lists open under their field, over the form, and ↑ ↓ walk them (they moved the
  commits behind the card); esc closes the list, not the card. A list of labels or milestones keeps the wheel
  instead of scrolling the form with it. An agent's draft opens with its branch however you get there, not only
  from its toast.
- **An agent shows you its changes without being asked.** It opens them in GitLance, with a summary and a note on
  each file the diff alone does not explain, after a commit, after it makes or rewrites a branch (a squash, a
  rebase, a cherry-pick, a reworded commit), after it addresses your comments, and before it asks you to look or to
  approve — a local branch not pushed yet included. Settings → Agents → Connect also gives Claude Code a `gitlance`
  skill saying the same; press it again to add the skill to a profile connected before.
- **Every way in, easiest first.** A repository whose host GitLance has no token for lists the ways this computer
  offers, each with the account it signs in as and a button named for what it does: the GitLab or GitHub CLI's
  token (Use — nothing is asked, nothing copied), the one git keeps, one made with your SSH key (read-only under
  ▾), one copied from IntelliJ IDEA, Android Studio or another JetBrains IDE signed in to the host (Copy… — macOS
  asks once; an OAuth sign-in is not taken), or one made on the host's page and pasted. ↑ ↓ choose, ↵ connects,
  ⌘V pastes a token. A way found that cannot be used says why underneath — a token for cloning only, a key the
  host refused, a CLI signed in elsewhere. The way picked is remembered for the host; Settings → Merge requests
  lists the same ways, the one in use tagged, to change it.
- **A token found is no longer used without a word.** git's token and the GitHub CLI's were taken as they were
  found; they are now listed with whose they are, so a second account is not used by mistake. Once picked they
  are used as before.
- **The window never waits on the GitHub CLI.** Its token was asked for each time the window drew; it is read once
  a run, in the background, and again when GitHub refuses it.
- **A file marked reviewed stays marked.** Once 300 diffs had marks, marking a file in a new one could do nothing
  (V and its box alike) or wipe another one's marks; now the diff marked longest ago makes room, never the one
  being marked. A merge request's ✓ in the lists, and how far you got in it, no longer drop away the same way.
- **A pending reply sits under its discussion.** A reply written in review but not sent yet shows right under the
  discussion it answers, on that discussion's line, saying whose it answers — in the diff, in Pending comments
  and to an agent — instead of somewhere else or nowhere.
- **V reaches every file.** V goes on to the next file not reviewed in the files' own order, opening a folded
  folder to get there, and goes round to the first ones when it reaches the end.
- **An agent can point you at a place.** When it says where something is — a file and a line, a commit, a comment
  on a merge request — a toast says so, "Claude Code points at nav.rs:785 · Show", and Show takes you there: the
  line in the diff on screen when the file is in it, else in Code. A file opened at a line from a note now shows
  that line, not the file's top.
- **The whole history, not its newest 5000.** A branch's commits come a thousand at a time as you scroll down,
  the count saying 1000+ while there are more; a search reads on through older ones until it has looked at all of
  them, and a link to an old commit reads down to it.
- **News reaches you in the background.** Settings → General → System notifications (off unless you turn it
  on) sends what comes while GitLance is not in front to the system's notifications, on macOS, Windows and
  Linux: a mention, a review asked, a reply, a to-do; an agent's pending comments, its changes, its answers. One
  click brings GitLance forward with it open. Many at once come as one; nothing comes while GitLance is in front.
- **Mine and To review, merged or closed, end where yours end.** The host picks them, so the list stops at your
  last one instead of reading the whole project to find it, and All · To review · Mine say how many there are in
  all.

## 1.2.0 — 2026-10-27

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
- **An agent can explain its changes.** Its notes sit under the lines they are about, in the agent's colour and
  apart from your comments, with its summary over the diff, wherever you open those changes: the working tree,
  its branch, a comparison or a merge request of that branch. Answer on a note writes a comment for the agent on
  that line; dismiss one, or all of them. They stay on this Mac and never go to GitLab or GitHub.
- **An agent can draft a merge request.** It writes the title and description from what it did, with reviewers
  and labels if it knows them, and a note offers Open: the New merge request card for its branch, filled in, for
  you to check, edit and create. Nothing is created until you do, and GitLance never pushes; a branch only on
  your Mac says to push it first, as before.
- **Connect GitLab with your SSH key.** Merge requests no longer wait for a token pasted from GitLab's site. A
  token git already keeps for the host, in the remote's URL or its credential helper, is used for the session
  when GitLab takes it, and never copied. Otherwise, when your SSH key reaches GitLab, Merge requests offer
  Connect: a token for this computer made with the key, for 30 days and renewed before it ends (or Read only,
  or Not now); Settings → Merge requests has Use SSH key. A host key not yet known is never accepted for you,
  and nothing asks for a password.
- **Less text.** The app no longer explains what a reviewer already knows: a pending comment does not say who sees
  it, Submit review just counts them, the comment box says "Comment" or "Note for your agent", the Discussions
  panel says "Pending" once, Settings rows keep a line only where it adds something (Codex's limits are in a
  tooltip), and the connect panel and Settings → Merge requests say in a line which token they need.

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
