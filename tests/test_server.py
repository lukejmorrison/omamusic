#!/usr/bin/env python3
from __future__ import annotations

import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))

import auth  # noqa: E402
import player  # noqa: E402
import queue_session  # noqa: E402
from catalog import AuthRequired, CatalogError  # noqa: E402
from protocol import ERROR_AUTH  # noqa: E402
from server import AuthError, Backend, idle_should_exit  # noqa: E402


class IsolatedConfigMixin:
    def _isolate_config(self):
        self._config = tempfile.TemporaryDirectory()
        self._previous_config_dir = auth.CONFIG_DIR
        auth.CONFIG_DIR = Path(self._config.name) / "omamusic"

    def _restore_config(self):
        auth.CONFIG_DIR = self._previous_config_dir
        self._config.cleanup()


class IdleWatchTests(unittest.TestCase):
    def test_idle_exit_requires_minutes_and_silence(self):
        now = 1_000.0
        self.assertFalse(idle_should_exit(
            idle_minutes=15, playing=False, client_count=0,
            last_activity=now, now=now))
        self.assertTrue(idle_should_exit(
            idle_minutes=15, playing=False, client_count=0,
            last_activity=now - 15 * 60, now=now))

    def test_idle_exit_skips_playing_and_connected_clients(self):
        now = 1_000.0
        self.assertFalse(idle_should_exit(
            idle_minutes=15, playing=True, client_count=0,
            last_activity=now - 15 * 60, now=now))
        self.assertFalse(idle_should_exit(
            idle_minutes=15, playing=False, client_count=1,
            last_activity=now - 15 * 60, now=now))
        self.assertFalse(idle_should_exit(
            idle_minutes=0, playing=False, client_count=0,
            last_activity=now - 15 * 60, now=now))


class StreamCacheWarmUpTests(IsolatedConfigMixin, unittest.TestCase):
    """Solving YouTube's player JS challenge before the first play."""

    def setUp(self):
        self._runtime = tempfile.TemporaryDirectory()
        self._previous = os.environ.get("XDG_RUNTIME_DIR")
        os.environ["XDG_RUNTIME_DIR"] = self._runtime.name
        self._isolate_config()
        self.backend = Backend(Path(self._runtime.name) / "absent.json")

    def tearDown(self):
        if self._previous is None:
            os.environ.pop("XDG_RUNTIME_DIR", None)
        else:
            os.environ["XDG_RUNTIME_DIR"] = self._previous
        self._restore_config()
        self._runtime.cleanup()

    def test_warm_up_is_skipped_when_the_cache_is_already_warm(self):
        with mock.patch.object(player, "yt_dlp_cache_warm", return_value=True), \
             mock.patch.object(self.backend, "_catalog_video_id") as picker:
            self.backend._warm_stream_cache()
        picker.assert_not_called()

    def test_warm_up_uses_a_catalog_video_not_a_hardcoded_one(self):
        with mock.patch.object(player, "yt_dlp_cache_warm", return_value=False), \
             mock.patch.object(self.backend, "_catalog_video_id",
                               return_value="dQw4w9wgkcQ") as picker, \
             mock.patch.object(self.backend.player.resolver, "resolve") as resolve:
            self.backend._warm_stream_cache()
            for _ in range(50):
                if picker.called and resolve.called:
                    break
                time.sleep(0.02)
        picker.assert_called_once_with()
        resolve.assert_called_once_with("dQw4w9wgkcQ")

    def test_state_reports_whether_a_resolve_is_in_flight(self):
        self.assertIs(self.backend.state()["resolving"], False)
        self.backend.player.resolving = True
        self.assertIs(self.backend.state()["resolving"], True)

    def test_state_reports_auth_kind_and_oauth_fields(self):
        state = self.backend.state()
        self.assertEqual(state["auth_kind"], "none")
        self.assertEqual(state["oauth_status"], "idle")
        self.assertEqual(state["oauth_user_code"], "")
        self.assertNotIn("oauth_device_code", state)


