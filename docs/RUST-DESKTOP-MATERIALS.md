# Rust wallpaper service and glass material

Axia embeds these services in `rust/axia-shell`; there is no new daemon, runtime,
Cargo dependency, privileged action or distribution-specific wallpaper service.
Catalog video downloads use the host's curl executable in a background worker.
The React settings page and existing C compositor use narrow C ABI adapters.

## Wallpapers

The desktop context menu and the Control Center wallpaper heading open Settings →
Wallpaper. PNG/JPEG imports are validated and copied by Rust into
`$XDG_DATA_HOME/axia/wallpapers` (default `~/.local/share/axia/wallpapers`). A content
filename deduplicates repeated imports; existing bytes are compared before reuse.
Imports use a temporary private file and atomic rename. Their size is limited to
32 MiB, dimensions to 8192 per side, and area to 16,777,216 pixels, before decoding.
The original can subsequently be moved or deleted. User files are never modified.

The bounded Rust decoder validates the actual managed copy, and its PNG encoder
writes a thumbnail resampled by Rust, at most 320×180. Galleries decode thumbnails,
not full originals. The image bridge releases managed originals and scale caches
when their last display/node releases them; shared displays retain their references.
Imported copies remain on disk for future selection.

`wallpaperUser` in user settings stores the managed entries. Both themes, Overview
and Control Center resolve the selection independently of the theme collection.
Existing preferences need no schema conversion. Selecting a still image clears
motion wallpaper overrides. Theme, mode and accent changes preserve the selection.
The login wallpaper and third-party desktop settings remain separate.

## Glass

Rust owns every backdrop pixel kernel, geometry, clipping, reusable working
buffer, worker scratch, reduced backdrop cache, and the light/dark material policy.
The old C blur implementation is removed. C provides a thin canvas/worker-pool
adapter, compositing order and damage tracking. The blur remains the
same three-pass reduced-resolution algorithm; rounded clipping is applied anew
at the current position. The Dock and Rust app switcher share the material.
Disabling effects makes their material opaque and disables blur.

Before each lookup, Rust samples the actual backdrop into its owned memory, including
external app content on the CPU renderer. The cache compares every reduced input
pixel, dimensions and blur radius. It reuses only the blurred intermediate, never
a screen rectangle, tint or rounded edge. Wallpaper changes, app updates and window
movement therefore cannot reuse a stale result. Partial repaint dependencies and
blur margins still belong to the existing compositor damage tracker.

There are four entries, at most 2 MiB total per rendering thread. Larger regions
bypass the result cache. Working pixel arrays are capped at 32 MiB per thread,
with bounded column/division arrays and up to 32 MiB worker scratch. Actual
allocations follow the regions rendered; storage is reused after warm-up and TLS
releases it when a thread exits. No lock or background refresh loop runs per frame.
C must finish all workers before invoking the next cache operation on that thread;
raw pointers stay valid until the next prepare/scratch call documented in the ABI.

GPU external-window blur remains the limitation described in `GPU.md`: external
GPU textures are not in the CPU canvas. This change does not enable that backend
in Fedora RPMs (`GPU=0`) or claim to implement a GPU blur pass.

## Validation

`make -C server test-platform` runs Rust validity/budget tests, the C ABI selector
checks, 128 frozen legacy-C pixel comparisons (serial/parallel, cold/cached),
wallpaper image lifetime and existing input/cursor regressions.
`build/engine-test` (run from `server/`) checks compositor partial repaints and
existing CPU/GPU model pixel goldens. `tools/desktop/check.py` starts both native
themes and imports real PNG fixtures through Rust → C → QuickJS, validates invalid
inputs, managed thumbnails, persistence, theme-independent selection and the app
switcher. It also exercises the asynchronous Rust motion store with an offline
SHA-256-verified catalog fixture, and tests the actual sandboxed player without a
GPU: frame size/rate bounds, pause/resume, looping, and uncapped ordinary videos.
All run in isolated profiles without touching the active session.

`server/tests/glass_bench.c` compares a fixed 1366×220 backdrop, a 1000×136 glass
region and radius 24. Compile the same harness against the reference `canvas.c`
and this version. Compare checksums for unchanged and changing backgrounds before
considering timings. This is a serial raster microbenchmark, not a measurement of
mouse latency, complete desktop FPS, or GPU rendering.

Local validation on 2026-11-03: 17 Rust tests, 128 frozen C/Rust pixel comparisons,
9,521 compositor checks, 602 Wayland input checks, 24 cursor checks, image lifetime,
both headless themes and native wallpaper/motion-storage integration passed. The
software-only player produced 43 wallpaper frames with a 1280×720 ring, pause/resume
and looping; an ordinary video kept 1920×1080 and about 100 frames in the same test.
A separate isolated run downloaded a real catalog video, verified its SHA-256 and
played it through the actual CPU compositor/helper on the local Intel laptop.

The Rust horizontal pass writes directly to the second buffer, eliminating both
the per-row scratch copy and the whole-intermediate copy of the legacy C path.
Serial blur microbenchmarks retain identical checksums. Local cached-C/Rust samples
were 2.298/2.264 ms per unchanged frame and 3.098/3.055 ms per changing frame;
variation between runs is large enough that these are not a desktop speedup claim.

The per-language catalog checks pass for English, Spanish and Portuguese. The
repository-wide text lint still reports its pre-existing six prose exceptions in
`js_switcher.c` and six brand-name matches in the shortcut-profile descriptions;
this change adds no new global-lint findings.
