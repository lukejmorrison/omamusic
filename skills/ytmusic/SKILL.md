---
name: ytmusic
description: >
  Use when an agent should play, pause, skip, search, queue, like, or report
  now-playing on the Omarchy YouTube Music player (wizwam.omamusic, mpv,
  Super+Shift+M, omarchy-ytmusic). Triggers: ytmusic, YouTube Music, play a
  song, pause music, what's playing, skip this track, Hermes, OpenClaw,
  Grok Bot.
version: 1.0.0
author: Wizwam
platforms: [linux]
metadata:
  hermes:
    tags: [omarchy, ytmusic, youtube-music, media, playback, mpv]
    related_skills: [omarchy, omarchy-agents, google-services]
    helper: omarchy-ytmusic
prerequisites:
  commands: [omarchy-ytmusic]
---

# YouTube Music (Omarchy player)

Control **this computer's** YouTube Music player: plugin id `wizwam.omamusic`,
local **mpv** + **yt-dlp**, Unix-socket API. Not Chromium. Not the YouTube
Data API.

**Canonical skill:** `skills/ytmusic/SKILL.md` in
[lukejmorrison/omamusic](https://github.com/lukejmorrison/omamusic).
Install for agents: `scripts/install-agent-skill.sh`

## Dual runtime

| Runtime | How to run |
|---------|------------|
| **OpenClaw / Namshub** | Read this skill; `exec` → `omarchy-ytmusic --json …` |
| **Hermes / Metatron (Signal)** | Skill under `~/.hermes/skills/ytmusic`; **must use `terminal`** |
| **Grok Build / Grok Bot** | `~/.grok/skills/ytmusic`; bash → `omarchy-ytmusic` |
| **Any new agent** | `install-agent-skill.sh` then the same CLI |

**Signal:** enable `terminal` (and preferably `file`), restart
`hermes-gateway.service`, then `/reset`. Do **not** tell Luke to run the
commands when `terminal` is available.

## Agent execution rules (mandatory)

1. **Always run** `omarchy-ytmusic`. Do not speak NDJSON at the socket, and do
   not drive playback through `google-services` / `agent-youtube`.
2. **Prefer `--json`.** Never claim success unless stdout is JSON with
   `"ok":true` (or human output starts with `playing` / `paused` / `ok`).
3. **Status first** when asked what is playing: `omarchy-ytmusic --json status`.
4. **Play a named song** with `omarchy-ytmusic --json play QUERY`. Bare `play`
   only resumes.
5. **Do not** print `~/.config/omarchy-ytmusic/browser.json`, cookies, or
   headers. Sign-in is the player's **Use Chromium session** button.
6. **Do not** edit `$OMARCHY_PATH` or `~/.local/share/omarchy/`.
7. If stderr says the socket is missing, start the player once
   (`omarchy-ytmusic open` or Super+Shift+M) and retry. The CLI starts the
   existing user unit; it is not enabled at login.

## Natural language → action

| User intent | Command |
|-------------|---------|
| “What's playing?” | `omarchy-ytmusic --json status` |
| “Play *song*” | `omarchy-ytmusic --json play song name` |
| “Pause / resume” | `omarchy-ytmusic --json pause` / `play` |
| “Skip / previous” | `omarchy-ytmusic --json next` / `prev` |
| “Volume 30” | `omarchy-ytmusic --json volume 30` |
| “Search city pop” | `omarchy-ytmusic --json search city pop` |
| “Like this” | `omarchy-ytmusic --json like` |
| “Open the player” | `omarchy-ytmusic open` |
| “Is the backend up?” | `omarchy-ytmusic --json health` |

```bash
omarchy-ytmusic --json status
omarchy-ytmusic --json play here comes the sun
omarchy-ytmusic --json pause
omarchy-ytmusic play-id VIDEO_ID
omarchy-ytmusic raw search '{"query":"city pop","limit":8}'
```

UI only (Omarchy shell IPC, not the socket):

```bash
omarchy shell -q wizwam.omamusic.player togglePlayer
omarchy shell -q wizwam.omamusic.player toggleMiniPlayer
```

## Stack

- Socket: `$XDG_RUNTIME_DIR/omarchy-ytmusic/backend.sock` (NDJSON, protocol 1,
  256 KiB frame cap).
- Unit: `omarchy-ytmusic.service` (user, not enabled at login).
- CLI: `~/.local/bin/omarchy-ytmusic` → `~/.local/lib/omarchy-ytmusic/cli.py`
  (stdlib + `protocol.py` only).
- Catalog/likes need the Chromium session already imported in the player.
- Related: `google-services` is OAuth playlist editing, not this player.

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `backend socket missing` | `omarchy-ytmusic open`, or Super+Shift+M once |
| `Sign in to like songs` | User must use **Use Chromium session** in the player |
| `unknown command` | `omarchy-ytmusic --help` |
| CLI not on PATH | `plugins/ytmusic/scripts/setup.sh` then `install-agent-skill.sh` |
