# Technical notes

This plugin is a fork of
[rlimberger/omarchy-ytmusic](https://github.com/rlimberger/omarchy-ytmusic)
by [rlimberger](https://github.com/rlimberger). The shell layout, plugin
kinds, and much of the player UI started from
[Omarchy Spotify](https://github.com/stappmus/Omarchy-Spotify) in his tree.
EQ bands, spectrum, and the bar now-playing pill follow
[cliamp](https://github.com/bjarneo/cliamp). Lyrics open
[Omasing](https://github.com/stappmus/Omasing). Catalog access and local
playback are YouTube Music specific. See [README credits](../README.md#credits).

## Architecture

Omarchy YouTube Music runs as a plugin inside Omarchy's existing `omarchy-shell`
Quickshell process. It provides a shared service, a bar widget, and a
lazy-loaded panel. There is no embedded website or browser engine.

Catalog data uses the unofficial [`ytmusicapi`](https://github.com/sigma67/ytmusicapi)
client. Local audio is **mpv**, with stream URLs from **yt-dlp**. mpv is launched
headless (`--vo=null`, no Wayland/X display) and uses the D-Bus-safe client
name `omarchy-ytmusic` so MPRIS cannot stall the player. Each track sets
`force-media-title` so MPRIS clients show the song name, not the stream URL.
The plugin
talks to a private Unix socket using versioned newline-delimited JSON. When
`omamusic` is installed that socket is
`$XDG_RUNTIME_DIR/omamusic/backend.sock` (`omamusic.service`). Otherwise it
stays `$XDG_RUNTIME_DIR/omarchy-ytmusic/backend.sock`.

The backend is supervised by a static systemd user unit that
is never enabled at login. `omamusic` is the default daemon (Rust).
`scripts/setup.sh` downloads the binary pinned in
`scripts/backend-release` from GitHub Releases, verifies `SHA256SUMS`,
and only then tries `cargo` or the Python port. The plugin
starts the unit when a UI is visible or you press play, and the backend
exits after the configured idle period. The play queue is written to
`play-queue.json` while you listen and on shutdown. A restart restores that
queue, track, and position, and resumes playback when the saved session was
playing. If that file is missing, the last local play from
`play-history.json` is restored paused.

Omarchy hot-reloads plugins on any write inside their directory, so the
installed backend lives outside the plugin tree. Cargo's target dir is
`$HOME/.cache/omamusic/target`. Runtime paths:

- `$HOME/.local/bin/omamusic` and `$HOME/.config/systemd/user/omamusic.service`
- `$HOME/.config/omamusic/browser.json`
- `$HOME/.config/omamusic/play-history.json`
- `$HOME/.config/omamusic/play-queue.json`
- `$XDG_RUNTIME_DIR/omamusic/backend.sock`

Legacy Python fallback (no cargo):

- `$HOME/.local/share/omarchy-ytmusic/venv`
- `$HOME/.local/lib/omarchy-ytmusic/`
- `$HOME/.config/omarchy-ytmusic/browser.json`

## Stream resolution

Playback URLs come from `yt-dlp -g`. The first resolve against a new YouTube
player build has to fetch and solve a JS challenge, which is far slower than a
warm resolve, so the budget depends on whether
`~/.cache/yt-dlp/youtube-sigfuncs/` is populated: 40s warm, 150s cold. The
backend also warms that challenge in the background at startup, picking a
`videoId` out of history, liked songs, or home shelves rather than hardcoding a
video that could later be taken down or region-blocked.

While a resolve is in flight the state carries `resolving: true`, which the
panel shows as "Preparing playback…".

## Protocol

Requests:

```json
{"v":1,"id":7,"command":"pause"}
```

Successful responses keep that id. Failures set `ok` to false with a stable
error code. The server pushes `state_changed` on connection and whenever
playback state changes. While audio is playing it also pushes a small
`spectrum` event (~20 Hz) with ten 0–1 band levels. Those events do not
replace `state_changed`.

```json
{"type":"event","v":1,"event":"spectrum","bands":[0.12,0.4,0.8,0.55,0.2,0.1,0.08,0.04,0.02,0.01]}
```

Band edges match cliamp (`20,100,200,400,800,1600,3200,6400,12800,16000,20000`
Hz). Capture is `parec` on the default Pulse/PipeWire sink `.monitor`, so the
bars show post-EQ output rather than the pre-filter stream.

Commands include `hello`, `setup_auth`, `import_browser`, `logout`, `play`, `pause`, `toggle`,
`next`, `previous`, `seek`, `set_volume`, `set_shuffle`, `set_repeat`, `load`,
`add_to_queue`, `search`, `browse`, `get_playlist`, `get_album`, `get_artist`,
`like`, `create_playlist`, `add_to_playlist`, and `sleep`.

## Authentication

The usual sign-in path copies the YouTube Music session already in Chromium
(or Chrome/Brave) on this computer: decrypt the browser cookie database with
the libsecret OSCrypt key, then write `ytmusicapi` headers with
`ytmusicapi.setup()`. Pasting request headers is still supported as a
fallback. Cookies are exported to a Netscape cookie file so yt-dlp can
resolve member-only or region-locked streams when the session allows it.
SAPISIDHASH is recomputed from those cookies on backend start and at least
every five minutes so a day-old snapshot does not look signed-out.

## Local development

```bash
./scripts/install-local.sh
./scripts/test.sh
./scripts/reload.sh
```

Plugin QML usually hot-reloads on save, but `.pragma library` files
(`Api.js`) and the installed daemon do not. `scripts/reload.sh` runs
`setup.sh` (rebuild `omamusic` into `~/.cache/omamusic/target`, or copy
`backend/*.py` when using Python), restarts the selected user unit,
restarts Omarchy shell, and reopens the player. `--backend-only` skips
the shell restart. `--no-open` leaves the window closed.

Complete removal:

```bash
./scripts/remove-runtime.sh --purge
omarchy plugin remove wizwam.omamusic --yes
```
