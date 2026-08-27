#!/usr/bin/env python3
"""Command-line client for the Omarchy YouTube Music backend socket."""

from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from protocol import (  # noqa: E402
    BACKEND_VERSION,
    MAX_LINE_BYTES,
    PROTOCOL_VERSION,
    dumps,
    parse_line,
)

CLI_VERSION = "1.0.0"
DEFAULT_TIMEOUT = 15.0
CATALOG_TIMEOUT = 45.0
UNIT = "omarchy-ytmusic.service"
PANEL_PROP = "wizwam.omamusic.player"

TRANSPORT = ("play", "pause", "toggle", "stop", "next", "previous")
KNOWN_COMMANDS = TRANSPORT + (
    "status", "now", "state", "hello", "ping", "get-state", "health",
    "prev", "volume", "seek", "shuffle", "repeat", "search", "play-id",
    "playid", "like", "unlike", "queue", "browse", "open", "player",
    "mini", "open-mini", "raw",
)
BOOL_TRUE = {"1", "true", "on", "yes"}
BOOL_FALSE = {"0", "false", "off", "no"}


class CliError(Exception):
    def __init__(self, message: str, code: int = 1):
        super().__init__(message)
        self.code = code


def socket_path() -> Path:
    root = Path(os.environ.get("XDG_RUNTIME_DIR") or f"/tmp/omarchy-ytmusic-{os.getuid()}")
    return root / "omarchy-ytmusic" / "backend.sock"


def format_ms(ms: Any) -> str:
    seconds = max(0, int(ms or 0) // 1000)
    return f"{seconds // 60}:{seconds % 60:02d}"


def parse_bool(value: str) -> bool:
    text = str(value or "").strip().lower()
    if text in BOOL_TRUE:
        return True
    if text in BOOL_FALSE:
        return False
    raise CliError(f"expected on/off, got {value!r}")


def parse_volume(value: str) -> int:
    volume = int(value)
    if volume < 0 or volume > 100:
        raise CliError("volume must be 0-100")
    return volume


def parse_position(value: str) -> int:
    text = str(value or "").strip()
    if ":" in text:
        parts = text.split(":")
        if len(parts) != 2 or not parts[0].isdigit() or not parts[1].isdigit():
            raise CliError(f"seek expected mm:ss or milliseconds, got {value!r}")
        return (int(parts[0]) * 60 + int(parts[1])) * 1000
    if not text.isdigit():
        raise CliError(f"seek expected mm:ss or milliseconds, got {value!r}")
    number = int(text)
    # Bare 0-600 looks like seconds; larger values are already ms.
    return number * 1000 if number <= 600 else number


def first_playable(result: dict[str, Any] | None) -> dict[str, Any] | None:
    payload = result or {}
    buckets: list[Any] = []
    if isinstance(payload.get("items"), list):
        buckets.extend(payload["items"])
    for section in payload.get("sections") or []:
        if isinstance(section, dict) and isinstance(section.get("items"), list):
            buckets.extend(section["items"])
    for item in buckets:
        if isinstance(item, dict) and item.get("videoId"):
            return item
    return None


def track_label(track: dict[str, Any] | None) -> str:
    if not isinstance(track, dict) or not track:
        return "Nothing playing"
    artist = str(track.get("subtitle") or "").strip()
    title = str(track.get("name") or track.get("title") or "YouTube Music").strip()
    if artist:
        return f"{artist} — {title}"
    return title


def format_status(result: dict[str, Any]) -> str:
    track = result.get("track") if isinstance(result.get("track"), dict) else None
    playing = "playing" if result.get("playing") else "paused"
    if result.get("resolving"):
        playing = "resolving"
    lines = [
        f"{playing}  {track_label(track)}",
        "  ".join((
            f"{format_ms(result.get('position_ms'))} / {format_ms(result.get('duration_ms'))}",
            f"vol={int(result.get('volume') or 0)}",
            f"shuffle={'on' if result.get('shuffle') else 'off'}",
            f"repeat={result.get('repeat') or 'off'}",
        )),
        f"signed_in={str(bool(result.get('signed_in'))).lower()}  "
        f"lifecycle={result.get('lifecycle') or 'unknown'}",
    ]
    url = ""
    if track:
        url = str(track.get("externalUrl") or "")
        video_id = str(track.get("videoId") or "")
        if not url and video_id:
            url = f"https://music.youtube.com/watch?v={video_id}"
    if url:
        lines.append(url)
    error = str(result.get("error") or "").strip()
    if error:
        lines.append(f"error={error}")
    return "\n".join(lines)


def format_search(result: dict[str, Any]) -> str:
    items = [item for item in (result.get("items") or []) if isinstance(item, dict)]
    if not items:
        return "No results"
    lines = []
    for index, item in enumerate(items[:12], start=1):
        kind = str(item.get("type") or "item")
        ident = str(item.get("videoId") or item.get("id") or "")
        lines.append(f"{index}. [{kind}] {track_label(item)}  {ident}")
    return "\n".join(lines)


def format_queue(result: dict[str, Any]) -> str:
    items = result.get("items") if isinstance(result.get("items"), list) else []
    index = int(result.get("index") or 0)
    if not items:
        return "Queue is empty"
    lines = []
    for offset, item in enumerate(items):
        if not isinstance(item, dict):
            continue
        mark = ">" if offset == index else " "
        lines.append(f"{mark} {offset}. {track_label(item)}")
    return "\n".join(lines) or "Queue is empty"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="omarchy-ytmusic",
        description="Control the Omarchy YouTube Music player over its local socket.",
    )
    parser.add_argument("--json", action="store_true", help="Print the backend JSON response")
    parser.add_argument("--human", action="store_true", help="Pretty-print even when stdout is not a TTY")
    parser.add_argument("--start", action="store_true", default=True,
                        help="Start the user unit if the socket is missing (default)")
    parser.add_argument("--no-start", action="store_false", dest="start",
                        help="Do not start the backend; fail if the socket is missing")
    parser.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT,
                        help="Seconds to wait for a response (default 15)")
    parser.add_argument("--socket", default="", help="Override the backend Unix socket path")
    parser.add_argument("--version", action="store_true", help="Print CLI and protocol versions")
    parser.add_argument("command", nargs="?", help="status, play, pause, next, search, …")
    parser.add_argument("args", nargs="*", help="Command arguments")
    return parser


