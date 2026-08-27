#!/usr/bin/env bash
set -euo pipefail

action=${1:-}
source_root=${2:-}
if [[ -z $action || ( $# -gt 2 ) ]]; then
  echo "Usage: scripts/playback-runtime.sh check|start|stop|restart|health|status|unit|socket [plugin-dir]" >&2
  exit 2
fi

venv_python="$HOME/.local/share/omarchy-ytmusic/venv/bin/python"
lib_dir="$HOME/.local/lib/omarchy-ytmusic"
backend_script="$lib_dir/server.py"
python_unit=omarchy-ytmusic.service
omamusic_unit=omamusic.service
backend_files=(server.py protocol.py auth.py catalog.py player.py play_history.py queue_session.py spectrum.py)

omamusic_bin() {
  if [[ -x $HOME/.local/bin/omamusic ]]; then
    printf '%s\n' "$HOME/.local/bin/omamusic"
    return 0
  fi
  command -v omamusic 2>/dev/null
}

unit_exists() {
  systemctl --user cat "$1" >/dev/null 2>&1
}

python_ready() {
  [[ -x $venv_python && -f $backend_script ]] && unit_exists "$python_unit" \
    && command -v mpv >/dev/null 2>&1 && command -v yt-dlp >/dev/null 2>&1
}

omamusic_ready() {
  local bin
  bin=$(omamusic_bin) || return 1
  [[ -n $bin && -x $bin ]] && unit_exists "$omamusic_unit" \
    && command -v mpv >/dev/null 2>&1 && command -v yt-dlp >/dev/null 2>&1
}

if omamusic_ready; then
  backend=omamusic
  unit=$omamusic_unit
else
  backend=python
  unit=$python_unit
fi

socket_file() {
  if [[ $backend == omamusic ]]; then
    printf '%s/omamusic/backend.sock\n' "${XDG_RUNTIME_DIR:-/tmp}"
  else
    printf '%s/omarchy-ytmusic/backend.sock\n' "${XDG_RUNTIME_DIR:-/tmp}"
  fi
}

runtime_ready() {
  if [[ $backend == omamusic ]]; then
    omamusic_ready
  else
    python_ready
  fi
}

stop_other_backend() {
  if [[ $backend == omamusic ]]; then
    systemctl --user stop "$python_unit" 2>/dev/null || true
  else
    systemctl --user stop "$omamusic_unit" 2>/dev/null || true
  fi
}

compile_backend() {
  [[ -x $venv_python && -d $lib_dir ]] || return 1
  local files=()
  local name
  for name in "${backend_files[@]}"; do
    [[ -f $lib_dir/$name ]] && files+=("$lib_dir/$name")
  done
  (( ${#files[@]} > 0 )) || return 1
  "$venv_python" -m py_compile "${files[@]}"
}

sync_backend() {
  BACKEND_CHANGED=0
  [[ -n $source_root && -f $source_root/backend/server.py && -d $lib_dir ]] || return 1
  local name
  for name in "${backend_files[@]}"; do
    [[ -f $source_root/backend/$name ]] || continue
    if [[ ! -f $lib_dir/$name ]] \
        || ! cmp -s -- "$source_root/backend/$name" "$lib_dir/$name"; then
      install -m 644 -- "$source_root/backend/$name" "$lib_dir/$name"
      BACKEND_CHANGED=1
    fi
  done
  chmod 755 -- "$lib_dir/server.py"
  compile_backend || {
    echo "playback-runtime.sh: installed backend failed to compile" >&2
    return 2
  }
  return 0
}

probe_socket() {
  python3 - "$1" <<'PY'
import socket
import sys

path = sys.argv[1]
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.settimeout(2.0)
try:
    sock.connect(path)
    data = b""
    while b"\n" not in data and len(data) < 262144:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    raise SystemExit(0 if b"\n" in data else 1)
except OSError:
    raise SystemExit(1)
finally:
    sock.close()
PY
}

wait_healthy() {
  local sock
  sock=$(socket_file)
  local i
  for i in $(seq 1 30); do
    if systemctl --user is-active --quiet "$unit" \
        && [[ -S $sock ]] \
        && probe_socket "$sock"; then
      return 0
    fi
    sleep 0.2
  done
  echo "playback-runtime.sh: backend did not become healthy" >&2
  return 1
}

ensure_running() {
  local force_restart=${1:-0}
  BACKEND_CHANGED=0
  if [[ $backend == python && -n $source_root ]]; then
    sync_backend || return $?
  fi
  runtime_ready || {
    echo "playback-runtime.sh: YouTube Music playback is not installed yet" >&2
    return 1
  }
  stop_other_backend
  if systemctl --user is-active --quiet "$unit"; then
    if (( force_restart == 1 || BACKEND_CHANGED == 1 )); then
      systemctl --user restart "$unit"
    fi
  else
    systemctl --user start "$unit"
  fi
  if wait_healthy; then
    return 0
  fi
  systemctl --user restart "$unit"
  wait_healthy
}

case $action in
  check)
    runtime_ready
    ;;
  start)
    ensure_running 0
    ;;
  stop)
    systemctl --user stop "$unit" 2>/dev/null || true
    ;;
  restart)
    ensure_running 1
    ;;
  health)
    runtime_ready || exit 1
    wait_healthy
    ;;
  status)
    runtime_ready || exit 1
    systemctl --user is-active "$unit"
    ;;
  unit)
    runtime_ready || exit 1
    printf '%s\n' "$unit"
    ;;
  socket)
    printf '%s\n' "$(socket_file)"
    ;;
  *)
    echo "playback-runtime.sh: unknown action: $action" >&2
    exit 2
    ;;
esac
