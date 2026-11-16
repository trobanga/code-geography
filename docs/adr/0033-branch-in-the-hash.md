# 33. The branch of the commit shown: a hint in the hash, else the default branch

Status: accepted (2026-11-16)

## Context
The hash names a commit by its sha, so that a reload and a shared link show the same code
(ADR 0020, G5). The branch picked from the branch menu was in memory only: after a reload
the menu showed no name.

Git gives a commit no branch. A branch is a pointer, and one commit is reachable from many
branches. A forge can only say whether one branch holds one commit, at one call per branch.

## Decision
- The hash holds `branch=<name>` beside `gh=` or `forge=`. It is a memory of what the
  developer picked, not a part of the identity: the sha names the commit. A reload shows
  the same commit on that branch and makes no call for the branch. The name is not checked
  against the forge, so a hand-written link can name a branch that does not hold the commit.
- A commit without a branch (an old link, a pasted sha, the input field) gets the default
  branch of the repository, if the forge says that this branch holds the commit:
  - `Forge.defaultBranch(repo)`: one call (`GET repos/{owner}/{repo}`, field
    `default_branch`, on GitHub and Forgejo). Asked once per repository until a reload.
  - `Forge.mergeBase(repo, branch, sha) == sha`: one call. No call for a repository opened
    without a ref, since its commit is the head of the default branch.
  The search runs after the picture is shown. A failure is no error: the commit stays
  without a branch. A commit that only another branch holds stays without a branch.
- The found name goes into the hash like a picked one, so the next reload costs no call.
- Not `main`, else `master`: the forge knows the default branch, also when it is `develop`.

## Consequences
- The first open of a link without `branch=` costs two API calls more (GitHub: 60 per hour
  without a token); one for a repository without a ref.
- A commit that several branches hold shows the default branch, also when the developer
  came to it on another branch by a way that keeps no branch (a pasted sha).
- The commit list still starts at the commit shown, not at the head of the branch: a link
  to an old commit does not show the commits after it.
- A ref of the input field that is a branch name (`owner/repo@feature`) is not taken as
  the branch; the default branch is shown if it holds the commit, else none.