class LikeAuthTests(IsolatedConfigMixin, unittest.TestCase):
    def setUp(self):
        self._runtime = tempfile.TemporaryDirectory()
        self._previous = os.environ.get("XDG_RUNTIME_DIR")
        os.environ["XDG_RUNTIME_DIR"] = self._runtime.name
        self._isolate_config()
        self.backend = Backend(Path(self._runtime.name) / "absent.json")

    def tearDown(self):
        if self._previous is None:
            os.environ.pop("XDG_RUNTIME_DIR", None)
        else:
            os.environ["XDG_RUNTIME_DIR"] = self._previous
        self._restore_config()
        self._runtime.cleanup()

    def test_like_without_session_asks_to_sign_in(self):
        self.backend.signed_in = False
        with self.assertRaises(AuthError) as raised:
            self.backend.like("abcdefghijk", True)
        self.assertEqual(str(raised.exception), "Sign in to like songs")

    def test_like_playlist_without_session_asks_to_sign_in(self):
        self.backend.signed_in = False
        with self.assertRaises(AuthError) as raised:
            self.backend.like_playlist("OLAK5uy_abc", True)
        self.assertEqual(str(raised.exception), "Sign in to like albums")


class PlaybackCommandTests(IsolatedConfigMixin, unittest.TestCase):
    def setUp(self):
        self._runtime = tempfile.TemporaryDirectory()
        self._previous = os.environ.get("XDG_RUNTIME_DIR")
        os.environ["XDG_RUNTIME_DIR"] = self._runtime.name
        self._isolate_config()
        self.backend = Backend(Path(self._runtime.name) / "absent.json")
        self.track = lambda vid, name: {
            "type": "track",
            "videoId": vid,
            "name": name,
            "subtitle": "Artist",
        }
        self.backend.player.queue = [
            self.track("aaa", "First"),
            self.track("bbb", "Second"),
            self.track("ccc", "Third"),
        ]
        self.backend.player.index = 1

    def tearDown(self):
        if self._previous is None:
            os.environ.pop("XDG_RUNTIME_DIR", None)
        else:
            os.environ["XDG_RUNTIME_DIR"] = self._previous
        self._restore_config()
        self._runtime.cleanup()

    def test_reorder_queue_moves_items_and_keeps_now_playing_index(self):
        with mock.patch.object(self.backend.player, "apply_eq"):
            result = self.backend.dispatch("reorder_queue", {
                "source_index": 0,
                "destination_index": 2,
            })
        ids = [item["videoId"] for item in result["queue"]]
        self.assertEqual(ids, ["bbb", "ccc", "aaa"])
        self.assertEqual(self.backend.player.index, 0)
        self.assertEqual(self.backend.player.current["videoId"], "bbb")

    def test_set_eq_preset_applies_cliamp_curve(self):
        with mock.patch.object(self.backend.player, "apply_eq") as apply_eq:
            snapshot = self.backend.dispatch("set_eq_preset", {"name": "Rock"})
        self.assertEqual(snapshot["preset"], "Rock")
        self.assertEqual(snapshot["bands"], list(player.EQ_PRESETS["Rock"]))
        apply_eq.assert_called_once()

    def test_restore_eq_reloads_custom_bands(self):
        with mock.patch.object(self.backend.player, "apply_eq"):
            snapshot = self.backend.dispatch("restore_eq", {
                "preset": "Custom",
                "bands": [4, 0, -2],
            })
        self.assertEqual(snapshot["preset"], "Custom")
        self.assertEqual(snapshot["bands"][0], 4.0)
        self.assertEqual(snapshot["bands"][2], -2.0)
        self.assertEqual(len(snapshot["bands"]), 10)

    def test_get_playlist_uses_item_id_not_request_id(self):
        catalog = mock.Mock()
        catalog.playlist.return_value = {"type": "playlist", "name": "Liked Music", "tracks": []}
        with mock.patch.object(self.backend, "require_catalog", return_value=catalog):
            reply = self.backend.handle({
                "v": 1,
                "id": 7,
                "command": "get_playlist",
                "item_id": "LM",
            })
        self.assertEqual(reply["id"], 7)
        self.assertTrue(reply["ok"])
        catalog.playlist.assert_called_once_with("LM")

    def test_load_album_playlist_id_uses_playlist_not_get_album(self):
        catalog = mock.Mock()
        catalog.playlist.return_value = {
            "tracks": [{"videoId": "aaa", "type": "track", "name": "One"}],
        }
        catalog.album.side_effect = CatalogError(
            "Invalid album browseId provided, must start with MPRE.")
        with mock.patch.object(self.backend, "require_catalog", return_value=catalog), \
             mock.patch.object(self.backend.player, "load") as player_load:
            self.backend.load({"album_id": "OLAK5uy_abc"})
        catalog.playlist.assert_called_once_with("OLAK5uy_abc")
        catalog.album.assert_not_called()
        player_load.assert_called_once()

    def test_browse_liked_auth_error_asks_to_sign_in(self):
        catalog = mock.Mock()
        catalog.liked.side_effect = AuthRequired("Sign in to see liked songs")
        with mock.patch.object(self.backend, "require_catalog", return_value=catalog):
            reply = self.backend.handle({
                "v": 1,
                "id": 4,
                "command": "browse",
                "view": "liked",
            })
        self.assertFalse(reply["ok"])
        self.assertEqual(reply["error"]["code"], ERROR_AUTH)
        self.assertEqual(reply["error"]["message"], "Sign in to see liked songs")


