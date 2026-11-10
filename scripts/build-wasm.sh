#!/usr/bin/env bash
# Builds the `cg-wasm` crate and writes the JS glue and the .wasm file into the viewer.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo build -p cg-wasm --target wasm32-unknown-unknown --release --offline
wasm-bindgen --target web --out-dir web/src/lib/wasm \
    target/wasm32-unknown-unknown/release/cg_wasm.wasm
