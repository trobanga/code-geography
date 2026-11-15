# Issue tracker

- **TRACKER**: Exponential, the self-hosted workspace at `https://exponential.trobanga.de`.
  Team `CG` (Code Geography). Issues have identifiers such as `CG-1`.
- **FORGE**: `none`. The repository has no remote.
- **Branch format**: `{id}--{slug}`. The id is the number of the identifier: `CG-7` → `7`.

## Operations

The tracker is reached through `exp`, the command-line client of Exponential
(`~/code/exponential/cli`, install with `cargo install --path ~/code/exponential/cli`). It reads
`EXPONENTIAL_API_TOKEN` from the environment and exits non-zero with the error of the server.

- **T1 — Fetch the issue.** `exp issue get CG-7` prints the fields, the description and the
  comments. The list is `exp issue list --team CG` (`--status`, `--assignee`).
- **T2 — Detect existing work.** Look for a local branch `{id}--*` (`git branch --list '7--*'`)
  and a worktree below `.claude/worktrees/`.
- **T3 — Claim the issue.** `exp issue update CG-7 --status in_progress --assignee me`.
- **T4 — Branch id.** The number of the identifier: `CG-7` → `7`.
- **T5 — Close the issue.** Write `CG-<number>` into the commit message. Nothing closes the
  issue on merge: after the merge, run `exp issue update CG-7 --status done`.
- **T6 — Create an issue.** `exp issue create --team CG --title "..." --description-file body.md`
  prints the new identifier. If it fails with no answer from the server, the issue may exist:
  look with `exp issue list --team CG` before you try again.
- **T7 — Link blocking.** State the dependency in the description or in a comment
  (`exp issue comment CG-7 --body-file -`).
