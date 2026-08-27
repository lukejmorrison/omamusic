#!/usr/bin/env python3
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))

from unittest import mock

from catalog import (  # noqa: E402
    AuthRequired,
    Catalog,
    CatalogError,
    context_item,
    duration_ms,
    looks_unauthorized,
    map_items,
    thumbnail_url,
    track_item,
)


class CatalogTests(unittest.TestCase):
    def test_duration_parses_clock_and_seconds(self):
        self.assertEqual(duration_ms({"duration": "3:45"}), 225000)
        self.assertEqual(duration_ms({"duration": "1:02:03"}), 3723000)
        self.assertEqual(duration_ms({"duration_seconds": 90}), 90000)
        self.assertEqual(duration_ms({}), 0)

    def test_track_item_normalizes_song(self):
        item = track_item({
            "title": "Under the Bridge",
            "videoId": "GLvqBAudoEg",
            "artists": [{"name": "Red Hot Chili Peppers", "id": "UC123"}],
            "album": {"name": "Blood Sugar Sex Magik", "id": "MPREb_album"},
            "duration": "4:24",
            "thumbnails": [{"url": "https://img/small.jpg", "width": 60},
                           {"url": "https://img/large.jpg", "width": 544}],
            "likeStatus": "LIKE",
        })
        self.assertIsNotNone(item)
        self.assertEqual(item["type"], "track")
        self.assertEqual(item["kind"], "item")
        self.assertEqual(item["uri"], "ytm:track:GLvqBAudoEg")
        self.assertEqual(item["subtitle"], "Red Hot Chili Peppers")
        self.assertEqual(item["album"], "Blood Sugar Sex Magik")
        self.assertTrue(item["liked"])
        self.assertEqual(item["imageUrl"], "https://img/large.jpg")
        self.assertEqual(item["durationMs"], 264000)
        self.assertEqual(item["albumItem"]["type"], "album")
        self.assertEqual(item["albumItem"]["id"], "MPREb_album")
        self.assertEqual(item["albumItem"]["externalUrl"],
                         "https://music.youtube.com/browse/MPREb_album")

    def test_album_item_keeps_playlist_id(self):
        item = track_item({
            "title": "Song",
            "videoId": "abcdefghijk",
            "album": {
                "name": "Record",
                "id": "MPREb_record",
                "audioPlaylistId": "OLAK5uy_abc",
            },
        })
        self.assertEqual(item["albumItem"]["playlistId"], "OLAK5uy_abc")
        self.assertIn("browse/MPREb_record", item["albumItem"]["externalUrl"])

    def test_context_item_playlist(self):
        item = context_item({
            "title": "Liked Music",
            "playlistId": "LM",
            "count": 12,
        }, "playlist")
        self.assertEqual(item["type"], "playlist")
        self.assertEqual(item["kind"], "context")
        self.assertIn("12 songs", item["subtitle"])

    def test_map_items_skips_junk(self):
        rows = map_items([
            None,
            {"title": "Nope"},
            {"title": "Song", "videoId": "abcdefghijk"},
        ])
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["videoId"], "abcdefghijk")

    def test_thumbnail_prefers_wide_image(self):
        url = thumbnail_url({
            "thumbnails": [
                {"url": "a", "width": 60},
                {"url": "b", "width": 226},
            ]
        })
        self.assertEqual(url, "b")

    def test_looks_unauthorized_catches_ytmusicapi_401(self):
        self.assertTrue(looks_unauthorized(
            "Server returned HTTP 401: Unauthorized. "
            "You must be signed in to perform this operation."))
        self.assertFalse(looks_unauthorized("connection timed out"))

    def test_home_caches_until_forced(self):
        yt = mock.Mock()
        yt.get_home.return_value = [{
            "title": "That summer feeling",
            "contents": [{"title": "Song", "videoId": "abcdefghijk"}],
        }]
        catalog = Catalog(yt)
        first = catalog.home()
        second = catalog.home()
        self.assertEqual(len(first), 1)
        self.assertEqual(first[0]["title"], "That summer feeling")
        self.assertEqual(first, second)
        yt.get_home.assert_called_once()
        catalog.home(force=True)
        self.assertEqual(yt.get_home.call_count, 2)

    def test_rate_song_maps_401_to_sign_in(self):
        catalog = Catalog(mock.Mock())
        catalog.yt.rate_song.side_effect = Exception(
            "Server returned HTTP 401: Unauthorized. "
            "You must be signed in to perform this operation.")
        with self.assertRaises(AuthRequired) as raised:
            catalog.rate_song("abcdefghijk", True)
        self.assertEqual(str(raised.exception), "Sign in to like songs")

    def test_liked_maps_401_to_sign_in(self):
        yt = mock.Mock()
        yt.get_liked_songs.side_effect = Exception(
            "Server returned HTTP 401: Unauthorized. "
            "You must be signed in to perform this operation.")
        with self.assertRaises(AuthRequired) as raised:
            Catalog(yt).liked()
        self.assertEqual(str(raised.exception), "Sign in to see liked songs")

    def test_playlists_maps_401_to_sign_in(self):
        yt = mock.Mock()
        yt.get_library_playlists.side_effect = Exception(
            "Server returned HTTP 401: Unauthorized.")
        with self.assertRaises(AuthRequired) as raised:
            Catalog(yt).playlists()
        self.assertEqual(str(raised.exception), "Sign in to see playlists")

    def test_liked_does_not_hide_other_errors(self):
        yt = mock.Mock()
        yt.get_liked_songs.side_effect = Exception("connection timed out")
        with self.assertRaises(CatalogError) as raised:
            Catalog(yt).liked()
        self.assertIn("timed out", str(raised.exception))

    def test_playlist_loads_a_page_not_the_whole_library(self):
        yt = mock.Mock()
        yt.get_playlist.return_value = {"title": "Bangers", "tracks": []}
        Catalog(yt).playlist("PLabc")
        yt.get_playlist.assert_called_once_with("PLabc", limit=80)

    def test_album_keeps_browse_id_and_stamps_tracks(self):
        yt = mock.Mock()
        yt.get_album.return_value = {
            "title": "Rooted Through Darkness",
            "artists": [{"name": "Kate Sutherland", "id": "UC123"}],
            "audioPlaylistId": "OLAK5uy_abc",
            "thumbnails": [{"url": "https://img/cover.jpg", "width": 226}],
            "tracks": [{
                "title": "Glorious Day",
                "videoId": "abcdefghijk",
                "album": "Rooted Through Darkness",
                "artists": [{"name": "Kate Sutherland", "id": "UC123"}],
            }],
        }
        result = Catalog(yt).album("MPREb_rooted")
        self.assertEqual(result["id"], "MPREb_rooted")
        self.assertEqual(result["playlistId"], "OLAK5uy_abc")
        self.assertEqual(result["type"], "album")
        track = result["tracks"][0]
        self.assertEqual(track["albumItem"]["id"], "MPREb_rooted")
        self.assertEqual(track["albumItem"]["playlistId"], "OLAK5uy_abc")
        self.assertTrue(track["albumItem"]["imageUrl"])
        self.assertEqual(track["album"], "Rooted Through Darkness")

    def test_playlist_keeps_requested_id_when_raw_omits_it(self):
        yt = mock.Mock()
        yt.get_playlist.return_value = {
            "title": "Kate Sutherland",
            "tracks": [{"title": "Song", "videoId": "abcdefghijk"}],
        }
        result = Catalog(yt).playlist("VLPLQ6abc")
        self.assertEqual(result["id"], "VLPLQ6abc")
        self.assertEqual(result["playlistId"], "VLPLQ6abc")
        self.assertEqual(result["type"], "playlist")
        self.assertEqual(result["tracks"][0]["videoId"], "abcdefghijk")


if __name__ == "__main__":
    unittest.main()
