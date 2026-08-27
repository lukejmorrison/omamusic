#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

usage() {
  cat <<'EOF'
Usage: scripts/reload.sh

Rebuild omamusic, install the binary, and restart omamusic.service.
EOF
}

if [[ ${1:-} == -h || ${1:-} == --help ]]; then
  usage
  exit 0
fi

"$source_root/scripts/setup.sh"
systemctl --user restart omamusic.service || systemctl --user start omamusic.service
echo "Reloaded omamusic."
