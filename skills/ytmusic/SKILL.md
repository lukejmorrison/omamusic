---
name: ytmusic
description: >
  Use when an agent should play, pause, skip, search, queue, like, or report
  now-playing on the Omarchy YouTube Music player (wizwam.omamusic, mpv,
  Super+Shift+M, omamusic). Triggers: ytmusic, YouTube Music, play a
  song, pause music, what's playing, skip this track, Hermes, OpenClaw,
  Grok Bot.
version: 1.0.0
author: Wizwam
platforms: [linux]
metadata:
  hermes:
    tags: [omarchy, ytmusic, youtube-music, media, playback, mpv]
    related_skills: [omarchy, omarchy-agents, google-services]
    helper: omamusic
prerequisites:
  commands: [omamusic]
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
| **OpenClaw / Namshub** | Read this skill; `exec` → `omamusic --json …` |
| **Hermes / Metatron (Signal)** | Skill under `~/.hermes/skills/ytmusic`; **must use `terminal`** |
| **Grok Build / Grok Bot** | `~/.grok/skills/ytmusic`; bash → `omamusic` |
| **Any new agent** | `install-agent-skill.sh` then the same CLI |

**Signal:** enable `terminal` (and preferably `file`), restart
`hermes-gateway.service`, then `/reset`. Do **not** tell Luke to run the
commands when `terminal` is available.

## Agent execution rules (mandatory)

1. **Always run** `omamusic`. Do not speak NDJSON at the socket, and do
   not drive playback through `google-services` / `agent-youtube`.
2. **Prefer `--json`.** Never claim success unless stdout is JSON with
   `"ok":true` (or human output starts with `playing` / `paused` / `ok`).
3. **Status first** when asked what is playing: `omamusic --json status`.
4. **Play a named song** with `omamusic --json play QUERY`. Bare `play`
   only resumes.
5. **Do not** print `~/.config/omamusic/browser.json`, cookies, or
   headers. Sign-in is the player's **Use Chromium session** button.
6. **Do not** edit `$OMARCHY_PATH` or `~/.local/share/omarchy/`.
7. If stderr says the socket is missing, start the player once
   (`omamusic open` or Super+Shift+M) and retry. The CLI starts the
   existing user unit; it is not enabled at login.

## Natural language → action

| User intent | Command |
|-------------|---------|
| “What's playing?” | `omamusic --json status` |
| “Play *song*” | `omamusic --json play song name` |
| “Pause / resume” | `omamusic --json pause` / `play` |
| “Skip / previous” | `omamusic --json next` / `prev` |
| “Volume 30” | `omamusic --json volume 30` |
| “Search city pop” | `omamusic --json search city pop` |
| “Like this” | `omamusic --json like` |
| “Open the player” | `omamusic open` |
| “Is the backend up?” | `omamusic --json health` |

```bash
omamusic --json status
omamusic --json play here comes the sun
omamusic --json pause
omamusic play-id VIDEO_ID
omamusic raw search '{"query":"city pop","limit":8}'
```

UI only (Omarchy shell IPC, not the socket):

```bash
omarchy shell -q wizwam.omamusic.player togglePlayer
omarchy shell -q wizwam.omamusic.player toggleMiniPlayer
```

## Stack

- Socket: `$XDG_RUNTIME_DIR/omamusic/backend.sock` (NDJSON, protocol 1,
  256 KiB frame cap).
- Unit: `omamusic.service` (user, not enabled at login).
- CLI: `~/.local/bin/omamusic`.
- Catalog/likes need the Chromium session already imported in the player.
- Related: `google-services` is OAuth playlist editing, not this player.

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `backend socket missing` | `omamusic open`, or Super+Shift+M once |
| `Sign in to like songs` | User must use **Use Chromium session** in the player |
| `unknown command` | `omamusic --help` |
| CLI not on PATH | `plugins/ytmusic/scripts/setup.sh` then `install-agent-skill.sh` |