def want_json(args: argparse.Namespace) -> bool:
    if args.human:
        return False
    if args.json:
        return True
    return not sys.stdout.isatty()


def ensure_socket(path: Path, start: bool, timeout: float) -> Path:
    if path.is_socket():
        return path
    if not start:
        raise CliError(
            f"backend socket missing: {path}\n"
            "Open the player with Super+Shift+M or pass --start.",
            3,
        )
    started = subprocess.run(
        ["systemctl", "--user", "start", UNIT],
        capture_output=True, text=True,
    )
    if started.returncode != 0:
        detail = (started.stderr or started.stdout or "").strip()
        raise CliError(
            f"could not start {UNIT}: {detail or 'systemctl failed'}\n"
            "Open the player with Super+Shift+M once so setup can install the unit.",
            3,
        )
    deadline = time.monotonic() + min(8.0, max(1.0, timeout))
    while time.monotonic() < deadline:
        if path.is_socket():
            return path
        time.sleep(0.1)
    raise CliError(f"backend started but {path} never appeared", 3)


def read_response(sock: socket.socket, request_id: Any, timeout: float) -> dict[str, Any]:
    sock.settimeout(timeout)
    buffer = b""
    deadline = time.monotonic() + timeout
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise CliError("timed out waiting for the backend", 3)
        sock.settimeout(remaining)
        try:
            chunk = sock.recv(65536)
        except socket.timeout as exc:
            raise CliError("timed out waiting for the backend", 3) from exc
        if not chunk:
            raise CliError("backend closed the socket", 3)
        buffer += chunk
        if len(buffer) > MAX_LINE_BYTES and b"\n" not in buffer:
            raise CliError("backend sent an oversized frame", 3)
        while b"\n" in buffer:
            raw, buffer = buffer.split(b"\n", 1)
            if len(raw) > MAX_LINE_BYTES:
                raise CliError("backend sent an oversized frame", 3)
            message = parse_line(raw.decode("utf-8", errors="replace"))
            if not message:
                continue
            if message.get("type") == "event":
                continue
            if message.get("id") == request_id:
                return message


