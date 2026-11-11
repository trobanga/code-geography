#!/usr/bin/env bash
# Measures the Java `calls` edges against javac. Writes only into <scratch>.
#   tools/java-truth/run.sh <maven repo> <scratch>
# Needs: JDK 21+, Maven with the dependencies in ~/.m2 (runs offline), cargo, node, rsync, patch.
set -euo pipefail
repo=$(realpath "$1")
scratch=$(mkdir -p "$2" && realpath "$2")
here=$(cd "$(dirname "$0")" && pwd)
ws=$(cd "$here/../.." && pwd)

# 1. A copy of the repository, so that Maven writes nothing into it.
rsync -rlt --exclude .git --exclude target --exclude .worktrees --exclude .claude "$repo/" "$scratch/repo/"

# 2. The union of the test classpaths of all modules, without the modules' own jars.
modules=$(sed -n 's:.*<module>\(.*\)</module>.*:\1:p' "$scratch/repo/pom.xml")
: > "$scratch/cp-all.txt"
for m in $modules; do
  (cd "$scratch/repo" && mvn -q -o -pl "$m" dependency:build-classpath -Dmdep.outputFile="$scratch/cp-$m.txt")
  { cat "$scratch/cp-$m.txt"; echo; } >> "$scratch/cp-all.txt"
done
group=$(sed -n '0,/<groupId>/s:.*<groupId>\(.*\)</groupId>.*:\1:p' "$scratch/repo/pom.xml" | tr . /)
tr ':' '\n' < "$scratch/cp-all.txt" | grep -v '^$' | grep -v "/repository/$group/" | awk '!s[$0]++' | paste -sd: > "$scratch/cp-union.txt"

# 3. The truth.
java "$here/CallTruth.java" "$scratch/repo" "$scratch/cp-union.txt" "$scratch/truth.tsv" "$scratch/javac-out"

# 4. Our graph and our per-site outcomes, from a patched copy of the indexer.
rm -rf "$scratch/cg" && mkdir -p "$scratch/cg"
cp -r "$ws/Cargo.toml" "$ws/Cargo.lock" "$ws/crates" "$scratch/cg/"
(cd "$scratch/cg" && patch -s -p1 < "$here/print-sites.patch" && cargo build -q --release -p cg)
"$scratch/cg/target/release/cg" index "$scratch/repo" -o "$scratch/graph.json" 2> "$scratch/sites.txt"
grep -v '^SITE' "$scratch/sites.txt" || true

# 5. The comparison.
mkdir -p "$scratch/lists"
node "$here/compare.mjs" "$scratch/graph.json" "$scratch/truth.tsv" "$scratch/sites.txt" --list "$scratch/lists"
