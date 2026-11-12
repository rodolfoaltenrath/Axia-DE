# Server migration to Rust

Started on 2026-11-11, from commit
`07b8f0bb489a96cd23bb9f7831bf1c4e74eb0edf`.

The objective is to replace the project's server implementation with Rust,
including the executable entry points, helpers and proof of concept. Work proceeds
in locally validated batches. Do not push, dispatch GitHub Actions, or publish a
build until the entire migration is complete. The existing workflows run on push
and pull requests, so a push would trigger builds prematurely.

## Baseline and completion criteria

The baseline in `server/src` is 270 C files (111,934 physical lines) and 254
headers (20,654 physical lines), including comments and blank lines. C tests add
119 implementation files and five headers. `server/poc` adds one C file.
Dependencies and generated files in `server/build` are excluded from these counts.

A module counts as migrated when its Rust implementation is linked into the
existing targets, its C implementation has been removed, and its behavior has
been checked. During the transition, C compatibility headers remain as long as
unmigrated callers need them. Keeping an implementation in C behind a Rust wrapper
does not count as migration.

Completion requires Rust entry points and implementations for all owned server
targets; removal of obsolete owned C headers, sources and build rules; migrated
regression coverage; and local integration checks of the complete desktop and
helpers. Generated protocol bindings and external system libraries must be
accounted for separately: using libc, Mesa or Wayland does not make their C source
part of the owned server migration. The Wayland protocol generation and QuickJS
boundary still need an explicit audit during the build/entry-point phase.

## Working sequence

1. Move isolated policies and algorithms into the existing `axia-shell` library.
   Keep the C ABI temporarily and verify it from existing C callers.
2. Migrate subsystem implementations and their tests: rendering/text, input,
   terminal, network/firewall, media, documents/printing, hardware, host services,
   storage/backups, mail, vault and security. Follow actual dependency boundaries;
   move shared types into Rust before removing their compatibility headers.
3. Migrate the Wayland server, GPU/KMS ownership, device/event dispatch and the
   compositor main loop. Replace C-owned resources with Rust owners and explicit
   lifetimes, preserving protocol and file-descriptor contracts.
4. Migrate every helper, benchmark/testbench and proof of concept, consolidate
   Cargo/build/packaging targets, and remove the remaining owned C implementation
   and compatibility headers. Port the C regression tests alongside their owners.
5. Run local full regression and integration checks, including CPU/GPU behavior,
   display/input lifecycle, sandbox/security, installed helpers and both supported
   architectures where a local toolchain/runtime is available. Record anything
   that cannot be exercised locally. Only after migration is complete, generate
   the GitHub Actions build requested by the user.

## Batch 1: frame scheduling, GPU damage and shortcut profiles

| Removed implementation | Rust replacement | Compatibility header |
| --- | --- | --- |
| `server/src/fpipe.c` | `rust/axia-shell/src/frame_pipeline.rs` | `server/src/fpipe.h` |
| `server/src/gpu/gscissor.c` | `rust/axia-shell/src/gpu_scissor.rs` | `server/src/gpu/gscissor.h` |
| `server/src/wl/keygrab.c` | `rust/axia-shell/src/keygrab.rs` | `server/src/wl/keygrab.h` |

All server and engine build source lists now use the Rust archive for these
implementations. Shortcut fallback letters reuse the existing Rust keymap table.
GPU rectangle area calculations widen before subtraction and saturate at the C
`long` maximum for extreme coordinates where the old C arithmetic overflowed.
Defined inputs retain their existing behavior.

Validation:

- `cargo test --manifest-path rust/Cargo.toml -p axia-shell --lib --locked --offline`:
  471 passed, 11 ignored (the existing environment-specific tests).
- `make -C server build/fpipe-test build/keygrab-test`: the C caller regressions
  passed, including all 61 frame-pipeline checks.
- A temporary differential harness compiled the baseline C implementations under
  renamed symbols and linked them beside the Rust archive. It checked all 256
  letter byte values, codes -1 through 768, modifier masks 0 through 31, profile
  aliases/rejected changes, layout fallback strings, and 20,000 randomized GPU
  history operations. All 63,447,250 comparisons passed.
- New Rust coverage checks frame thresholds, non-boolean C flag values, GPU age
  history/reset, extreme rectangle clipping, and 300 frames of a three-buffer
  swapchain against independent full redraws.

The local Fedora environment has GLib/GIO runtime libraries but lacks their devel
link names and pkg-config files. Unit tests were linked using temporary symlinks
in `/tmp/axia-rust-native-libs`, with
`LIBRARY_PATH=/tmp/axia-rust-native-libs`. The two standalone C regressions were
also run with `RUST_NATIVE_LIBS=-Wl,--gc-sections`; these tests do not use GIO.
Formatting used `rustup run 1.99.0 rustfmt --edition 2021` because the default
toolchain does not have rustfmt installed. No full compositor build or desktop
integration run has been claimed for this batch. No GitHub Actions build ran.

