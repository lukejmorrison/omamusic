#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

usage() {
  cat <<'EOF'
Usage: scripts/setup.sh

Build omamusic and install the unprivileged user unit. The unit is never
enabled at login; the CLI or a future shell plugin starts it on demand.
EOF
}

if [[ ${1:-} == -h || ${1:-} == --help ]]; then
  usage
  exit 0
fi

for command_name in cargo mpv yt-dlp systemctl install; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "setup.sh: required command is missing: $command_name" >&2
    echo "Install mpv and yt-dlp with: omarchy pkg add mpv yt-dlp" >&2
    exit 1
  }
done

config_root=${XDG_CONFIG_HOME:-"$HOME/.config"}
bin_dir="$HOME/.local/bin"
unit_dir="$config_root/systemd/user"
unit_file="$unit_dir/omamusic.service"
auth_dir="$config_root/omamusic"

install -d -m 700 -- "$auth_dir" "$unit_dir" "$bin_dir"

cargo build --release --manifest-path "$source_root/Cargo.toml"
install -m 755 -- "$source_root/target/release/omamusic" "$bin_dir/omamusic"
install -m 644 -- "$source_root/systemd/omamusic.service" "$unit_file"

systemctl --user daemon-reload

if [[ ! -s $auth_dir/browser.json && -s $config_root/omarchy-ytmusic/browser.json ]]; then
  install -m 600 -- "$config_root/omarchy-ytmusic/browser.json" "$auth_dir/browser.json"
fi

"$bin_dir/omamusic" serve --self-test >/dev/null

echo "Installed omamusic to $bin_dir/omamusic"
echo "The user unit is $unit_file and is not enabled at login."
