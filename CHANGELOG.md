# Changelog

## 1.3.0

- Publish as https://github.com/lukejmorrison/omamusic with plugin id
  `wizwam.omamusic`. One plugin per repository so the Omarchy marketplace
  can list it without scanning Keychron or Remote.
- Keep the Python playback backend and prefer the `omamusic` Rust daemon
  when that binary is installed.

## Unreleased

- Keep the Chromium YouTube Music session fresh by re-reading the live
  cookie database on start and every five minutes, instead of relying on
  a day-old `browser.json` snapshot.
- Add experimental Google device-code sign-in (`start_oauth`) that stores
  a refresh token in `oauth.json` without Google Cloud Console. The login
  page keeps Chromium as the recommended path and probes Music library
  before treating OAuth as signed in (Innertube still rejects many tokens).
- Resolve YouTube Music's public InnerTube client key at runtime
  (`YTM_API_KEY`, else `INNERTUBE_API_KEY` from music.youtube.com) so
  a Google API key is not committed in the tree.
- Chicklet tooltips show on mouse hover only, so opening the player no
  longer flashes "Pause · Space" on the play button.
- CLI, socket, config, and Python fallback all use `omamusic` names.
  Leftover `omarchy-ytmusic` paths are still imported or deleted on
  uninstall. Upstream credits still point at rlimberger/omarchy-ytmusic.
- `scripts/setup.sh` installs the `omamusic` Rust daemon from a pinned
  GitHub Release (no cargo). It builds from source if that download fails
  and `cargo` is present, and falls back to Python only if both fail.
  Local rebuilds use `setup.sh --from-source` so Omarchy does not
  hot-reload the plugin mid-build (`~/.cache/omamusic/target`).
- Plugin name and bar widget are "OMA Music". Plugin id stays
  `wizwam.omamusic`.
- `scripts/setup.sh` copies `queue_session.py` with the rest of the Python
  backend, so local install no longer fails with `ModuleNotFoundError`.
- Sidebar brand is "OMA Music" / "for YouTube".
- Sleep timer menu opens in the player window instead of behind it.
- Prefer the `omamusic` Rust daemon when it is installed. The player UI stays
  QML and still falls back to the Python backend otherwise.
- Clicking a song, album, artist, or playlist name opens that page (or plays
  the song). Album and playlist ids survive YouTube's catalog payloads, so
  those pages keep their art and tracks instead of "cannot be played".
- Artist and album pages load while a song is playing. Spectrum events no
  longer splice themselves into catalog replies, which had left the page on
  Loading until it timed out.
- `scripts/reload.sh` restarts the playback backend and Omarchy shell after
  a local fix so QML and Python both match the checkout.
- Mini player stacks like/copy (song and album) above a larger cover with
  wrapping left-aligned titles.
- Mini player and bar chip stay put when the song changes. The chip keeps a
  fixed label slot, and transport buttons sit on a stable row, so skip no
  longer slides the popup sideways.
- Player and CLI how-to manuals, with live screenshots of the bar chip,
  mini player, and full window.
- Credit the projects this player is built on: rlimberger's YouTube Music
  plugin, stappmus Omarchy Spotify and Omasing, Bjarne's cliamp and
  now-playing chip, ytmusicapi, mpv, yt-dlp, and Omarchy.
- Refresh the Chromium session hash on each backend start and every five
  minutes, so library login does not die overnight.
- Liked songs, playlists, and library no longer fail silent. An expired
  session asks you to sign in again instead of showing an empty list.
- Playlist pages load the first 80 tracks and time out after 45s instead of
  staying on Loading with “0 items”.
- A refused or failed stream says so in the player, instead of hanging on
  a song that never starts.
- Full player window is capped so search results cannot inflate the panel.

- Live cliamp-style spectrum: ten frequency bars (green / yellow / red) in
  the mini player, the full-player footer, and Settings. The backend reads
  the Pulse/PipeWire sink monitor so the display follows the sound leaving
  this computer, including EQ.
- Full-player footer stacks a larger cover above a wrapping, left-aligned
  title. Song and album like/copy sit above the transport. Seek and volume
  tracks are thicker, and the seek bar matches the spectrum width.
- Footer title and byline stay on one line across the player until the
  window is half- or quarter-width, then they wrap.
- Mini-player EQ sliders share the spectrum's 10-column grid. The EQ
  preset chicklet sits in the transport row with shuffle and repeat.
- The last EQ preset and custom bands are saved with plugin settings and
  restored when playback reconnects.
- Playback restart always copies the full backend, compile-checks it, and
  waits until the socket answers. A stuck connect screen auto-retries twice
  then re-enables the button instead of hanging on Working.
