#!/bin/bash
# uruchom.sh — Polish for "run". Launches the already-built release
# binary from src-tauri/target/release/. This is the *post-build*
# wrapper, distinct from run.sh which handles the dev/build/install
# cycle. Build first with `./run.sh build`, then `./uruchom.sh`.

cd "$(dirname "$(readlink -f "$0")")"
exec ./src-tauri/target/release/document-processor "$@"