def transact(path: Path, payload: dict[str, Any], timeout: float) -> dict[str, Any]:
    request = dict(payload)
    request.setdefault("v", PROTOCOL_VERSION)
    request.setdefault("id", int(time.time() * 1000) % 1_000_000_000)
    data = (dumps(request) + "\n").encode("utf-8")
    if len(data) > MAX_LINE_BYTES:
        raise CliError("request is too large", 1)
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        sock.settimeout(timeout)
        sock.connect(str(path))
        sock.sendall(data)
        reply = read_response(sock, request["id"], timeout)
    except FileNotFoundError as exc:
        raise CliError(f"backend socket missing: {path}", 3) from exc
    except ConnectionRefusedError as exc:
        raise CliError(f"backend refused the connection: {path}", 3) from exc
    finally:
        try:
            sock.close()
        except OSError:
            pass
    if not reply.get("ok", True):
        error = reply.get("error") or {}
        message = str(error.get("message") or "Request failed")
        raise CliError(message, 1)
    return reply


def request(path: Path, timeout: float, command: str, **fields: Any) -> dict[str, Any]:
    payload = {"command": command, **fields}
    return transact(path, payload, timeout)


def open_panel(method: str) -> dict[str, Any]:
    completed = subprocess.run(
        ["omarchy", "shell", "-q", f"{PANEL_PROP} {method}"],
        capture_output=True, text=True,
    )
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout or "").strip()
        raise CliError(detail or f"omarchy shell -q {PANEL_PROP} {method} failed", 1)
    return {"ok": True, "result": {"opened": method}}


def play_query(path: Path, timeout: float, query: str) -> dict[str, Any]:
    search = request(path, max(timeout, CATALOG_TIMEOUT), "search",
                     query=query, filter="songs", limit=8)
    item = first_playable(search.get("result") or {})
    if not item:
        raise CliError(f"no playable songs for {query!r}")
    loaded = request(
        path, max(timeout, CATALOG_TIMEOUT), "load",
        video_id=str(item.get("videoId") or ""),
        name=str(item.get("name") or query),
        subtitle=str(item.get("subtitle") or ""),
    )
    result = dict(loaded)
    inner = dict(loaded.get("result") or {})
    inner["picked"] = {
        "name": item.get("name"),
        "subtitle": item.get("subtitle"),
        "videoId": item.get("videoId"),
    }
    result["result"] = inner
    return result


def like_current(path: Path, timeout: float, liked: bool) -> dict[str, Any]:
    state = request(path, timeout, "get_state")
    track = (state.get("result") or {}).get("track") or {}
    video_id = str(track.get("videoId") or "")
    if not video_id:
        raise CliError("nothing playing to like")
    return request(path, timeout, "like", video_id=video_id, liked=liked)


