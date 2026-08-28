#!/usr/bin/env bash
set -euo pipefail

purge=0
if [[ ${1:-} == --purge ]]; then
  purge=1
elif [[ -n ${1:-} ]]; then
  echo "Usage: scripts/remove-runtime.sh [--purge]" >&2
  exit 2
fi

config_root=${XDG_CONFIG_HOME:-"$HOME/.config"}
data_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
cache_root=${XDG_CACHE_HOME:-"$HOME/.cache"}
runtime_root=${XDG_RUNTIME_DIR:-/tmp}

systemctl --user stop omamusic.service 2>/dev/null || true
systemctl --user stop omarchy-ytmusic.service 2>/dev/null || true
rm -f -- \
  "$config_root/systemd/user/omamusic.service" \
  "$config_root/systemd/user/omarchy-ytmusic.service"
systemctl --user daemon-reload 2>/dev/null || true

rm -f -- "$HOME/.local/bin/omamusic" "$HOME/.local/bin/omarchy-ytmusic"
rm -rf -- "$HOME/.local/lib/omamusic" "$HOME/.local/lib/omarchy-ytmusic"
rm -f -- \
  "$runtime_root/omamusic/backend.sock" \
  "$runtime_root/omamusic/mpv.sock" \
  "$runtime_root/omarchy-ytmusic/backend.sock" \
  "$runtime_root/omarchy-ytmusic/mpv.sock"

if (( purge )); then
  rm -rf -- \
    "$data_root/omamusic" \
    "$data_root/omarchy-ytmusic" \
    "$config_root/omamusic" \
    "$config_root/omarchy-ytmusic" \
    "$cache_root/omamusic" \
    "$cache_root/omarchy-ytmusic" \
    "$runtime_root/omamusic" \
    "$runtime_root/omarchy-ytmusic"
fi

echo "Removed the OMA Music playback units and installed backend."
if (( purge )); then
  echo "Purged venv, auth files, cache, and CLI wrapper."
fi