class QueuePersistTests(IsolatedConfigMixin, unittest.TestCase):
    def setUp(self):
        self._runtime = tempfile.TemporaryDirectory()
        self._previous = os.environ.get("XDG_RUNTIME_DIR")
        os.environ["XDG_RUNTIME_DIR"] = self._runtime.name
        self._isolate_config()

    def tearDown(self):
        if self._previous is None:
            os.environ.pop("XDG_RUNTIME_DIR", None)
        else:
            os.environ["XDG_RUNTIME_DIR"] = self._previous
        self._restore_config()
        self._runtime.cleanup()

    def test_backend_restores_saved_queue_on_start(self):
        queue_session.save({
            "items": [{"videoId": "aaa", "name": "Saved"}],
            "index": 0,
            "shuffle": False,
            "repeat": "off",
            "position_ms": 0,
        })
        backend = Backend(Path(self._runtime.name) / "absent.json")
        self.assertEqual(backend.player.current["videoId"], "aaa")
        self.assertEqual(backend.state()["track"]["name"], "Saved")
        self.assertFalse(backend.state()["playing"])

    def test_backend_falls_back_to_last_history_when_queue_is_missing(self):
        backend = Backend(Path(self._runtime.name) / "absent.json")
        backend.local_history = [{"videoId": "hist", "name": "Last play"}]
        backend._restore_queue_session()
        self.assertEqual(backend.player.current["videoId"], "hist")
        self.assertEqual(backend.state()["track"]["name"], "Last play")

    def test_remember_queue_writes_the_current_session(self):
        backend = Backend(Path(self._runtime.name) / "absent.json")
        backend.player.queue = [{"videoId": "aaa", "name": "First"}]
        backend.player.index = 0
        backend.player.shuffle = True
        backend.player.playing = True
        backend.player.position_ms = 9000
        backend._remember_queue()
        loaded = queue_session.load()
        self.assertEqual(loaded["items"][0]["videoId"], "aaa")
        self.assertEqual(loaded["index"], 0)
        self.assertTrue(loaded["shuffle"])
        self.assertTrue(loaded["playing"])
        self.assertEqual(loaded["position_ms"], 9000)

    def test_backend_resumes_when_the_saved_session_was_playing(self):
        queue_session.save({
            "items": [{"videoId": "aaa", "name": "Saved"}],
            "index": 0,
            "playing": True,
            "position_ms": 4000,
        })
        backend = Backend(Path(self._runtime.name) / "absent.json")
        with mock.patch.object(backend.player, "play") as play:
            backend._resume_queue_session()
        play.assert_called_once_with()
        self.assertEqual(backend.player.current["videoId"], "aaa")


if __name__ == "__main__":
    unittest.main()
