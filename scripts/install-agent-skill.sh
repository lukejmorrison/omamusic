#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
skill_src="$source_root/skills/ytmusic/SKILL.md"
wrapper="$source_root/scripts/omarchy-ytmusic"
lib_dir="$HOME/.local/lib/omarchy-ytmusic"
bin_dir="$HOME/.local/bin"

usage() {
  cat <<'EOF'
Usage: scripts/install-agent-skill.sh

Install the YouTube Music agent skill and omarchy-ytmusic CLI for Grok,
Hermes, OpenClaw, and other local agents. Does not enable the backend at
login. Playback still starts when the player opens or when the CLI starts
the existing user unit.
EOF
}

if [[ ${1:-} == -h || ${1:-} == --help ]]; then
  usage
  exit 0
fi

[[ -f $skill_src ]] || {
  echo "install-agent-skill.sh: missing $skill_src" >&2
  exit 1
}

install -d -m 755 -- "$lib_dir" "$bin_dir"
install -m 644 -- "$source_root/backend/cli.py" "$lib_dir/"
# The playback backend already ships protocol.py. Do not replace it with an
# older copy if this machine is running a newer plugin checkout.
if [[ ! -f $lib_dir/protocol.py ]]; then
  install -m 644 -- "$source_root/backend/protocol.py" "$lib_dir/"
fi
install -m 755 -- "$wrapper" "$bin_dir/omarchy-ytmusic"

link_skill() {
  local target=$1
  install -d -m 755 -- "$target"
  ln -sfn "$skill_src" "$target/SKILL.md"
  echo "Linked $target/SKILL.md"
}

link_skill "$HOME/.grok/skills/ytmusic"
link_skill "$HOME/.hermes/skills/ytmusic"
link_skill "$HOME/.agents/skills/ytmusic"
link_skill "$HOME/.openclaw/skills/ytmusic"
link_skill "$HOME/.config/opencode/skills/ytmusic"

if [[ -d /home/luke/dev/template_skills ]]; then
  install -d -m 755 -- /home/luke/dev/template_skills/ytmusic
  ln -sfn "$skill_src" /home/luke/dev/template_skills/ytmusic/SKILL.md
  ln -sfn "$skill_src" /home/luke/dev/template_skills/ytmusic.SKILL.md
  echo "Linked template_skills/ytmusic"
fi

echo "Installed $bin_dir/omarchy-ytmusic"
echo "Agents: use the ytmusic skill and run omarchy-ytmusic --json <command>"
