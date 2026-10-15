# Copyright 2026 Samaksh Kamboj - SPDX-License-Identifier: Apache-2.0
#!/usr/bin/env bash
# Stage fetchlined as a Tauri sidecar binary.
# Used locally and in CI before `tauri build`.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
HOST="$(rustc -vV | sed -n 's/^host: //p')"
cargo build --release -p fetchlined --locked --manifest-path "$ROOT/Cargo.toml"
mkdir -p "$ROOT/src-tauri/binaries"
cp "$ROOT/target/release/fetchlined" "$ROOT/src-tauri/binaries/fetchlined-${HOST}"
echo "sidecar: src-tauri/binaries/fetchlined-${HOST}"
