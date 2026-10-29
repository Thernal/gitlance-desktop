---
name: gitlance
description: Show the reviewer your changes in GitLance without being asked, with a summary and notes — after a commit; after a branch is made or rewritten (squash, rebase, cherry-pick, reword); after review comments are addressed; before asking the reviewer to look or to approve. Use whenever GitLance's MCP tools (open_changes) are here and one of these has just happened, a local branch not pushed yet included.
---

# Show your changes in GitLance

GitLance is the reviewer's diff app. The reviewer reads your work there, and sees it only when you open it.

## When

Without being asked, right after:

- a commit;
- a branch made or rewritten: a squash, a rebase, a cherry-pick, a reworded commit;
- review comments addressed (after `address_comment`);

and before you ask the reviewer to look or to approve. A local branch not pushed yet counts: the reviewer reads it in
GitLance before anything is pushed.

## How

Call `open_changes`:

- your branch against its base (`head` the branch, `base` left out for the main line), or `working_tree: true` when
  nothing is committed;
- `summary`: what the changes do, in a few sentences;
- `notes`: one on each file whose change the diff alone does not explain — why this way, what you left alone, what
  to look at closely — on a line the change leaves in that file.

Notes again on the same changes replace your earlier ones; answers to them come back as review comments. When a tool
says GitLance is not running, call `open_gitlance` first.
