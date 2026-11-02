#!/bin/sh
# Builds the test modules (needs clang and lld; see docs/WASM.md, "Packages in the VM").
set -e
cd "$(dirname "$0")"
clang --target=wasm32 -O2 -nostdlib -mbulk-memory -msign-ext -mnontrapping-fptoint \
      -Wl,--no-entry -Wl,--allow-undefined -Wl,--export-memory -o demo.wasm demo.c
wat2wasm api.wat -o api.wasm
