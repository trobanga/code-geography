# 30. The commit context: message, pull requests and references of a commit

Status: accepted (2026-11-16)

## Context
An overlay shows what a commit touches, not why. The reason is in the full message and in
the pull requests and issues behind it (CG-1). GitHub allows 60 API calls per hour without
a token, so each extra call must be one the developer asked for.

## Decision
- Forge repositories only. A local `diff.json` has no message.
- A forge gives two more calls (`CommitSource`): `between(base, head)`, the commits of
  `base..head` with their full messages, newest first, and `pullsOf(sha)`, the pull
  requests that the forge links to one commit.
  - GitHub: `compare/{base}...{head}?per_page=100` and `commits/{sha}/pulls`. The compare
    lists the commits oldest first, in pages of equal size (`per_page`, `page`; checked on
    api.github.com 2026-11-16 with 300 and 101 commits, beyond the old limit of 250). Up to
    100 commits: one call. More: the first answer gives `total_commits`, and a second call
    asks for the last page of the page size up to 100 whose last page is fullest. It holds
    the newest commits, the head among them: 50 to 100 of them (101 commits give 50, as
    51 + 50). Two calls in all, never more.
  - Forgejo: `compare/{base}...{head}` and `commits/{sha}/pull`. The second gives the one
    pull request whose merge commit is the sha; 404 means none. Verified against Codeberg
    on 2026-11-16: a commit inside a merged pull request gives 404.
  - A root commit (no base) is the one commit of the commit list call.
- Nothing is asked before the developer opens the context ("message"). Then one call for
  the commits of the overlay, and one call for the pull requests of each commit opened.
  A single commit opens at once; a range shows its list, one commit open at a time.
- Answers are kept in memory for the session, by `base..head` and by commit sha, not in
  IndexedDB: a pull request can be opened after the commit.
- Issues need a token on both forges to list. The viewer reads references from the text
  instead: `#12`, `owner/repo#12`, and issue or pull request URLs of the same host, in the
  message and in the pull request bodies. A reference is a guess (it can name a pull
  request, or a number in prose); the viewer shows it as a link and does not call it
  "closes". A reference to a pull request that the list shows already is dropped.
- New GitHub requests go through `authorized()` (ADR 0022) with no credentials.
- A failed call shows a line in the context; the overlay stays.

## Consequences
- On Forgejo a commit merged by a merge commit or a squash shows its pull request; a
  commit inside a merged branch does not.
- A range of more than 100 commits on GitHub lists the newest 50 to 100 and says so; a
  third call could fill the list to 100, and is not worth one of 60 calls per hour.
- A pull request request still pending for a sha is reused when the commit opens again.
- An issue reference is not checked: a wrong number gives a link to a missing page.