Remaining after this batch: **267 C implementation files / 111,738 physical
lines in `server/src`**; the 254 compatibility/other headers and C test/helper
migration remain outstanding. Three removed implementation files account for
196 original C lines. This is the first batch, not completion of the migration.

## Batch 2: pixel worker pool and keyboard ownership

| Removed implementation | Rust replacement | Compatibility header |
| --- | --- | --- |
| `server/src/par.c` | `rust/axia-shell/src/parallel.rs` | `server/src/par.h` |
| `server/src/keyboard.c` | `rust/axia-shell/src/keyboard/` | `server/src/keyboard.h` |
| `server/src/osk.c` | `rust/axia-shell/src/onscreen_keyboard.rs` | `server/src/osk.h` |

The worker pool owns its synchronization in Rust and borrows each job's callback
context only until every worker has acknowledged the generation. It retains the
eight-thread cap, three pieces per thread, 256 KiB worker stacks, caller
participation, and serial fallback for nested/concurrent submissions. Waiting for
all workers to finish their claim loop also prevents a previous generation from
claiming a piece after the next job has reset the shared counter. Initialization
is now strictly once-only, including when configured for one thread; the old C
implementation could retry initialization if it still had just one thread. The
documented compositor call remains once at startup.

Realtime setup still checks `RLIMIT_RTPRIO`, uses `SCHED_RR` priority 2, applies
`SCHED_RESET_ON_FORK` to the compositor, drops both RTPRIO limits to zero and
demotes recorded thread IDs on SIGXCPU. Workers acknowledge their registration
before initialization returns. The signal handler performs no allocation or
locking. The local tests verify the unprivileged path and RTPRIO limit drop in
child processes; privileged realtime scheduling and actual SIGXCPU exhaustion
have not been exercised locally.

The keyboard implementation now owns XKB contexts, keymaps, states, compose
tables/states and label scratch states with Rust destructors. It preserves the
existing layout/group, modifier, repeat, compose and keymap-serialization ABI.
Failed keymap/state creation leaves the old keyboard intact. The exported layout
setter copies borrowed getter strings before modifying their backing storage.
The on-screen keyboard has a bounded, synchronized tap queue that erases consumed
entries and retains the fixed key whitelist and token-device exclusions.

