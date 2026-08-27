#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

usage() {
  cat <<'EOF'
Usage: scripts/reload-daemon.sh

Rebuild the playback backend, install it, and restart the user unit.
Same as scripts/reload.sh --backend-only --no-open.
EOF
}

if [[ ${1:-} == -h || ${1:-} == --help ]]; then
  usage
  exit 0
fi

if command -v cargo >/dev/null 2>&1; then
  "$source_root/scripts/setup.sh" --from-source
else
  "$source_root/scripts/setup.sh"
fi
"$source_root/scripts/playback-runtime.sh" restart "$source_root"
echo "Reloaded playback backend."
