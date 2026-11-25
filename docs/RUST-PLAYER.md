# Player in Rust

Goal: replace `apps/player/index.jsx` and `library.js` (and the JSON manifest) with a compiled
native window, keeping every feature: library by song, artist and album, videos, queue, shuffle
and repeat, time and volume bars, `.srt` and in-stream subtitles, audio track choice, full
screen, picture in picture, "Open with", file drop, the Control Center tile and the media keys.
Decoding stays in `axia-media` (see [MEDIA.md](MEDIA.md)); nothing here changes the helper.

## What the JSX does and where it goes

| JSX | Rust |
| --- | --- |
| `library.js`: walk ~/Music and ~/Videos, probe tags, cache | `model.rs` (`partition`, `track_from`, `cache_json`), the walk in the window |
| `albumsOf`, `artistsOf`, sort orders | `model.rs` (`albums_of`, `artists_of`, `collate`) |
| `parseSrt`, cue under the position | `model.rs` (`parse_srt`, `cue_at`) |
| next, previous, shuffle, repeat | `model.rs` (`Queue::next`, `Queue::prev`) |
| `usePlayer` reducer | `model.rs` (`PlayerState::reduce`) |
| volume, seek, bar fractions, controls hiding, picture-in-picture frame | `model.rs` |
| `nowPlaying` payload | `model.rs` (`now_playing`) |
| `Tri`, `PlayGlyph`, `Bar`, rows, cards, popups | `ui.rs` (stage 3) |

## Status

All four stages are written and registered; the folder has only Rust sources. Nothing has been
compiled or run on a desktop yet: the Rust and Node suites below are the checks to run first.

1. **Model**: `apps/player/model.rs`, `model_tests.rs`. The sort orders replace `localeCompare`
   with `axia_native::matching::fold` (case and accents do not decide, the text breaks ties);
   numbers are not compared as numbers, as before. A dot file keeps its whole name as its title
   (the JSX gave an empty one). The library cache keeps the shape `library.js` wrote, so
   `~/.cache/axia/music.json` is reused.
2. **Services** (transport only, `runtime/native_media_services.js` and
   `native_window_services.js`): `media.probe`; `media.nowPlaying`, which publishes the
   `nowPlaying` store with buttons that come back as `{kind:'nowPlaying', action}` window events
   and withdraws it with the window; `windows.screen` and `windows.pip`, so Rust chooses the
   picture-in-picture frame. Full screen is the host effect `fullscreen`; the context carries
   `windowMode`. Covered by `node server/tests/native_media_test.mjs`.
3. **Window**: `mod.rs` (state, jobs, events, queue, checkpoint), `ui.rs` (trees and the bar
   geometry), `scan.rs` (the walk and the cache on worker threads), `manifest.rs`, `tests.rs`.
4. **Registration**: compiled registry, `native_view/mod.rs`, the `axia-shell` dependency,
   `Cargo.lock`, the `native_apps_runtime` expectation; `index.jsx`, `library.js` and
   `manifest.json` removed.

## Behaviour that differs from the JSX

- The two bars are hit-tested by arithmetic, so the layout fixes their boxes: every transport
  button is a fixed 30 px, and the library bar has the volume bar only (subtitles, audio track,
  picture in picture and full screen are in the video view, one click on the cover away).
- The song list scrolls with the wheel; its scroll thumb shows the position but is not dragged.
  Its column header stays fixed above the rows.
- Two files are probed at a time (the helper has two workers); the JSX probed one.
- A one-track queue on "next" or on the end of the track with repeat all starts over; the JSX
  did nothing.
- Full screen and picture in picture can mount the window again, which closes its session.
  The window saves queue, view, volume, play state and position first (`windowState`) and
  reopens the track at that position. A short gap is audible.
- Covers are asked for in the background, six at a time, for every card the page shows.

## Still to do

- A GPU runtime test (`player_runtime`) that opens a real file in the compositor, and the
  graphical check of the video overlay, picture in picture and the Control Center tile.
- Whether Control Center should read `nowPlaying` from the Rust session instead of the SDK store.
