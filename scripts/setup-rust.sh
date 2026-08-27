#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

usage() {
  cat <<'EOF'
Usage: scripts/setup-rust.sh

Build omamusic and install the unprivileged user unit. Same as
scripts/setup.sh --rust. The unit is never enabled at login.
EOF
}

if [[ ${1:-} == -h || ${1:-} == --help ]]; then
  usage
  exit 0
fi

exec "$source_root/scripts/setup.sh" --rust
