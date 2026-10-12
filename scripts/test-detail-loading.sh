#!/usr/bin/env bash
set -euo pipefail
source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
command -v quickshell >/dev/null 2>&1 || {
  echo "test-detail-loading.sh: quickshell is required" >&2
  exit 1
}
test_root=$(mktemp -d)
trap 'rm -rf -- "$test_root"' EXIT
mkdir -m 700 "$test_root/runtime"
ln -s /usr/share/omarchy/shell/Commons "$test_root/Commons"
python3 - "$source_root" "$test_root" <<'PY'
import sys
from pathlib import Path
source, target = map(Path, sys.argv[1:])
text = (source / 'tests/quickshell/detail-loading/shell.qml').read_text()
(target / 'shell.qml').write_text(text.replace('__PLUGIN_URL__', source.as_uri()))
PY
output=$(env -u WAYLAND_DISPLAY -u DISPLAY XDG_RUNTIME_DIR="$test_root/runtime" \
  QT_QPA_PLATFORM=offscreen QT_QPA_PLATFORMTHEME=generic \
  timeout 15s quickshell -p "$test_root" --no-duplicate 2>&1) || {
  printf '%s\n' "$output"
  exit 1
}
printf '%s\n' "$output"
[[ $output == *DETAIL_LOADING_PASS* && $output != *DETAIL_LOADING_FAIL* ]]
