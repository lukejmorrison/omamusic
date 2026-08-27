# OMA Music — agent notes

Plugin id **`wizwam.omamusic`**. Binary **`omamusic`**. Work only here:

| Path | Role |
|------|------|
| `/home/luke/dev/omamusic` | This repository (QML plugin + Rust daemon; Python fallback) |

Do **not** open `/home/luke/dev/Omarchy/omarchy` as a second Grok workspace.
**Never clone or use `/home/luke/dev/omarchy`.** That lowercase path collides
with `/home/luke/dev/Omarchy`.

The maintenance workspace may symlink `/home/luke/dev/Omarchy/ytmusic` here.

Open PRs against `lukejmorrison/omamusic`. Do **not** open PRs against:

- `rlimberger/omarchy-ytmusic` (upstream read-only)
- `lukejmorrison/omarchy-ytmusic` (wrong fork)
- `haripako/omamusic` (unrelated MPRIS widget)

## Verify before push

```bash
cd /home/luke/dev/omamusic
./scripts/test.sh
```

`test.sh` runs the Rust daemon tests (`cargo` required) and the Python
fallback tests. `./scripts/test-rust.sh` is the Rust-only subset.

After a local plugin fix, rebuild the daemon and reload QML:

```bash
./scripts/reload.sh
```

Tests belong in `tests/` (Python/QML) and `src/` (Rust). Never add
`test_server.py` at the plugin root.

## Do not commit

- `AGENT_FEEDBACK.md`
- `/target/`
- `__pycache__/`
