#!/usr/bin/env bash
# Measures the Rust `calls` edges against rust-analyzer (SCIP). Writes only into <scratch>.
#   tools/rust-truth/run.sh <cargo repo> <scratch>
# Needs: rust-analyzer (rustup component), cargo with the dependencies already in ~/.cargo
# (runs offline), node, rsync, patch.
set -euo pipefail
repo=$(realpath "$1")
scratch=$(mkdir -p "$2" && realpath "$2")
here=$(cd "$(dirname "$0")" && pwd)
ws=$(cd "$here/../.." && pwd)

# 1. A copy of the repository, so that cargo and rust-analyzer write nothing into it.
#    `.env` is left out: sqlx macros would read DATABASE_URL from it and try to connect.
rsync -rlt --exclude .git --exclude target --exclude .claude --exclude .env "$repo/" "$scratch/repo/"

# 2. A CARGO_HOME of our own that reuses ~/.cargo read-only: the registry by symlinks,
#    git dependencies from a copy of the git database (cargo checks them out on demand).
src=${CARGO_HOME:-$HOME/.cargo}
ch="$scratch/cargo-home"
if [ ! -d "$ch" ]; then
  mkdir -p "$ch/registry/src" "$ch/git"
  ln -s "$src/registry/index" "$ch/registry/index"
  ln -s "$src/registry/cache" "$ch/registry/cache"
  for d in "$src"/registry/src/*; do
    mkdir -p "$ch/registry/src/$(basename "$d")"
    for c in "$d"/*; do ln -s "$c" "$ch/registry/src/$(basename "$d")/"; done
  done
  [ -d "$src/git/db" ] && cp -r "$src/git/db" "$ch/git/db"
  [ -f "$src/config.toml" ] && cp "$src/config.toml" "$ch/"
fi
export CARGO_HOME="$ch" CARGO_NET_OFFLINE=true SQLX_OFFLINE=true

# 3. The truth, and how complete rust-analyzer's analysis is.
(cd "$scratch/repo" && rust-analyzer scip . --output "$scratch/index.scip") > "$scratch/scip.log" 2>&1
(cd "$scratch/repo" && rust-analyzer analysis-stats .) > "$scratch/stats.log" 2>&1 || true
grep -E 'exprs:|pats:' "$scratch/stats.log" || true   # ?ty: partly unknown type, ??ty: unknown
echo "scip: $(grep -c 'Duplicate symbol' "$scratch/scip.log" || true) duplicate symbols (root fns of test crates), $(grep -c 'should have been in an SCIP document' "$scratch/scip.log" || true) definitions without a document, $(grep -c 'enclosing definition with no name' "$scratch/scip.log" || true) 'enclosing definition with no name' (crate root)"

# 4. Our graph and our per-site outcomes, from a patched copy of the indexer.
rm -rf "$scratch/cg" && mkdir -p "$scratch/cg"
cp -r "$ws/Cargo.toml" "$ws/Cargo.lock" "$ws/crates" "$scratch/cg/"
(cd "$scratch/cg" && patch -s -p1 < "$here/print-sites.patch" && CARGO_HOME="$src" cargo build -q --release -p cg)
"$scratch/cg/target/release/cg" index "$scratch/repo" -o "$scratch/graph.json" 2> "$scratch/sites.txt"
grep -v -E '^(SITE|DECL|MACRO)' "$scratch/sites.txt" || true

# 5. The comparison.
node "$here/compare.mjs" "$scratch/graph.json" "$scratch/index.scip" "$scratch/repo" "$scratch/sites.txt" --list "$scratch/lists"
