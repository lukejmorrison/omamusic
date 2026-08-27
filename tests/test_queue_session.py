#!/usr/bin/env python3
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))

import queue_session  # noqa: E402


class QueueSessionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / "play-queue.json"

    def tearDown(self):
        self.tmp.cleanup()

    def test_missing_file_is_absent_not_empty(self):
        self.assertIsNone(queue_session.load(self.path))

    def test_roundtrip_keeps_current_track(self):
        payload = {
            "items": [
                {"videoId": "aaa", "name": "One"},
                {"videoId": "bbb", "name": "Two"},
            ],
            "index": 1,
            "shuffle": True,
            "repeat": "context",
            "position_ms": 12345,
            "playing": True,
        }
        queue_session.save(payload, self.path)
        loaded = queue_session.load(self.path)
        self.assertEqual(loaded["index"], 1)
        self.assertEqual(loaded["items"][1]["videoId"], "bbb")
        self.assertTrue(loaded["shuffle"])
        self.assertEqual(loaded["repeat"], "context")
        self.assertEqual(loaded["position_ms"], 12345)
        self.assertTrue(loaded["playing"])

    def test_clip_keeps_current_when_queue_is_long(self):
        items = [{"videoId": f"v{i}", "name": str(i)} for i in range(120)]
        saved = queue_session.save({"items": items, "index": 90}, self.path)
        loaded = queue_session.load(self.path)
        self.assertLessEqual(len(loaded["items"]), queue_session.MAX_ITEMS)
        self.assertEqual(loaded["items"][loaded["index"]]["videoId"], "v90")
        self.assertEqual(saved["items"][saved["index"]]["videoId"], "v90")

    def test_corrupt_file_is_absent(self):
        self.path.write_text("{not json", encoding="utf-8")
        self.assertIsNone(queue_session.load(self.path))


if __name__ == "__main__":
    unittest.main()