def dispatch(path: Path, timeout: float, command: str, args: list[str]) -> dict[str, Any]:
    name = command.replace("_", "-")
    if name in ("status", "now", "state", "hello", "ping", "get-state"):
        return request(path, timeout, "get_state")
    if name == "health":
        reply = request(path, timeout, "hello")
        inner = dict(reply.get("result") or {})
        inner["socket"] = str(path)
        reply["result"] = inner
        return reply
    if name == "play" and args:
        return play_query(path, timeout, " ".join(args))
    if name in TRANSPORT:
        backend = "previous" if name == "prev" else name
        return request(path, timeout, backend)
    if name == "prev":
        return request(path, timeout, "previous")
    if name == "volume":
        if not args:
            return request(path, timeout, "get_state")
        return request(path, timeout, "set_volume", volume=parse_volume(args[0]))
    if name == "seek":
        if not args:
            raise CliError("seek requires mm:ss or milliseconds")
        return request(path, timeout, "seek", position_ms=parse_position(args[0]))
    if name == "shuffle":
        if not args:
            raise CliError("shuffle requires on or off")
        return request(path, timeout, "set_shuffle", shuffle=parse_bool(args[0]))
    if name == "repeat":
        if not args:
            raise CliError("repeat requires off, one, or all")
        mode = args[0].strip().lower()
        if mode not in {"off", "one", "all"}:
            raise CliError("repeat requires off, one, or all")
        return request(path, timeout, "set_repeat", mode=mode)
    if name == "search":
        if not args:
            raise CliError("search requires a query")
        return request(path, max(timeout, CATALOG_TIMEOUT), "search",
                       query=" ".join(args), limit=12)
    if name in ("play-id", "playid"):
        if not args:
            raise CliError("play-id requires a video id")
        return request(path, max(timeout, CATALOG_TIMEOUT), "load", video_id=args[0])
    if name == "like":
        liked = parse_bool(args[0]) if args else True
        return like_current(path, timeout, liked)
    if name == "unlike":
        return like_current(path, timeout, False)
    if name == "queue":
        return request(path, timeout, "get_queue")
    if name == "browse":
        view = args[0] if args else "home"
        return request(path, max(timeout, CATALOG_TIMEOUT), "browse", view=view)
    if name in ("open", "player"):
        return open_panel("togglePlayer")
    if name in ("mini", "open-mini"):
        return open_panel("toggleMiniPlayer")
    if name == "raw":
        if not args:
            raise CliError("raw requires a backend command name")
        extra: dict[str, Any] = {}
        if len(args) > 1:
            try:
                extra = json.loads(args[1])
            except json.JSONDecodeError as exc:
                raise CliError(f"raw payload must be JSON: {exc}") from exc
            if not isinstance(extra, dict):
                raise CliError("raw payload must be a JSON object")
        return transact(path, {"command": args[0], **extra}, max(timeout, CATALOG_TIMEOUT))
    raise CliError(f"unknown command: {command}")


def render(command: str, reply: dict[str, Any], as_json: bool) -> str:
    if as_json:
        return dumps(reply)
    result = reply.get("result") if isinstance(reply.get("result"), dict) else {}
    name = command.replace("_", "-")
    if name in ("status", "now", "state", "hello", "ping", "get-state", "health",
                "play", "pause", "toggle", "stop", "next", "previous", "prev",
                "volume", "seek", "shuffle", "repeat", "play-id", "playid",
                "like", "unlike"):
        if name == "play" and result.get("picked"):
            picked = result["picked"]
            return f"Playing {track_label(picked)}\n{format_status(result)}"
        return format_status(result) if result else "ok"
    if name == "search":
        return format_search(result)
    if name == "queue":
        return format_queue(result)
    if name in ("open", "player", "mini", "open-mini"):
        return f"ok {result.get('opened') or name}"
    return dumps(reply)


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.version:
        print(f"omarchy-ytmusic {CLI_VERSION} protocol={PROTOCOL_VERSION} backend={BACKEND_VERSION}")
        return 0
    if not args.command:
        parser.print_help()
        return 2
    command = args.command.replace("_", "-")
    if command not in KNOWN_COMMANDS:
        print(f"unknown command: {args.command}", file=sys.stderr)
        return 1
    try:
        path = Path(args.socket) if args.socket else socket_path()
        if command not in ("open", "player", "mini", "open-mini"):
            path = ensure_socket(path, args.start, args.timeout)
        reply = dispatch(path, args.timeout, args.command, list(args.args))
        print(render(args.command, reply, want_json(args)))
        return 0
    except CliError as exc:
        print(str(exc), file=sys.stderr)
        return exc.code


if __name__ == "__main__":
    sys.exit(main())
