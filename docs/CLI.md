# `omarchy-ytmusic` CLI — how to use it

Index on the [repo homepage](https://github.com/lukejmorrison/omamusic).

A small command for **this computer’s** YouTube Music player. Agents (Hermes,
OpenClaw, Grok, a local LLM) should use this instead of inventing HTTP or
driving Chromium.

It talks to the same Unix socket the GUI uses. It does **not** call the
YouTube Data API. For playlist OAuth on Google’s API, see the
`google-services` skill.

The GUI is documented in [USER.md](USER.md).

## Install

`scripts/setup.sh` installs:

- `~/.local/bin/omamusic` (when cargo is present)
- `~/.local/bin/omarchy-ytmusic` (shim that prefers `omamusic`)

For Grok / Hermes / OpenClaw skill links:

```bash
./plugins/ytmusic/scripts/install-agent-skill.sh
```

If the command is missing, run setup from the plugin directory, then open the
player once with Super+Shift+M so the user unit exists.

## How it works

```
omarchy-ytmusic  (or omamusic)
  → NDJSON on $XDG_RUNTIME_DIR/omamusic/backend.sock
    → omamusic.service (mpv + yt-dlp)
```

Without cargo, the legacy Python socket and unit stay
`$XDG_RUNTIME_DIR/omarchy-ytmusic/backend.sock` and
`omarchy-ytmusic.service`.

1. If the socket is missing, the CLI starts the selected user unit
   (`--no-start` skips that). The unit is still not enabled at login.
2. One request line: `{"v":1,"id":…,"command":"pause"}`.
3. It skips `event` frames (`state_changed`, `spectrum`) and waits for the
   matching `id`.
4. Frames are capped at 256 KiB.

`--json` prints the backend reply (default when stdout is not a TTY). In a
terminal, you get a short now-playing line unless you pass `--json`.

Agents: always pass `--json`. Only treat `"ok":true` as success.

Exit codes: `0` ok, `1` command/backend error, `2` usage, `3` backend down.

Opening the window is **not** the socket:

```bash
omarchy-ytmusic open      # full player
omarchy-ytmusic mini      # mini player
# same as:
omarchy shell -q wizwam.omamusic.player togglePlayer
```

## Everyday commands

```bash
omarchy-ytmusic --json status
omarchy-ytmusic --json play                 # resume
omarchy-ytmusic --json play technologic     # search songs, play first hit
omarchy-ytmusic --json pause
omarchy-ytmusic --json next
omarchy-ytmusic --json prev
omarchy-ytmusic --json volume 40
omarchy-ytmusic --json seek 1:30
omarchy-ytmusic --json shuffle on
omarchy-ytmusic --json repeat all
omarchy-ytmusic --json search city pop
omarchy-ytmusic --json play-id R5uHYAIkzgU
omarchy-ytmusic --json like
omarchy-ytmusic --json unlike
omarchy-ytmusic --json queue
omarchy-ytmusic --json browse playlists
omarchy-ytmusic --json health
```

`play QUERY` searches **songs** and loads the first `videoId`. Bare `play`
only resumes.

## Playlists and library

Your playlists are a browse, then a `load`:

```bash
omarchy-ytmusic --json browse playlists
omarchy-ytmusic --json raw load '{"playlist_id":"PLJFpg2A87Hzc"}'
```

`browse` views include `home`, `playlists`, and other library shelves the
backend already serves. Likes and private lists need the Chromium session.

`raw` is the escape hatch for any socket command (EQ, sleep, queue reorder,
album like). Examples:

```bash
omarchy-ytmusic --json raw set_eq_preset '{"name":"Rock"}'
omarchy-ytmusic --json raw get_playlist '{"item_id":"LM"}'
omarchy-ytmusic --json raw sleep '{"minutes":30}'
```

Use `item_id` for `get_playlist` / `get_album` / `get_artist`, not `id`
(that field is the request id).

## Flags

| Flag | Meaning |
| --- | --- |
| `--json` | Print the backend JSON |
| `--human` | Pretty text even when stdout is not a TTY |
| `--no-start` | Fail if the socket is missing |
| `--timeout N` | Seconds to wait (default 15; catalog uses 45) |
| `--socket PATH` | Override the Unix socket |
| `--version` | CLI and protocol versions |

## What it will not do

- Sign in for you (no cookies in the shell)
- Print spectrum in the terminal (that is UI-only)
- Replace `google-services` / `agent-youtube` for Google Cloud playlist OAuth

## Skill

Hermes, OpenClaw, and Grok: skill name `ytmusic`. Prefer the helper on PATH.

```bash
omarchy-ytmusic --json status
omarchy-ytmusic --json play here comes the sun
```