All affected Makefile source lists and the on-screen keyboard regression target
now link these implementations from the Rust archive. The shared native link
libraries include libxkbcommon, which the main compositor already required.
Its minimal Rust declarations and keysym ABI values were checked against the
[upstream 1.13.1 headers](https://github.com/xkbcommon/libxkbcommon/tree/xkbcommon-1.13.1/include/xkbcommon).

Validation:

- `cargo test --manifest-path rust/Cargo.toml --locked --offline --quiet`:
  478 library tests passed, 11 library tests and four existing native integration
  tests were ignored by their existing environment gates. No tests failed.
- Worker-process tests exercise one, four, capped-at-eight and automatically
  selected thread counts. They verify actual participation of every worker,
  idempotent startup, concurrent submissions, nested calls, empty/single-row
  ranges, exact row coverage and completion before borrowed data is released.
  An additional test covers the entire signed 32-bit row interval without
  subtraction overflow.
- `make -C server build/osk-test build/glass-renderer-test build/fpipe-test
  build/keygrab-test` passed with the local native-library workaround described
  below. Running `server/build/glass-renderer-test` passed all 128 frozen legacy-C
  pixel hashes, covering serial/threaded, cold/cached rendering. The frame
  pipeline's 61 C ABI checks and existing shortcut regressions still pass.
- A temporary differential harness linked renamed baseline keyboard/OSK objects
  beside the Rust archive. All 1,508,278 comparisons passed: nine real
  layout/variant combinations, two group selections, randomized key/repeat/reset
  sequences, modifier serialization, label levels, serialized keymap imports,
  all key codes from -1 through 99,999, 100,000 queue operations, capability
  bitmaps, token names and chassis/bus policy.
- Rust keyboard tests cover dead-accent composition, modifier suppression of
  text, focus reset preserving groups, scratch labels leaving live modifiers
  intact, serialized keymaps and tiny/empty output buffers. Queue tests cover
  wraparound, overflow, forbidden modifiers/system keys and erased tap storage.

Local commands use `LIBRARY_PATH=/tmp/axia-rust-native-libs`, now also containing
`libxkbcommon.so` pointing to the installed `libxkbcommon.so.0`, because the XKB
devel link name is absent too. Standalone C tests use
`RUST_NATIVE_LIBS='-Wl,--gc-sections -lxkbcommon'`. Temporary upstream headers were
used only to compile the historical C reference for differential testing; the
Rust implementation has no build-time header dependency.

Remaining after batch 2: **264 C implementation files / 111,254 physical lines
in `server/src`**. Six files / 680 original C lines have been replaced across
both batches. Headers and C tests remain transitional; the full compositor and
privileged realtime paths still need local integration validation. No push or
GitHub Actions build has been requested or run.

## Batch 3: evdev input and timing-only latency traces

| Removed implementation | Rust replacement | Compatibility header |
| --- | --- | --- |
| `server/src/input.c` | `rust/axia-shell/src/evdev.rs` | `server/src/input.h` |
| `server/src/keylag.c` | `rust/axia-shell/src/key_latency.rs` | `server/src/keylag.h` |

Evdev discovery, capability classification, exclusive grabs, seat-mediated
open/close, descriptor-list hotplug, duplicate detection, event dispatch and
attached-keyboard policy now run in Rust. The inotify descriptor has a Rust
owner; the device array retains the existing C layout until the compositor's
containing `Server` moves to Rust. Dispatch deliberately keeps device access as
raw pointers across the C callback, so it does not lend an exclusive Rust
reference to the shell while the callback accesses that same shell. Kernel
device names are explicitly NUL-terminated within their fixed buffer.

The latency tracer now owns its bounded event queue and private output file in
Rust. No API or queue field accepts key codes, text or modifiers. Only traceable
Wayland deliveries receive sequence numbers; protected deliveries, shell-handled
events and autorepeat remain uncounted. The former mutable C
`keylag_enabled` global is now an atomic query function, and every C caller and
the compatibility header have been updated together. Disabled/repeat receive
calls do not dereference the supplied event pointer. File creation retains the
absolute runtime-directory requirement, path length bound, unlink of a stale
entry, exclusive creation, NOFOLLOW, CLOEXEC and mode 0600. Output remains
immediately visible and preserves the legacy timing-line format.

Validation:

- `cargo test --manifest-path rust/Cargo.toml --locked --offline --quiet`:
  **486 library tests passed**, 11 library tests and four native integration
  tests were ignored by the existing environment gates. No failures.
- Three new Rust evdev tests cover dispatch across multiple read batches,
  updating the typing flag before callbacks, rejected-descriptor closure,
  empty lists, removal ordering, and closing every live descriptor exactly once.
- `make -C server build/input-ffi-test` passes a new native ABI fixture using
  real pipes with mocked kernel capabilities/stat and mocked seat/touchpad
  providers. It checks keyboard/mouse/tablet/touchpad/touchscreen/token
  classification, high-resolution wheels, absolute ranges, nonblocking flags,
  the 32-device limit, device closure/freeing, active-only rescans, hotplug and
  deduplication by device identity. The fixture was also compiled with the
  baseline `input.c` and `osk.c` from the recorded baseline commit, and passed
  unchanged against that implementation.
- Five new Rust latency tests cover privacy gates and no-file/no-count behavior,
  exact timing output, 0600/CLOEXEC, stage ordering and one timestamp per stage,
  queue overflow, missing-stage expiry, the strict time-window boundary, line
  cap cleanup, replacing a stale symlink without modifying its target, and
  failure on missing/relative/oversized runtime paths.
- `make -C server build/key-latency-ffi-test` passes a new native ABI fixture in
  fresh child processes with a deterministic clock. It checks disabled and
  autorepeat calls with null event pointers, protected/shell events, exact
  printed numbers (including rounding), queue overflow and expiry, the 120 s
  boundary and all 4,000 lines through automatic stop. The same fixture passed
  against the baseline C tracer via a temporary adapter for the new enabled
  query function.
- Both ABI fixtures are part of the normal `test` and `test-platform` targets.
  Their small C callers are transitional ABI coverage, not production C
  implementations. They must move to Rust with the remaining native tests
  before the overall migration is considered complete.

Commands use the same local `LIBRARY_PATH` and standalone `RUST_NATIVE_LIBS`
workarounds recorded in batch 2. No real host input device was opened or grabbed
by the fixtures. Actual hardware hotplug/inotify and the full graphical session
remain integration work for the completed server.

Remaining after batch 3: **262 C implementation files / 110,932 physical lines
in `server/src`**. Eight files / 1,002 original C lines have been replaced in
total. No push or GitHub Actions build has been requested or run.

## Batch 4: Korean, Japanese and Chinese input methods

Removed `server/src/ime.c` (1,006 physical lines). Its complete implementation is
now in `rust/axia-shell/src/input_method/`, split into the common composition/UI
state, Korean syllables, Japanese conversion, Chinese candidates, dictionary
loading and the preserved language tables. `server/src/ime.h` remains the
temporary compatibility ABI, and `build/ime-test` links the Rust archive.

The Korean engine retains compound vowels/finals, moving a final into the next
syllable, and its twelve-step backspace history. Japanese retains romaji
conversion, small kana/gemination, EUC-JP SKK lookups with and without okurigana,
dynamic-programming phrase segmentation, function-word/particle costs, per-word
candidate selection and Shift+arrow segment resizing. Chinese retains exact
matches ordered by weight, syllable-initial abbreviations, eight highest-weight
prefix completions, partial phrase accumulation, deduplication and nine-item
candidate pages. All composition stays bounded by the original preedit, candidate,
segment, dictionary-entry and normalized-key budgets.

Dictionary contents are now read into bounded Rust-owned storage rather than
kept as raw mappings. Files must be regular, nonempty and smaller than 64 MiB;
nonblocking open avoids getting stuck on a FIFO before checking its metadata.
User-before-system lookup and first-activation caching remain intact. This gives
the indexes a stable snapshot even if a dictionary file is subsequently modified.
Encoding conversion continues through libc iconv. The former mmap sharing/RSS
measurements in `docs/LANGUAGES.md` are explicitly historical; memory usage of the
completed Rust compositor still needs to be measured.

Validation:

- `cargo test --manifest-path rust/Cargo.toml --locked --offline --quiet`:
  **494 library tests passed**, 11 library tests and four native integration
  tests were ignored by the existing environment gates. No failures.
- Eight new Rust tests use small, synthetic SKK/Rime fixtures. They check frozen
  legacy examples, Korean undo, Japanese phrase/okurigana conversion and segment
  focus, Chinese abbreviations/partial picks/paging, modifier pass-through,
  context reset, missing dictionaries, small/zero commit buffers, long
  compositions, bounded candidates and repeated segment resizing.
- Dictionary tests check user precedence, fallback only when the earlier path
  is absent, invalid selected files, directories/FIFOs/oversized files, SKK
  annotations/expression filtering, bad EUC-JP, truncated headers and a missing
  final newline. They use isolated temporary paths without changing the process
  home directory.
- A temporary native differential harness linked the baseline C implementation
  under renamed symbols beside the current Rust archive. It passed **1,503,624
  comparisons** of consumed flags, committed bytes, preedit, active candidates,
  selection, conversion/focus fields, current-engine/dictionary labels and
  changed-state notifications. Coverage includes 300,000 randomized key/control
  operations across the three engines, output capacities 1/2/3/4/8/16/512,
  dictionary-backed examples, all 18 existing Japanese corpus phrases, long
  compositions and 360 focused-segment resize operations. Synthetic dictionary
  paths were supplied through a test-only getenv wrapper; production has no
  reference-C dependency or added environment override.
- Rebuilt `build/ime-test` and the existing input, latency, on-screen keyboard,
  frame pipeline, shortcut and glass-renderer ABI targets. All executable
  regressions passed, including 61 frame-pipeline checks and 128 frozen C pixel
  hashes. Native IME examples retain `gksrmf` → committed 한 / preedit 글 and
  dictionary-free romaji → こんにちは.

The fixtures live in `rust/axia-shell/tests/data/ime/`; the original C headless
test remains as a temporary native ABI caller. Local build commands use the
native-library workaround recorded in batch 2. Full graphical-session validation
and the remaining server migration are still outstanding.

Remaining after batch 4: **261 C implementation files / 109,926 physical lines
in `server/src`**. Nine files / 2,008 original C lines have been replaced in
total. No push or GitHub Actions build has been requested or run.

## Batch 5: touchpad/touchscreen driver and gesture routing

Removed `server/src/tp/tp.c` (902 physical lines) and `tp_route.c` (157 lines).
Their implementations are now in `rust/axia-shell/src/touch/`, separated into
device discovery/event handling, gesture state machines and routing. The native
touch regression target now links the Rust archive. The existing `tp.h` and
`tp_route.h` remain compatibility interfaces for unmigrated compositor callers.

The driver retains protocol-B slots, hardware capability discovery, monotonic
event-clock setup, dropped-event resynchronization, millimeter conversion,
pointer acceleration/smoothing, palm/edge/thumb/typing filters, physical clicks,
tap/double-tap/drag, scroll/inertia, pinch/rotation, swipes/hold, three-finger
drag, touchscreen delivery/emulation, device-list JSON and timer/latency state.
The eight gesture states and six tap states are Rust enums. Device state is
owned in the registry with stable opaque handles; immutable capability/geometry
metadata lives separately, so callbacks can query it while a gesture is being
updated. Unsafe device entry points remain input-thread-only and prohibit
destruction or recursive dispatch of devices from callbacks.

Routing preserves the target chosen at gesture start, UI-only inertia, Wayland
rotation deltas, desktop swipe policy, high-resolution wheel accumulation,
screen-coordinate conversion and the host's pointer/button/flush callbacks.
No registry/router lock is held while invoking foreign callbacks. The router
borrows dimensions from the already Rust-owned UI state and uses the existing
painter's hit-test function until that painter is migrated; no new C production
bridge was added.

Validation:

- `cargo test --manifest-path rust/Cargo.toml --locked --offline --quiet`:
  **496 library tests passed**, 11 library tests and four native integration
  tests were ignored by their existing environment gates. No failures.
- `server/build/tp-test`: **57/57 passed**, covering the existing simulated
  pointer, click, tap, scroll/momentum, pinch/rotation, swipe, drag, palm, edge,
  typing suppression and touchscreen scenarios, including their recognition
  timing checks.
- A temporary differential harness linked the renamed baseline C driver beside
  the Rust archive. All **1,688,060 comparisons** passed across all simulated
  gestures, both device types, one/two/five position slots, twelve configuration
  profiles (including disabled features, typing suppression and app-consumed
  touches), acceleration samples, timers, output phases/identifiers and device
  JSON at capacities 1 through 511. Discrete fields and JSON are compared exactly;
  numerical outputs use a relative/absolute tolerance of 1e-9.
- A new `build/touch-route-ffi-test`, included in normal `test` and
  `test-platform`, checks actual Rust driver-to-router callbacks with recorder
  Wayland/JS endpoints. It covers fixed targets despite focus changes, momentum
  suppression, wheel120 conversion, pinch rotation deltas, three/four-finger
  desktop policy, touchscreen delivery and pointer emulation, and host flushing.
  JS callbacks query/update configuration and query device metadata, exercising
  callback reentry without a held driver/router lock. The identical fixture also
  passed against the two baseline C implementations.
- The existing input ABI fixture now uses the real Rust touch driver instead of
  fake touch handles. Mock kernel/seat calls verify device discovery, capabilities,
  monotonic clock selection, recovery via all three MTSLOTS queries after
  SYN_DROPPED on both device types, and destruction when evdev descriptors close.
- Two Rust tests check acceleration endpoints, missing-resolution geometry,
  slot bounds, default tracking IDs and, in an isolated child, the sixteen-device
  limit, capability-pointer stability across registry growth, JSON sanitization,
  invalid slots, dropped-event suppression, tap timers and metadata queries from
  button callbacks.
- Rebuilt and passed the input, key-latency, on-screen-keyboard, frame-pipeline
  and shortcut ABI regressions. The glass renderer still passes all **128** frozen
  C pixel hashes. Formatting and `git diff --check` pass.

Tests use the same local native-library workaround as prior batches. No physical
input device, real Wayland client or graphical session was exercised by these
recorders; hardware and whole-session integration remain outstanding.

Remaining after batch 5: **259 C implementation files / 108,867 physical lines
in `server/src`**. Eleven files / 3,067 original C lines have been replaced in
total. No push or GitHub Actions build has been requested or run.

Next: cursor and pointer/window interaction, then the Wayland input/text-input
protocol implementations. Header and native-test migration remain part of the
full goal.

## Batch 6 — cursors and window drag controller

Replaced `cursor.c` (262 lines) and `ui/grab.c` (335 lines), removing both
implementations from the build. `cursor.rs` owns vector rasterization and an
immutable, lazily initialized cache for all 23 shapes at five display scales.
The runtime style parser shares its CSS name table. The existing cursor ABI
exposes borrowed views into stable Rust-owned pixels.

`runtime/window_grab.rs` owns the active drag, saved theme dimensions, zone
tracking, linked resizing, detach state, layer lifecycle and scripted movement.
It calls the existing Rust snapping algorithms directly and updates Rust-owned
nodes. The controller runs on the UI thread; callbacks must not recursively
enter it or run while a Scene borrow is active. Painter and benchmark backends
remain C until their own migration. Compatibility headers remain transitional.

Validation:

- **500 library tests passed**, with 11 library tests and four native integration
  tests ignored by their environment gates. No failures.
- All **115 frozen cursor images** match baseline C hashes. A native differential
  harness compared **7,925,760 pixels exactly**, plus edge, Wayland protocol,
  invalid-input normalization and screen-scale mappings. Rust tests additionally
  check cache stability across threads and premultiplied pixels.
- A baseline/Rust controller harness passed **1,666,250 exact comparisons** over
  5,000 drags: moves, every resize edge, percentage dimensions, display limits,
  detach, linked windows, Shift/zone transitions, layers, finish/cancel and style
  restoration.
- The new `build/window-grab-ffi-test` passes against both the baseline C
  controller and the Rust replacement, using real Rust-owned nodes and recorded
  painter/benchmark endpoints. It covers deferred restoration, deleted nodes,
  min/max limits, snapping, links and both scripted-input paths. It is included
  in `test` and `test-platform`. Rust tests also cover numeric dimension equality
  (signed zero and NaN) and isolated Scene/controller integration.
- Input, touch routing, key latency, keyboard, frame pipeline, shortcut and
  snapping ABI regressions pass. The glass renderer still passes all **128**
  frozen C pixel checks. New Rust file formatting and `git diff --check` pass.

The local native-library workaround remains necessary. These tests do not replace
real Wayland client, graphical-session or hardware validation, which remains
outstanding for the complete migration.

Remaining after batch 6: **257 C implementation files / 108,270 physical lines
in `server/src`**. Thirteen original files / **3,664 C lines** have been replaced
in total. No push or GitHub Actions build has been requested or run.

Next: Wayland input/text-input protocol implementations. Headers, native tests
and the rest of the owned server remain within the full migration objective.

## Batch 7 — Wayland text input

Replaced `wl/wl_textinput.c` (115 lines) with `wayland/text_input.rs`.
Rust owns the input registry, pending and committed caret geometry, enable state,
focus transitions and resource destruction. Protocol descriptors and request
dispatch tables are also Rust-owned; this protocol no longer requires generated
C code. The advertised version remains **zwp_text_input_manager_v3 v1**.
`wayland/ffi.rs` declares the small public libwayland-server ABI used by this
module. Borrowed native handles stay on the display's dispatch thread.

Behavior preserves pending enable/disable until commit, reset-on-enable,
width-only caret changes without notifications, newest-input-first notification
order, focused-client creation, same-client focus reset, silent focused-surface
destruction and active-input destruction reports. Surrounding text, input
purpose and change-cause requests are decoded but not retained; text continues
through the existing keyboard path. Callback invocations occur after releasing
the Rust state borrow. The current host callback was inspected: it only appends
an event to the compositor queue and does not recursively enter this controller.

Validation:

- **501 library tests passed**, 11 library tests and four native integration tests
  ignored by their environment gates; no failures.
- `build/text-input-ffi-test`, included in `test` and `test-platform`, creates a
  real libwayland display and two socketpair clients. An independent wire client
  binds through wl_registry and dispatches every advertised request. It checks
  multiple text-input resources, pending commits, caret policy, ignored text
  requests, focus changes, destruction and disconnection, with no text/done events.
- The identical fixture passed against the baseline C implementation and Rust.
  Its **3,000-operation deterministic stream**, plus explicit scenarios, yields
  the same **44 reports and 1,113 focus events**. The complete ordered report and
  focus-event digest is frozen at **46c8615e21ad4334** and asserted in the test.
- Window-drag and touch-router ABI regressions pass after relinking the archive.
  Rust formatting and `git diff --check` pass.

Local validation uses public Wayland 1.24 headers downloaded to `/tmp`, the
installed libwayland-server 1.26 runtime, and the existing temporary native linker
symlinks. For the temporary baseline build, its generated protocol header includes
`wayland-server-core.h` directly to avoid the unavailable legacy umbrella header's
unneeded core-protocol include. No library behavior was mocked. No dependencies
or historical C implementations were added to the repository. Real toolkit,
graphical-session, hardware and complete compositor validation remain outstanding.

Remaining after batch 7: **256 C implementation files / 108,155 physical lines
in `server/src`**. Fourteen original files / **3,779 C lines** have been replaced
in total. Headers and C test fixtures remain transitional and within scope.
No push or GitHub Actions build has been requested or run.

Next: the remaining Wayland gesture, scroll, touchscreen and seat-capability
implementation in `wl_input.c`, using the new Rust protocol boundary.

## Batch 8 — Wayland gestures, scrolling and direct touches

Replaced `wl/wl_input.c` (306 lines) with `wayland/input.rs` and
`wayland/gestures.rs`. Rust owns resource registries, gesture-client latches,
high-resolution wheel remainders, the ten touch slots and per-client touch-frame
deduplication. Stable pinned seat listeners remove their Rust-owned registration
on native resource destruction; their foreign-owned list links use UnsafeCell.
Native handles remain borrowed and are removed through resource callbacks.
All operations stay on the display's dispatch thread, with no state borrow held
across host queries or event emission.

The pointer-gestures v3 descriptors are defined in Rust and exported under the
existing protocol symbols for the remaining native clients. The compositor no
longer links generated C metadata for this protocol. Client header generation
and separate test-client protocol generation remain available. The touch router
now calls the Rust backend directly instead of declaring it as foreign code.

Preserved behavior includes pointer version gates (axis_source/frame/stop at v5,
value120 at v8 and natural-scroll direction at v9), discrete wheel fallback,
cross-client wheel remainders, round-to-nearest 24.8 coordinates, gesture targets
latched at begin, per-recipient serial ordering, rotation deltas, local touch
coordinates latched to their initial surface, per-slot cancellation and dynamic
touchscreen seat capabilities. Unused C rotation bookkeeping was removed.

Validation:

- **503 library tests passed**, with the existing 11 library and four native
  integration environment gates ignored. No failures. New unit tests cover wheel
  remainder signs, fixed-point rounding and bounded touch-frame deduplication.
- `build/wayland-input-ffi-test`, included in `test` and `test-platform`, uses real
  libwayland-server dispatch, two independent socketpair clients and six pointer
  versions (1, 4, 5, 7, 8, 9). It exercises every migrated entry point, pointer
  event version gates, fractional-wheel accumulation, focus changes, multiple
  resources, gestures after pointer leave, touch target/frame behavior, resource
  release, seat-listener destruction and client disconnects. Capabilities use a
  real Rust-owned simulated touchscreen. Additional manager v1/v2 and touch v1
  resources check older clients. Clock wrapping and half-fixed-unit rounding are
  checked explicitly. Only the monotonic clock and host surface queries are
  controlled; protocol dispatch, serials, resources and transmission are real.
- The unchanged fixture also passes against renamed baseline C functions and
  baseline C protocol descriptors. **5,000 deterministic mixed operations** plus
  explicit scenarios produced **25,220 ordered events**. The complete event
  traces match **byte for byte** using `cmp`, not just their digest. The baseline
  digest **b972f4087f659b2a** is frozen and asserted by the regression test.
- `build/touch-route-ffi-test` now uses this real backend and wire peers rather
  than C definitions of migrated functions. It still checks actual driver/router
  callbacks, UI targeting, inertia policy, wheel flushing, rotation deltas,
  three/four-finger system swipes, direct touches and pointer emulation. Its
  assertions now inspect delivered protocol events; calls dropped due to missing
  focus and release events without rotation payload are checked accordingly.
- Text-input, evdev and window-drag ABI regressions pass after relinking. The
  glass renderer still passes all **128** frozen C pixel checks. New Rust file
  formatting and `git diff --check` pass.

Local native tests use Wayland 1.26 development headers extracted from Debian's
libwayland-dev package into `/tmp`, the installed 1.26 runtime and the previous
temporary linker symlinks. Nothing was installed into the host or added as a
vendored dependency. Test fixtures replace compositor surface queries and do not
exercise real toolkit rendering, a whole graphical session or input hardware;
those checks remain outstanding for the full migration.

Remaining after batch 8: **255 C implementation files / 107,849 physical lines
in `server/src`**. Fifteen original files / **4,085 C lines** have been replaced
in total. Headers and native C tests remain transitional and within scope.
No push or GitHub Actions build has been requested or run.

Next: X11 wire helpers and XWayland process integration, before the larger
window-manager/compositor state migration.

## Batch 9 — X11 wire protocol and request ownership

Replaced `wl/x11wire.c` (208 lines) with `x11/wire.rs` and `x11/ffi.rs`.
Rust now owns the request buffer in a bounded Vec, request framing, connection
setup parsing, packet lengths, sequence widening and ICCCM/EWMH/Motif property
decoding. The three inline implementations in `x11wire.h` (unaligned little-endian
reads and client-ID comparison) were moved into Rust as well.

The transitional `XOut` ABI is now an opaque owning handle with Rust accessors.
`xwm.c` uses borrowed byte views and length/failure queries instead of reading or
writing Rust buffer fields. Queue overflow marks a sticky error through `xo_fail`.
The handle must be zero-initialized, used on its owning thread, never copied, and
freed through `xo_free`; byte views expire at the next mutation. Both the initial
state allocation and buffer growth report allocation failure, and checked size
arithmetic prevents overflow. The historical 64 MiB bound and growth policy are
preserved. Rust APIs for the future window-manager port avoid the compatibility
handle altogether.

Parsing preserves partial/incomplete output behavior (including which setup fields
are written on malformed accepted headers), little-endian encoding, sequence
wraparound, property bounds and exact title sanitation. In particular, invalid
UTF-8 is handled byte by byte, decoded surrogates are dropped, and a character
that cannot fit can be skipped while subsequent smaller characters are retained;
using generic lossy UTF-8 conversion would change this behavior.

Validation:

- **507 library tests passed**, with the existing 11 library tests and four native
  integration tests ignored by their environment gates; no failures. Four new Rust
  tests cover bounded ownership, overflow, the opaque ABI lifecycle, UTF-8/control
  handling, output capacities, setup parsing and sequence wraparound.
- `build/xwm-test`: **65/65 checks passed**. The existing wire/property fixtures now
  link Rust; added native checks exercise growth across an allocation boundary,
  partial-write dropping, forced queue failure, repeated cleanup, oversized-size
  rejection and recovery. It is included in both `test` and `test-platform`.
- A temporary native harness linked renamed baseline C helpers beside the Rust
  archive. All **1,864,430 exact comparisons** passed: 5,000 accumulated request
  sequences with partial drops and 100,000 randomized property/setup/framing
  samples, both title encodings and output capacities 0–64, unaligned reads,
  sequence widening, client masks, display choice and clipboard target ranking.
  Comparisons include complete output bytes, untouched sentinel bytes, setup
  padding, partially written output, the 64 MiB boundary, sticky failure and
  cleanup. Historical code and the differential harness remain outside the repo.
- Both current C consumers (`xwm.c` and `xwayland.c`) pass compiler syntax checks
  against the changed header. This proves source integration, not a full running
  XWayland session.
- Wayland input, touch-router and text-input protocol regressions pass after
  relinking. The glass renderer still passes all **128** frozen C pixel checks.
  New Rust formatting and `git diff --check` pass.

Tests use the existing temporary native-library/header workaround. No X server or
real X11 application was started in this batch. XWayland process lifecycle,
window management, real client/session behavior and complete compositor validation
remain outstanding.

Remaining after batch 9: **254 C implementation files / 107,641 physical lines
in `server/src`**. Sixteen original files / **4,293 C lines** have been replaced
in total, plus the three header helper implementations. Compatibility headers
and native C tests remain within the full migration scope. No push or GitHub
Actions build has been requested or run.

Next: `xwayland.c` process lifecycle and its display/socket reservations, then the
X window manager and the main Wayland compositor.

## Batch 10 — lazy XWayland process lifecycle

Replaced `wl/xwayland.c` (321 lines) with `x11/xwayland.rs`. Rust owns the
listening socket and pidfd as OwnedFd values, Wayland event-source lifetimes,
display reservations, client-destruction listener and restart policy. The listener
has stable thread-local storage with foreign-owned links in UnsafeCell. Borrowed
display/socket strings live separately from mutable process state, so unrelated
state queries cannot invalidate their addresses. All operations stay on the
Wayland/UI thread, and host callbacks run without a process-state borrow.

The executable, argument list and environment remain fixed by the implementation.
The child plan, environment and UID/GID map strings are prepared before fork.
The post-fork path uses libc calls and the existing C scope-entry function before
execve/_exit; it does not allocate Rust strings or access RefCell state. Descriptor
remapping to 3/4/5, close_range, signal reset, scope failure policy, namespace
request and no_new_privs preserve the existing behavior. Binary ownership/mode
checks, stale-lock handling, log bounds/NOFOLLOW, startup-on-connect, idle stop,
three-quick-failure cooldown and bounded shutdown waiting are retained.

`xwayland.h` now documents the transitional thread/lifetime contract and declares
the two lifecycle notifications previously redeclared inside `xwm.c`. No new C
production bridge or Rust dependency was added; the existing cgroup and X window
manager implementations remain C until their own migration.

Validation:

- **510 library tests passed**, with the existing 11 library and four native
  integration environment gates ignored. Three new Rust tests cover the restart
  budget, binary/string policy and borrowed-name stability across state queries.
- `build/xwayland-ffi-test`, included in `test` and `test-platform`, uses real
  Unix sockets, file permissions, descriptors and libwayland event/client callbacks
  in a private temporary directory. It checks live/stale/foreign/nonregular locks,
  all 64 displays occupied, invalid socket directories, binary refusal, startup
  monitoring failure, socket/bind/socketpair/fork failures, client/WM handoff
  failures, client destruction, missing pidfds, idle/cooldown policy and cleanup.
  Descriptor counts return to their initial value after reservation tests and
  after the full lifecycle test.
- Ten real forked children exercise the production child path with execve
  intercepted before any XWayland binary is run. Six verify complete exec plans:
  fixed arguments/environment, default environment values, descriptor 3 listener,
  descriptors 4/5, close-on-exec clearing, no descriptors 6–255, session creation,
  no_new_privs, scope request and namespace-map bytes. Other cases verify exit 126
  for managed-scope/fcntl/dup2 failures and exit 127 for exec failure. Tests also
  check log truncation and a symlink log target remaining unchanged.
- The identical native fixture passes against the renamed baseline C lifecycle
  implementation, including all ten real child cases. Mocked process outcomes
  exercise repeated restart transitions; real child reaping uses real pidfds.
  Namespace creation and cgroup placement are controlled test endpoints, so this
  does not prove real sessiond placement or kernel namespace availability.
- The X11 wire regression still passes **65/65** checks. Wayland input, touch
  routing and text-input regressions pass after relinking. The glass renderer
  still passes all **128** frozen C pixel checks. The remaining C window manager
  passes syntax checking against the updated declarations. Rust formatting and
  `git diff --check` pass.

Local tests use the existing temporary native-library and Wayland-header
workaround. Test path/environment wrappers redirect only the test process's
operations; host X sockets, locks, HOME and installed XWayland are untouched.
No real X11 application, kernel namespace/sessiond placement or complete graphical
session was tested. Those remain part of final integration validation.

Remaining after batch 10: **253 C implementation files / 107,319 physical lines
in `server/src`**. Seventeen original files / **4,614 C lines** have been replaced
in total, plus the header helper implementations from batch 9. The remaining
headers, native C tests, window manager and compositor stay within the full goal.
No push or GitHub Actions build has been requested or run.

**User-requested checkpoint:** finish this XWayland lifecycle batch and pause
further migration. No next module was started. The complete migration remains
unfinished; any later resumption continues from this worktree.
