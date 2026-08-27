#!/usr/bin/env python3
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))

import json
import threading
import time

from protocol import (  # noqa: E402
    ERROR_UNSUPPORTED_VERSION,
    MAX_LINE_BYTES,
    encode_line,
    parse_line,
    redact,
    response,
    spectrum_event,
    write_locked,
)


class ProtocolTests(unittest.TestCase):
    def test_parse_line_roundtrip(self):
        self.assertEqual(parse_line('{"command":"ping","id":1,"v":1}')["command"], "ping")
        self.assertIsNone(parse_line(""))
        self.assertIsNone(parse_line("not-json"))

    def test_response_ok_and_error(self):
        ok = response(7, True, {"lifecycle": "ready"})
        self.assertTrue(ok["ok"])
        self.assertEqual(ok["id"], 7)
        self.assertEqual(ok["result"]["lifecycle"], "ready")
        err = response(8, False, code=ERROR_UNSUPPORTED_VERSION, message="nope")
        self.assertFalse(err["ok"])
        self.assertEqual(err["error"]["code"], ERROR_UNSUPPORTED_VERSION)

    def test_parse_line_rejects_oversized_frames(self):
        huge = '{"command":"ping","pad":"' + ("x" * (MAX_LINE_BYTES + 8)) + '"}'
        self.assertIsNone(parse_line(huge))

    def test_encode_line_stays_under_the_ceiling(self):
        payload = {
            "type": "event",
            "state": {
                "track": {"name": "Song"},
                "queue": [{"name": "x" * 8000} for _ in range(80)],
                "play_history": [{"name": "y" * 8000} for _ in range(80)],
            },
        }
        encoded = encode_line(payload, max_bytes=4096)
        self.assertLessEqual(len(encoded), 4096)
        self.assertTrue(encoded.endswith(b"\n"))

    def test_spectrum_event_shape(self):
        payload = spectrum_event([0.5] * 10)
        self.assertEqual(payload["event"], "spectrum")
        self.assertEqual(len(payload["bands"]), 10)
        self.assertTrue(all(band == 0.5 for band in payload["bands"]))

    def test_redact_cookie_and_authorization(self):
        text = redact("cookie: SID=supersecret authorization: Bearer abc")
        self.assertNotIn("supersecret", text)
        self.assertNotIn("Bearer abc", text)
        self.assertIn("<redacted>", text)

    def test_write_locked_keeps_ndjson_lines_intact(self):
        class Sock:
            def __init__(self):
                self.buf = bytearray()

            def sendall(self, data):
                for byte in data:
                    self.buf.append(byte)
                    time.sleep(0)

        sock = Sock()
        lock = threading.Lock()
        frames = [
            b'{"type":"response","id":1,"pad":"' + (b"A" * 80) + b'"}\n',
            b'{"type":"event","event":"spectrum","bands":[0.1,0.2]}\n',
        ]

        def write(frame):
            write_locked(lock, sock, frame)

        workers = [threading.Thread(target=write, args=(frame,)) for frame in frames]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join()
        lines = bytes(sock.buf).decode().splitlines()
        self.assertEqual(len(lines), 2)
        parsed = [json.loads(line) for line in lines]
        kinds = sorted(item.get("type") for item in parsed)
        self.assertEqual(kinds, ["event", "response"])


if __name__ == "__main__":
    unittest.main()
