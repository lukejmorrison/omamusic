#!/usr/bin/env bash
set -euo pipefail
cd "$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

command -v cargo >/dev/null 2>&1 || {
  echo "test-rust.sh: cargo is required" >&2
  exit 1
}

# Keep Cargo's target dir out of the plugin tree so Omarchy does not
# hot-reload the shell while tests compile.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/omamusic/target}"
install -d -m 700 -- "$CARGO_TARGET_DIR"

cargo test --locked --manifest-path Cargo.toml
cargo run --locked --quiet --manifest-path Cargo.toml -- serve --self-test