- A restart restores the last queue, track, and position. If you were playing,
  playback resumes instead of coming back as Nothing playing.
- The play queue is saved while you listen, not only when the backend exits.
- Stale mpv processes from earlier restarts are stopped so they cannot keep
  playing a different song behind the new player.
- Bar chip play/pause stays in the middle of previous/next. Hover still
  reveals skip, but the glyph no longer jumps from the left.

## 1.2.6

- Bar chip matches the cliamp now-playing pill: artist and title, a rounded
  slot, hover previous/play/next, and a scrolling label.
- Volume in the full player shows the knob, fill, and percent. Mute is on the
  speaker. Missing backend volume no longer paints an empty slider.
- Selected sidebar pages join the main pane: same colour, open right edge,
  and curved scoops so Home / Queue / History read as part of the body.
  Half-screen keeps labeled nav, clamps the join, and paints the sidebar
  border in the pane colour so the highlight stays connected.
- Sidebar, main pane, and player footer use a shared corner radius. The
  hairline rules under Music / Queue are gone.
- Search and filter fields use the same rounded fill. The queue highlights
  the track that is actually playing, in the player-bar colour.
- Like, skip, pause, and other icon actions are fixed-size pill chicklets
  with the bar-widget border, so hover no longer jumps the layout.
- Album art is cropped and clipped to the same rounded chicklet shape, so
  square covers no longer sit inside round cards.
- The player footer keeps Space, skip, like, queue, and search keys in view.
- Icon-only controls, artwork, sliders, and elided titles have hover tooltips
  and accessible names.

## 1.2.5

- Home no longer stays blank after a backend restart. Catalog replies are not
  treated as now-playing state, Home reloads when the window opens empty, and
  `get_home` is cached so the player can paint shelves instead of waiting on a
  second YouTube round-trip.

## 1.2.4

- Full player now-playing shows two lines of the song title, a hover with the
  full name, and a copy-link button (`music.youtube.com/watch?v=…`).
- History is a sidebar page (Ctrl+Shift+H). Plays on this computer are kept
  even when you are not signed in; a signed-in session also merges YouTube
  history.

## 1.2.3

- After like asks you to sign in, the player opens the Connect page (Use
  Chromium session). The Home banner and a sidebar **Sign in** button do the
  same thing.

## 1.2.2

- Like no longer dumps `HTTP 401 Unauthorized` in the mini player. Missing or
  expired YouTube Music sessions ask you to sign in, and the footer button
  becomes **Sign in**.

## 1.2.1

- Mini player: search songs and add them to the play queue (`/` or Ctrl+K).
  Click a result to play it now, or the queue button to play it next.

## 1.2.0

- Wizwam fork: plugin id `wizwam.omamusic`. Super+Shift+M toggles the full player
  instead of launching Spotify. The bar chip sits on the right, before the
  tray, and shows the current title.
- Force yt-dlp `android` player client so local streams do not 403. Combined
  with the cold-cache resolve budget below.

- Fix the first play after an install reporting "Playback failed". yt-dlp has to
  fetch and solve YouTube's player JS challenge the first time it sees a new
  player build, which does not fit the 40s resolve budget. A cold cache now gets
  150s, and the backend warms the challenge in the background at startup using a
  video from the user's own catalog.
- Report a resolve timeout in plain language instead of a raw Python traceback.
- Show "Preparing playback…" while the backend waits on yt-dlp, so a slow
  resolve no longer looks like a stalled player.

## 1.1.1

- Keep the playback socket alive while the player is open, and reconnect when it drops.
- Open the backend socket before catalog setup so the player can connect immediately.
- Do not idle-stop playback while a player window is connected.
- Restart a stopped backend instead of leaving Home empty.

## 1.1.0

- Recreate the backend socket after a dropped connection so the player can recover.
- Sign in by copying the YouTube Music session already in Chromium on this computer.
- Keep pasted request headers as a fallback.
- Keep the local mpv process off the Wayland session so tracks actually start.
- Use a D-Bus-safe mpv client name so MPRIS cannot freeze playback.
- Publish the song title to MPRIS instead of the googlevideo stream URL.
- Refresh that MPRIS title when the next track starts, not after the stream URL loads.
- Keep the bar slot as the YouTube Music logo only.

## 1.0.0

- First release: Omarchy bar widget, mini-player, and full player for YouTube Music.
- Started from [Omarchy Spotify](https://github.com/stappmus/Omarchy-Spotify).
- Local playback through a plugin-owned mpv backend and yt-dlp, not Chromium.
- Library, search, playlists, queue, likes, radio, sleep timer, and Omasing lyrics.
