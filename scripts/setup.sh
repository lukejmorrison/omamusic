#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
mode=auto

usage() {
  cat <<'EOF'
Usage: scripts/setup.sh [--rust|--from-source|--python]

Install the unprivileged OMA Music playback backend. The user unit is
never enabled at login; the player or CLI starts it on demand.

  (default)  Download the pinned GitHub Release binary. If that fails,
             build with cargo. If cargo is missing, install Python.
  --rust     Download or cargo-build omamusic. Do not use Python.
  --from-source
             cargo build from this checkout. Skip the prebuilt download.
  --python   Install the legacy Python backend.
EOF
}

while (( $# > 0 )); do
  case $1 in
    --rust) mode=rust ;;
    --from-source) mode=from-source ;;
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
unit_file="$unit_dir/omamusic.service"
auth_dir="$config_root/omamusic"

install_cli_wrapper() {
  install -d -m 755 -- "$bin_dir"
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

cpu_asset() {
  case $(uname -m) in
    x86_64|amd64) printf '%s\n' omamusic-x86_64-linux ;;
    aarch64|arm64) printf '%s\n' omamusic-aarch64-linux ;;
    *) return 1 ;;
  esac
}

finish_rust_install() {
  local how=$1
  install -d -m 700 -- "$auth_dir" "$unit_dir" "$bin_dir"
  install -m 644 -- "$source_root/systemd/omamusic.service" "$unit_file"
  systemctl --user daemon-reload
  systemctl --user stop omarchy-ytmusic.service 2>/dev/null || true
  import_auth "$auth_dir"
  install_cli_wrapper
  "$bin_dir/omamusic" serve --self-test >/dev/null
  echo "Installed omamusic ($how) to $bin_dir/omamusic"
  echo "CLI: $bin_dir/omamusic"
  echo "The user unit is $unit_file and is not enabled at login."
}

load_release_pin() {
  local pin="$source_root/scripts/backend-release"
  [[ -f $pin ]] || return 1
  # shellcheck disable=SC1090
  source "$pin"
  [[ -n ${RELEASE_REPO:-} && -n ${RELEASE_VERSION:-} ]]
}

install_prebuilt() {
  local asset base tmp
  load_release_pin || return 1
  asset=$(cpu_asset) || return 1
  command -v curl >/dev/null 2>&1 || return 1
  command -v sha256sum >/dev/null 2>&1 || return 1
  base="https://github.com/${RELEASE_REPO}/releases/download/v${RELEASE_VERSION}"
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/omamusic-dl.XXXXXX")
  if ! curl -fsSL --retry 3 --retry-delay 1 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"; then
    rm -rf -- "$tmp"
    return 1
  fi
  if ! curl -fsSL --retry 3 --retry-delay 1 -o "$tmp/$asset" "$base/$asset"; then
    rm -rf -- "$tmp"
    return 1
  fi
  if ! grep -Eq "[[:space:]]\\*?${asset}\$" "$tmp/SHA256SUMS"; then
    rm -rf -- "$tmp"
    return 1
  fi
  if ! (cd "$tmp" && sha256sum -c --ignore-missing SHA256SUMS); then
    rm -rf -- "$tmp"
    return 1
  fi
  install -d -m 755 -- "$bin_dir"
  install -m 755 -- "$tmp/$asset" "$bin_dir/omamusic"
  rm -rf -- "$tmp"
  finish_rust_install "prebuilt v${RELEASE_VERSION} $asset"
}

install_from_source() {
  command -v cargo >/dev/null 2>&1 || return 1
  # Never write target/ inside the plugin tree. Omarchy hot-reloads on any
  # write there and would restart the shell mid-setup.
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$cache_root/omamusic/target}"
  install -d -m 700 -- "$CARGO_TARGET_DIR"
  cargo build --release --locked --manifest-path "$source_root/Cargo.toml" || return 1
  install -d -m 755 -- "$bin_dir"
  install -m 755 -- "$CARGO_TARGET_DIR/release/omamusic" "$bin_dir/omamusic"
  finish_rust_install "cargo build"
}

install_python() {
  command -v python3 >/dev/null 2>&1 || {
    echo "setup.sh: python3 is required for the legacy Python backend" >&2
    return 1
  }

  local lib_dir="$HOME/.local/lib/omamusic"
  local venv_dir="$data_root/omamusic/venv"
  local py_unit_file="$unit_file"
  local py_auth_dir="$auth_dir"

  # Never compile or write inside the plugin directory.
  install -d -m 700 -- "$lib_dir" "$py_auth_dir" "$unit_dir" "$(dirname -- "$venv_dir")"

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
    "$source_root/systemd/omamusic-python.service" > "$py_unit_file"
  chmod 644 -- "$py_unit_file"

  systemctl --user daemon-reload
  systemctl --user stop omarchy-ytmusic.service 2>/dev/null || true
  import_auth "$py_auth_dir"
  install_cli_wrapper
  if [[ ! -x $bin_dir/omamusic ]]; then
    cat > "$bin_dir/omamusic" <<EOF
#!/usr/bin/env bash
exec python3 "$lib_dir/cli.py" "\$@"
EOF
    chmod 755 -- "$bin_dir/omamusic"
  fi
  "$venv_dir/bin/python" "$lib_dir/server.py" --self-test >/dev/null

  echo "Installed Python playback fallback to $lib_dir"
  echo "CLI: $bin_dir/omamusic (or python3 $lib_dir/cli.py)"
  echo "The user unit is $py_unit_file and is not enabled at login."
}

install_omamusic() {
  if install_prebuilt; then
    return 0
  fi
  echo "setup.sh: prebuilt omamusic download failed; trying cargo" >&2
  if install_from_source; then
    return 0
  fi
  return 1
}

case $mode in
  from-source)
    install_from_source || {
      echo "setup.sh: cargo build failed" >&2
      exit 1
    }
    ;;
  rust)
    install_omamusic || {
      echo "setup.sh: could not install omamusic (download and cargo both failed)" >&2
      exit 1
    }
    ;;
  python)
    install_python
    ;;
  auto)
    if install_omamusic; then
      exit 0
    fi
    echo "setup.sh: omamusic unavailable; installing the legacy Python backend" >&2
    install_python
    ;;
esac
