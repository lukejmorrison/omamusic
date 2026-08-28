#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
skill_src="$source_root/skills/ytmusic/SKILL.md"

usage() {
  cat <<'EOF'
Usage: scripts/install-agent-skill.sh

Install the OMA Music agent skill for Grok, Hermes, OpenClaw, and other
local agents. Playback uses the omamusic CLI from scripts/setup.sh. The
user unit is not enabled at login.
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

if command -v omamusic >/dev/null 2>&1 || [[ -x $HOME/.local/bin/omamusic ]]; then
  echo "CLI: omamusic"
else
  echo "omamusic is not on PATH yet. Run scripts/setup.sh, then omamusic --json <command>."
fi
echo "Agents: use the ytmusic skill and run omamusic --json <command>"
