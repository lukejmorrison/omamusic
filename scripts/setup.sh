#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
mode=auto

usage() {
  cat <<'EOF'
Usage: scripts/setup.sh [--rust|--python]

Install the unprivileged OMA Music playback backend. The user unit is
never enabled at login; the player or CLI starts it on demand.

  (default)  Build and install omamusic when cargo is present. If cargo
             is missing, install the legacy Python backend.
  --rust     Require cargo and install omamusic. Do not fall back.
  --python   Install the legacy Python backend even if cargo is present.
EOF
}

while (( $# > 0 )); do
  case $1 in
    --rust) mode=rust ;;
    --python) mode=python ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "setup.sh: unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

for command_name in mpv yt-dlp systemctl install; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "setup.sh: required command is missing: $command_name" >&2
    echo "Install mpv and yt-dlp with: omarchy pkg add mpv yt-dlp" >&2
    exit 1
  }
done

config_root=${XDG_CONFIG_HOME:-"$HOME/.config"}
data_root=${XDG_DATA_HOME:-"$HOME/.local/share"}
cache_root=${XDG_CACHE_HOME:-"$HOME/.cache"}
bin_dir="$HOME/.local/bin"
unit_dir="$config_root/systemd/user"

install_cli_wrapper() {
  install -d -m 755 -- "$bin_dir"
  install -m 755 -- "$source_root/scripts/omarchy-ytmusic" "$bin_dir/omarchy-ytmusic"
}

import_auth() {
  local dest=$1
  install -d -m 700 -- "$dest"
  [[ -s $dest/browser.json ]] && return 0
  if [[ -s $config_root/omarchy-ytmusic/browser.json ]]; then
    install -m 600 -- "$config_root/omarchy-ytmusic/browser.json" "$dest/browser.json"
  elif [[ -s $config_root/ytmusicbar/browser.json ]]; then
    install -m 600 -- "$config_root/ytmusicbar/browser.json" "$dest/browser.json"
  fi
}

install_rust() {
  command -v cargo >/dev/null 2>&1 || {
    echo "setup.sh: cargo is required for the omamusic backend" >&2
    echo "Install Rust, or pass --python for the legacy backend." >&2
    exit 1
  }

  local auth_dir="$config_root/omamusic"
  local unit_file="$unit_dir/omamusic.service"
  # Never write target/ inside the plugin tree. Omarchy hot-reloads on any
  # write there and would restart the shell mid-setup.
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$cache_root/omamusic/target}"
  install -d -m 700 -- "$auth_dir" "$unit_dir" "$bin_dir" "$CARGO_TARGET_DIR"

  cargo build --release --locked --manifest-path "$source_root/Cargo.toml"
  install -m 755 -- "$CARGO_TARGET_DIR/release/omamusic" "$bin_dir/omamusic"
  install -m 644 -- "$source_root/systemd/omamusic.service" "$unit_file"
  systemctl --user daemon-reload
  systemctl --user stop omarchy-ytmusic.service 2>/dev/null || true
  import_auth "$auth_dir"
  install_cli_wrapper
  "$bin_dir/omamusic" serve --self-test >/dev/null

  echo "Installed omamusic to $bin_dir/omamusic"
  echo "CLI: $bin_dir/omarchy-ytmusic (shim) or $bin_dir/omamusic"
  echo "The user unit is $unit_file and is not enabled at login."
}

install_python() {
  command -v python3 >/dev/null 2>&1 || {
    echo "setup.sh: python3 is required for the legacy Python backend" >&2
    exit 1
  }

  local lib_dir="$HOME/.local/lib/omarchy-ytmusic"
  local venv_dir="$data_root/omarchy-ytmusic/venv"
  local unit_file="$unit_dir/omarchy-ytmusic.service"
  local auth_dir="$config_root/omarchy-ytmusic"

  # Never compile or write inside the plugin directory.
  install -d -m 700 -- "$lib_dir" "$auth_dir" "$unit_dir" "$(dirname -- "$venv_dir")"

  install -m 644 -- \
    "$source_root/backend/server.py" \
    "$source_root/backend/protocol.py" \
    "$source_root/backend/auth.py" \
    "$source_root/backend/catalog.py" \
    "$source_root/backend/player.py" \
    "$source_root/backend/play_history.py" \
    "$source_root/backend/queue_session.py" \
    "$source_root/backend/spectrum.py" \
    "$source_root/backend/cli.py" \
    "$lib_dir/"
  chmod 755 -- "$lib_dir/server.py"

  if [[ ! -x $venv_dir/bin/python ]]; then
    python3 -m venv "$venv_dir"
  fi
  PIP_DISABLE_PIP_VERSION_CHECK=1 \
    "$venv_dir/bin/pip" install --no-input --require-hashes \
    -r "$source_root/backend/requirements.txt"

  sed "s|ExecStart=.*|ExecStart=$venv_dir/bin/python $lib_dir/server.py|" \
    "$source_root/systemd/omarchy-ytmusic.service" > "$unit_file"
  chmod 644 -- "$unit_file"

  systemctl --user daemon-reload
  import_auth "$auth_dir"
  install_cli_wrapper
  "$venv_dir/bin/python" "$lib_dir/server.py" --self-test >/dev/null

  echo "Installed legacy Python playback to $lib_dir"
  echo "CLI: $bin_dir/omarchy-ytmusic"
  echo "The user unit is $unit_file and is not enabled at login."
}

case $mode in
  rust)
    install_rust
    ;;
  python)
    install_python
    ;;
  auto)
    if command -v cargo >/dev/null 2>&1; then
      install_rust
    else
      echo "setup.sh: cargo not found; installing the legacy Python backend" >&2
      install_python
    fi
    ;;
esac
