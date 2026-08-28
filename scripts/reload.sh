#!/usr/bin/env bash
set -euo pipefail

# After a local YouTube Music code change: rebuild/install the backend,
# restart playback, restart Omarchy shell so QML and Api.js reload, then
# reopen the player. Hot-reload misses .pragma library files (Api.js).

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
plugin_id=wizwam.omamusic
restart_shell=1
open_player=1

usage() {
  cat <<'EOF'
Usage: scripts/reload.sh [--backend-only] [--no-open]

Reload YouTube Music after a local fix so the running player matches this
checkout.

  1. Rebuild omamusic (or copy backend/*.py) and restart the daemon
  2. Restart Omarchy shell (QML, including Api.js)
  3. Reopen the full player

  --backend-only   Skip the shell restart (daemon-only change)
  --no-open         Do not reopen the full player window
EOF
}

while (( $# > 0 )); do
  case $1 in
    --backend-only) restart_shell=0 ;;
    --no-open) open_player=0 ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "reload.sh: unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

command -v omarchy >/dev/null 2>&1 || {
  echo "reload.sh: omarchy is required" >&2
  exit 1
}

echo "Reloading YouTube Music playback backend…"
if command -v cargo >/dev/null 2>&1; then
  "$source_root/scripts/setup.sh" --from-source
else
  "$source_root/scripts/setup.sh"
fi
"$source_root/scripts/playback-runtime.sh" restart "$source_root"

if (( restart_shell )); then
  echo "Restarting Omarchy shell so the player UI reloads…"
  omarchy restart shell
  for (( attempt = 0; attempt < 40; attempt++ )); do
    if omarchy-shell shell ping >/dev/null 2>&1; then
      break
    fi
    sleep 0.15
  done
  omarchy-shell shell ping >/dev/null 2>&1 || {
    echo "reload.sh: Omarchy shell did not come back" >&2
    exit 1
  }
  omarchy-shell shell rescanPlugins >/dev/null 2>&1 || true
  discovered=0
  for (( attempt = 0; attempt < 40; attempt++ )); do
    if omarchy plugin list --json 2>/dev/null | python3 -c \
      'import json,sys; data=json.load(sys.stdin); raise SystemExit(0 if any(item.get("id")=="wizwam.omamusic" for item in data) else 1)'
    then
      discovered=1
      break
    fi
    sleep 0.05
  done
  (( discovered )) || echo "reload.sh: warning: wizwam.omamusic not listed yet" >&2
fi

if (( open_player )); then
  echo "Opening the player…"
  omarchy-shell -q "$plugin_id.player" togglePlayer || true
fi

echo "Reloaded. Backend is healthy; player UI is a fresh load."
