#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$source_root"

command -v omarchy >/dev/null 2>&1 || {
  echo "test.sh: omarchy is required" >&2
  exit 1
}

omarchy plugin validate .

python3 "$source_root/backend/server.py" --self-test
python3 "$source_root/tests/test_catalog.py"
python3 "$source_root/tests/test_auth.py"
python3 "$source_root/tests/test_player.py"
python3 "$source_root/tests/test_protocol.py"
python3 "$source_root/tests/test_play_history.py"
python3 "$source_root/tests/test_queue_session.py"
python3 "$source_root/tests/test_server.py"
python3 "$source_root/tests/test_spectrum.py"
if [[ -f $source_root/tests/test_cli.py ]]; then
  python3 "$source_root/tests/test_cli.py"
fi

if command -v qmllint >/dev/null 2>&1; then
  qmllint -I /usr/share/omarchy/shell Api.js \
    ArtistLinks.qml MediaByline.qml MediaRow.qml MediaCollection.qml \
    PlaybackSlider.qml FastScrollHandler.qml LyricsInstallPrompt.qml \
    SidebarItem.qml RoundedField.qml Chicklet.qml Artwork.qml EqBar.qml \
    SpectrumBar.qml \
    BackendClient.qml \
    DaemonManager.qml \
    Service.qml \
    BarWidget.qml Panel.qml
fi

qml_test_runner=/usr/lib/qt6/bin/qmltestrunner
if [[ -x $qml_test_runner ]]; then
  QT_QPA_PLATFORM=offscreen "$qml_test_runner" \
    -input tests \
    -import "$source_root" \
    -o -,txt
fi

if command -v rg >/dev/null 2>&1; then
  if rg -n 'QtWebEngine|WebEngineView|WebView|node_modules|electron' \
    --glob '*.qml' --glob '*.js' --glob '*.sh' --glob '*.service' \
    --glob '!scripts/test.sh' .; then
    echo "test.sh: forbidden heavyweight runtime dependency found" >&2
    exit 1
  fi
  if rg -n 'pip install --upgrade pip|ytmusicapi>=' scripts/setup.sh backend/requirements.txt; then
    echo "test.sh: setup must pin hashed requirements, not upgrade pip" >&2
    exit 1
  fi
  if ! rg -q -- '--hash=sha256:' backend/requirements.txt; then
    echo "test.sh: backend/requirements.txt must pin hashes" >&2
    exit 1
  fi
  if ! rg -q 'play_history.py' scripts/playback-runtime.sh \
      || ! rg -q 'queue_session.py' scripts/playback-runtime.sh \
      || ! rg -q 'spectrum.py' scripts/playback-runtime.sh \
      || ! rg -q 'wait_healthy' scripts/playback-runtime.sh; then
    echo "test.sh: playback-runtime.sh must sync the full backend and health-check" >&2
    exit 1
  fi
  if ! rg -q 'omamusic.service' scripts/playback-runtime.sh \
      || ! rg -q 'omamusic/backend.sock' scripts/playback-runtime.sh \
      || ! rg -q 'stop_other_backend' scripts/playback-runtime.sh; then
    echo "test.sh: playback-runtime.sh must prefer omamusic when it is installed" >&2
    exit 1
  fi
  if [[ -f scripts/omarchy-ytmusic ]] && { ! rg -q 'backend/cli.py' scripts/setup.sh \
      || ! rg -q 'omarchy-ytmusic' scripts/setup.sh; }; then
    echo "test.sh: setup.sh must install the omarchy-ytmusic CLI" >&2
    exit 1
  fi
fi

socket_path=$(bash scripts/playback-runtime.sh socket)
if [[ $socket_path != *backend.sock ]]; then
  echo "test.sh: socket action must print a backend socket path" >&2
  exit 1
fi
if bash scripts/playback-runtime.sh unit >/dev/null 2>&1; then
  unit=$(bash scripts/playback-runtime.sh unit)
  if [[ $unit == omamusic.service && $socket_path != */omamusic/backend.sock ]]; then
    echo "test.sh: omamusic.service is selected so the plugin socket must be omamusic" >&2
    exit 1
  fi
fi

bash -n scripts/playback-runtime.sh
bash -n scripts/setup.sh
bash -n scripts/reload.sh
[[ -f scripts/omarchy-ytmusic ]] && bash -n scripts/omarchy-ytmusic
[[ -f scripts/install-agent-skill.sh ]] && bash -n scripts/install-agent-skill.sh
[[ -f scripts/setup-rust.sh ]] && bash -n scripts/setup-rust.sh

echo "All validation and tests passed."
