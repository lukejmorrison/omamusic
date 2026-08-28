# Upstream

This directory is a local fork of
[rlimberger/omarchy-ytmusic](https://github.com/rlimberger/omarchy-ytmusic)
(MIT), itself started from
[stappmus/Omarchy-Spotify](https://github.com/stappmus/Omarchy-Spotify).

Wizwam additions also take cues from [cliamp](https://github.com/bjarneo/cliamp)
(EQ, spectrum, bar pill) and [Omasing](https://github.com/stappmus/Omasing)
(lyrics). Named credits live in [README.md](README.md#credits).

## Pin

- Upstream `main` at clone: `de0d64a`
- Cherry-picked: [PR #4](https://github.com/rlimberger/omarchy-ytmusic/pull/4)
  (`aadcf7a`) — android player client, drop cookies from stream resolve
- Cherry-picked: [PR #1](https://github.com/rlimberger/omarchy-ytmusic/pull/1)
  (`5a96119`) — cold yt-dlp signature cache timeout / warmup
- Plugin id: `wizwam.omamusic`
- Published repo: https://github.com/lukejmorrison/omamusic

## Runtime paths

This fork uses `omamusic` names. Setup still imports a leftover
`~/.config/omarchy-ytmusic/browser.json` if `~/.config/omamusic/` has none:

- `~/.config/systemd/user/omamusic.service` (never enabled at login)
- `~/.local/bin/omamusic`
- `~/.config/omamusic/browser.json` (mode 600)

## Pulling upstream

`/home/luke/dev/Omarchy/ytmusic` is the published monorepo plugin path, not a clone of rlimberger. Fetch upstream in a throwaway clone, then copy the files you need:

```bash
git clone --depth 1 https://github.com/rlimberger/omarchy-ytmusic.git /tmp/omarchy-ytmusic-upstream
# diff / copy into /home/luke/dev/Omarchy/ytmusic, then discard /tmp/omarchy-ytmusic-upstream
```

A bundle of the old standalone clone is at `/home/luke/dev/Omarchy/.ytmusic-standalone.bundle` if you need the pre-monorepo history.

Re-apply Wizwam id / Super+Shift+M defaults after any merge.
