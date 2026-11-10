#!/usr/bin/env bash
# Builds the `cg-wasm` crate and writes the JS glue and the .wasm file into the viewer.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# tree-sitter-java is C code. The C compiler for wasm32-unknown-unknown has no libc
# headers, so `tree-sitter-language` ships a small set (`wasm/include`) for it.
language_manifest=$(cargo metadata --format-version 1 --offline |
    jq -r '.packages[] | select(.name == "tree-sitter-language") | .manifest_path')
include="$(dirname "$language_manifest")/wasm/include"
[ -d "$include" ] || { echo "no $include (tree-sitter-language headers for wasm)" >&2; exit 1; }
export CFLAGS_wasm32_unknown_unknown="-I$include"

cargo build -p cg-wasm --target wasm32-unknown-unknown --release --offline
wasm-bindgen --target web --out-dir web/src/lib/wasm \
    target/wasm32-unknown-unknown/release/cg_wasm.wasm
