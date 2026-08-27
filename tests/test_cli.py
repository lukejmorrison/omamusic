#!/usr/bin/env python3
from __future__ import annotations

import json
import socket
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))

import cli  # noqa: E402
from protocol import dumps, event, response  # noqa: E402


class HelperTests(unittest.TestCase):
    def test_first_playable_prefers_items_with_video_id(self):
        picked = cli.first_playable({
            "items": [{"name": "Skip me"}, {"name": "Song", "subtitle": "A", "videoId": "abc"}],
        })
        self.assertEqual(picked["videoId"], "abc")

    def test_first_playable_walks_sections(self):
        picked = cli.first_playable({
            "items": [],
            "sections": [{"title": "Songs", "items": [{"videoId": "xyz", "name": "Cut"}]}],
        })
        self.assertEqual(picked["videoId"], "xyz")

    def test_parse_position_seconds_and_timestamp(self):
        self.assertEqual(cli.parse_position("90"), 90_000)
        self.assertEqual(cli.parse_position("1:30"), 90_000)
        self.assertEqual(cli.parse_position("90000"), 90_000)

    def test_parse_volume_rejects_out_of_range(self):
        with self.assertRaises(cli.CliError):
            cli.parse_volume("140")

    def test_format_status_includes_now_playing(self):
        text = cli.format_status({
            "playing": True,
            "position_ms": 72000,
            "duration_ms": 185000,
            "volume": 80,
            "shuffle": False,
            "repeat": "off",
            "signed_in": True,
            "lifecycle": "ready",
            "track": {
                "name": "Here Comes The Sun",
                "subtitle": "The Beatles",
                "videoId": "xYz",
            },
        })
        self.assertIn("playing  The Beatles — Here Comes The Sun", text)
        self.assertIn("1:12 / 3:05", text)
        self.assertIn("watch?v=xYz", text)


class FakeBackend:
    def __init__(self, path: Path):
        self.path = path
        self.commands: list[dict] = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        if path.exists():
            path.unlink()
        path.parent.mkdir(parents=True, exist_ok=True)
        self.server.bind(str(path))
        self.server.listen(2)
        self.server.settimeout(0.2)

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        try:
            self.server.close()
        except OSError:
            pass
        self._thread.join(timeout=2)
        if self.path.exists():
            self.path.unlink()

    def _run(self) -> None:
        while not self._stop.is_set():
            try:
                client, _ = self.server.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            threading.Thread(target=self._client, args=(client,), daemon=True).start()

    def _client(self, client: socket.socket) -> None:
        try:
            client.sendall((dumps(event("state_changed", {"lifecycle": "ready"})) + "\n").encode())
            client.sendall((dumps(event("spectrum", {"bands": [0.1] * 10})) + "\n").encode())
            buffer = b""
            while not self._stop.is_set():
                chunk = client.recv(65536)
                if not chunk:
                    break
                buffer += chunk
                while b"\n" in buffer:
                    raw, buffer = buffer.split(b"\n", 1)
                    message = json.loads(raw.decode())
                    self.commands.append(message)
                    reply = self.handle(message)
                    client.sendall((dumps(reply) + "\n").encode())
        except OSError:
            pass
        finally:
            try:
                client.close()
            except OSError:
                pass

    def handle(self, message: dict) -> dict:
        command = message.get("command")
        request_id = message.get("id")
        if command == "search":
            return response(request_id, True, {
                "items": [{
                    "type": "track",
                    "name": "Here Comes The Sun",
                    "subtitle": "The Beatles",
                    "videoId": "sun123",
                }],
            })
        if command == "load":
            return response(request_id, True, {
                "playing": True,
                "lifecycle": "ready",
                "signed_in": True,
                "volume": 80,
                "position_ms": 0,
                "duration_ms": 185000,
                "track": {
                    "name": message.get("name") or "Here Comes The Sun",
                    "subtitle": message.get("subtitle") or "The Beatles",
                    "videoId": message.get("video_id"),
                },
            })
        if command in ("hello", "get_state", "play", "pause"):
            return response(request_id, True, {
                "playing": command != "pause",
                "lifecycle": "ready",
                "signed_in": True,
                "volume": 80,
                "position_ms": 1000,
                "duration_ms": 185000,
                "track": {"name": "Song", "subtitle": "Artist", "videoId": "abc"},
            })
        return response(request_id, False, code="invalid_request", message=f"unknown {command}")


class SocketClientTests(unittest.TestCase):
    def setUp(self):
        self._runtime = tempfile.TemporaryDirectory()
        self.path = Path(self._runtime.name) / "backend.sock"
        self.backend = FakeBackend(self.path)
        self.backend.start()
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline and not self.path.exists():
            time.sleep(0.01)

    def tearDown(self):
        self.backend.stop()
        self._runtime.cleanup()

    def test_status_skips_events_and_returns_state(self):
        reply = cli.request(self.path, 2.0, "get_state")
        self.assertTrue(reply["ok"])
        self.assertEqual(reply["result"]["track"]["name"], "Song")

    def test_play_query_searches_then_loads(self):
        reply = cli.play_query(self.path, 2.0, "here comes the sun")
        commands = [item["command"] for item in self.backend.commands]
        self.assertEqual(commands, ["search", "load"])
        self.assertEqual(self.backend.commands[1]["video_id"], "sun123")
        self.assertEqual(reply["result"]["picked"]["videoId"], "sun123")

    def test_main_prints_human_status(self):
        code = cli.main(["--human", "--no-start", "--socket", str(self.path), "status"])
        self.assertEqual(code, 0)


class MainTests(unittest.TestCase):
    def test_unknown_command_exits_one(self):
        code = cli.main(["--no-start", "--socket", "/tmp/missing-ytmusic.sock", "nope"])
        self.assertEqual(code, 1)

    def test_missing_socket_without_start_exits_three(self):
        code = cli.main(["--no-start", "--socket", "/tmp/definitely-missing-ytmusic.sock", "status"])
        self.assertEqual(code, 3)

    def test_help_is_usage(self):
        code = cli.main([])
        self.assertEqual(code, 2)


if __name__ == "__main__":
    unittest.main()
