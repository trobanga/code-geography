---
name: cut-release
description: Cut a release of code-geography — bump the version in Cargo.toml and web/package.json, commit, create an annotated tag vX.Y.Z, and push both to origin, which makes CI build the WASM and publish a GitHub release. Use when the user asks to cut, tag, or ship a release, to bump the version, or to create and push a version tag. Accepts an explicit version (e.g. "0.2.0") or a bump level (major/minor/patch).
---

# Cut a release

Bump the project version, commit it, tag it, push it. One commit, one annotated tag,
both on `origin/main`. The tag starts the `release` job of `.github/workflows/ci.yml`:
it builds the WASM from the sources, builds the viewer on it, and makes a GitHub
release with the two archives.

`$ARGUMENTS` is optional. It holds an explicit version (`0.2.0`) or a bump level
(`major`, `minor`, `patch`). If it is empty, propose a version at step 2.

## Rules

- Run every `git` command directly in the working directory. Do NOT use `cd <dir> && git ...`
  or `git -C <path> ...` — both start a sandbox permission prompt.
- Run `git push`, `git fetch` and `gh` with the sandbox off.
- Commit with `--signoff`.
- Tag the bump commit itself, not the commit after it.
- Never move or delete a tag that is already on `origin`. A pushed tag is public and
  can already have a release. If the version is wrong, cut the next version instead.

## The version

The version stands in two files, and the `release` job fails if the tag differs from
one of them:

- `Cargo.toml`, `[workspace.package] version`. All crates inherit it
  (`version.workspace = true`). `Cargo.lock` holds it once per crate.
- `web/package.json`, `"version"`.

## Steps

### 1. Check the preconditions

```bash
git fetch origin
git status -sb
git tag --sort=-v:refname | head -5
git ls-remote --tags origin
sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml
jq -r .version web/package.json
```

Stop and report if one of these is true:

- The branch is not `main`.
- A tracked file is modified or staged. Untracked files are correct — leave them alone.
- The branch is behind `origin/main`. Run `git pull --rebase` first.
- The two versions differ. Report both; do not guess which one is correct.

If no tag is on the remote yet and `$ARGUMENTS` is empty, the first release can be the
version in the files as it is. Propose that at step 2. Then there is no bump commit:
skip step 4 and tag `HEAD`.

### 2. Choose the version

If `$ARGUMENTS` names a version, use it. If it names a bump level, apply that level
to the highest tag (or to the version in the files when there is no tag).

If not, read the commits since the last tag and propose one:

```bash
git log --oneline $(git describe --tags --abbrev=0)..HEAD
```

With no tag yet, read `git log --oneline` instead.

The project is pre-1.0, so:

- Each new feature, each new front end, and each change to a format that other parts
  read (the `graph.json` or `diff.json` of `cg`, the URL hash of the viewer, the WASM
  interface of `Explorer`) → **minor** bump.
- Only fixes, tests, chores, CI changes, or docs → **patch** bump.

Ask the user to agree to the proposed version before you change a file. A pushed tag
is public.

### 3. Run the quality gates

These are the CI jobs, plus one check that CI does not do. Skip nothing. Report each
failure and stop — do not tag a red tree.

```bash
cargo fmt --all --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p cg-core --target wasm32-unknown-unknown
scripts/build-wasm.sh
git status --short web/src/lib/wasm
cd web && pnpm install --frozen-lockfile && pnpm check && pnpm test && pnpm build
```

The `web` job of CI tests the committed WASM, but the release ships a WASM built from
the sources. If `git status` shows a change in `web/src/lib/wasm` after
`scripts/build-wasm.sh`, the committed WASM is stale: stop and tell the user. The fix
is a commit of the rebuilt WASM (with a run of the `web` gates on it) before the
release, not a part of the bump commit.

`scripts/build-wasm.sh` needs `clang`, `jq` and the `wasm-bindgen` CLI at the version
of the `wasm-bindgen` crate in `crates/wasm/Cargo.toml`. If the CLI version differs,
report it; do not change the crate version for the release.

### 4. Bump the version

Change the version with the Edit tool in both places:

- `Cargo.toml`: the `version` line under `[workspace.package]`.
- `web/package.json`: the `"version"` field.

Then update the lock file and confirm that only the 8 workspace crates changed:

```bash
cargo update --workspace --offline
git diff --stat
```

`git diff --stat` must show `Cargo.toml`, `Cargo.lock` and `web/package.json`, and
`Cargo.lock` only the version lines of the `cg*` crates. Then commit:

```bash
git add Cargo.toml Cargo.lock web/package.json
git commit --signoff -m "Release <version>"
```

### 5. Create the annotated tag

Tag the bump commit (or `HEAD` for a first release without a bump). Build the message
from the commits in the range, in groups by kind. Keep each entry to one line, and
name the issue (`CG-<n>`) where a commit has one.

```bash
git tag -a v<version> -m "$(cat <<'EOF'
v<version>

Features
- <area>: <what a reader of the changelog must know> (CG-<n>).

Fixes
- <area>: <what was defective and what now operates>.

Operations
- <CI, build and release changes>.

Tests
- <test-only changes worth a mention>.
EOF
)"
```

Use the areas of the code: `core`, `java`, `rust`, `clojure`, `index`, `cli`, `wasm`,
`viewer`, `forge`. Remove each section that has no entry.

### 6. Push

```bash
git push
git push origin v<version>
git status -sb
```

The tag push starts a CI run on the tag: `rust` and `web`, then `release`. Wait for
it; it takes a few minutes:

```bash
gh run list --branch v<version> -L 1
gh run watch <run id> --exit-status
gh release view v<version> --json url,assets -q '.url, .assets[].name'
```

The release must hold `code-geography-v<version>-wasm.tar.gz` and
`code-geography-v<version>-viewer.tar.gz`.

If the run fails:

- At the version check: the tag and the files differ. Do not move the tag. Report it;
  the next version is necessary.
- On a runner problem (a download, a cache, a timeout): run it again once with
  `gh run rerun <run id> --failed`.
- In a gate or a build step: the tag is on a tree that CI does not accept. Report the
  log of the step. A fix needs a new commit and the next version.

### 7. Report

Give the user:

- the SHA of the bump commit and the name of the tag,
- the version you chose and the one-line reason (which commit made a minor bump
  necessary, for example),
- each item a gate showed but did not fail on,
- the URL of the release and the names of its two archives, or the failed step and
  its log.
