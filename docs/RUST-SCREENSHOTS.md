# Screenshots migration

The app is still in migration. `apps/screenshots/index.jsx` retains
capture/recording/image transport and window mounting. The icon and visual selector
are native Rust views in `selector.rs`; `auxiliary.rs` owns the thumbnail,
countdown and recording indicator scenes and their deadlines. `editor_view.rs`
owns the markup scene and calls `geometry` and `markup::Editor` directly in Rust.
`markup::Editor` owns the tool, color, operation
history, draft stroke, text input and undo. Each editor has a separate native
model. There is no JavaScript geometry or markup-policy fallback.

`panel.rs` now owns the main scene, preferences, recent-file presentation and
protection target policy. `session.rs` owns recording generations/identity,
completion metadata and the bounded recent history independently of mounted
views. The context's resident `capture::Service` owns that model and the capture
pipeline deadlines; the private app-model bridge no longer owns session state.
JavaScript retains a revisioned notification mirror of that session
snapshot, rather than owning the recording/history stores.

The native geometry preserves reverse drags, exclusive right/bottom window
edges, topmost eligible window selection and contain-fit letterboxing. Annotation
coordinates remain in image pixels, including a stroke dragged outside the image;
the existing rasterizer clips it. Text input anchors and pointer mapping use the
same fit. Invalid or zero-size layouts return no coordinate instead of submitting
an invalid stroke. Only numeric point fields cross the bridge, never UI event
objects. The final native view can use the typed functions directly and retire
this transitional JSON interface.

Evidence for this part (`build/validation/screenshots-geometry/`):

- Four Rust geometry tests and 716 shell tests passed, with seven shell tests
  ignored. A differential suite checks 105 cases against the calculations in
  `index.jsx` at commit `baeecaf4`: landscape/portrait images, letterboxing,
  offset viewports, reverse/empty drags and text anchors.
- A compositor test in light and dark modes drives the actual capture overlay
  through pointer input, verifies the crop rectangle and draws an arrow through
  the editor. Requests reach the production native model. Both editor captures
  were inspected; the arrow ends at the expected pointer position.
- GPU=1 build and platform/ABI checks passed, as did eight LPK contracts and
  eight switcher scenarios. All eight switcher PNGs match the Disks delivery.
  Twelve changed source files were compared byte for byte with the Fedora tree.
  This part adds no background timer and makes no
  performance improvement claim. The application is not counted as migrated yet.

The following markup-state delivery has its own evidence under
`build/validation/screenshots-markup/`:

- Nine app tests pass (four geometry and five markup). They check image-space
  drafts, a single commit per release, tiny-stroke rejection, cancel, undo,
  independent editors, reset, closed tools/colors, text preservation and the
  historical JavaScript whitespace rule. Tool/color changes do not invalidate
  the raster; idle pointer movement does not mutate the model.
- 716 shell tests pass, with seven ignored. The production GPU=1 build passes.
- Light/dark compositor cases drive the actual UI through area cropping,
  arrow/box/blur/line, color selection, text entry/submit and Ctrl+Z. The native
  history, coordinate requests and raster operations are checked. Idle movement
  issues no new annotation. No new timer is introduced.
- The 105 geometry references still match. Platform/ABI checks, eight LPK
  contracts and eight switcher scenarios pass; eight switcher PNGs remain
  identical. Six changed sources match the Fedora tree byte for byte. Editor
  captures were inspected in both modes.

## Native selector

PrintScreen opens the Rust scene with area, window, monitor and all-screen modes.
Dragging or choosing a target does not save an image. Enter or Capture confirms;
Escape or Cancel discards it. Space switches area/window mode. Direct captures
remain separate: Meta+Shift+3 in the macOS profile and Ctrl+PrintScreen in the
Linux profile; the chosen external screenshot app still overrides these actions.

The snapshot explicitly covers the entire logical desktop, independent of the
primary-display/capture-all backend setting. The view mounts in the shell overlay
layer, spans all displays and owns its modal keyboard scope. Window frames and
display geometry are frozen with the image; invisible windows and other desktops
are excluded; picture-in-picture windows remain eligible across desktops.
Fullscreen and floating targets use their actual displayed rectangles. A
display-layout change cancels the selection. Scaling is not
multiplied into logical coordinates. Controls stay on the main display and wrap
at small widths, so a gap between differently sized displays cannot hide them.

The saved thumbnail remains at the bottom-right corner of the display containing
most of the selected region (main display for all-screen captures). Its size,
saved/copied caption, hover-aware expiry and click-to-edit behavior are preserved.
The transport uses image-specific leases, ignores results for replaced sources,
and releases each adopted snapshot on confirm/cancel/unmount. Cropped images are
transferred to the thumbnail/editor. Legacy text focus is released while selecting
and restored if its source window is still active on return.

The view emits a terminal `captureSelection` effect. `NativeView.onEffect` routes
it only to the mounted transport; unrelated effects keep their existing runner.
The handler uses its latest props, and the source handle prevents an old selection
from being applied to a replacement image. This callback remains a temporary
transport until the capture controller itself migrates.

Validation is recorded in `build/validation/screenshots-selector/`: thirteen app
tests, 716 shell tests (seven ignored), shortcut-profile checks, and ten graphical
scenarios (eight selector, two markup). The selector matrix covers Axia/Adwaita,
light/dark, 760/1000 px and both keyboard profiles, with synthetic monitors at
different heights and 150/200% scales. Actual scene input exercises PrintScreen,
Enter/Escape, all four modes, file saving, thumbnail click, focused source text,
image replacement and layout-change cancellation. It checks control positions,
crop coordinates, single release of the source and focus restoration. Captures
are inspected; this is not a physical multi-monitor test.

The GPU=1 shipping build, platform/ABI checks, 105 historical geometry cases,
eight LPK checks and eight switcher scenarios pass. The private installation also
passes the switcher check. Seventeen executed source files match the workspace
byte for byte; all eight switcher captures match the preceding markup delivery.
The installed software/offline idle check reports medians of 13.950 poll calls/s
and 104.148 MiB RSS, within the existing 15 calls/s and 120 MiB limits.

`native_view_facts_test.mjs` remains unable to bundle: its isolated mocks lack
`applyNativeWindowState`, `settingsRevision` and `gestureBridge`. The same failure
was reproduced against the HEAD bridge before this delivery
(`facts-baseline.log`, compared with `facts.log`). This is a pre-existing fixture
limitation; the selector, markup and switcher GUI runs use the production bridge.

## Native auxiliary windows

`auxiliary::App` dispatches the selector and auxiliary scenes. The thumbnail
keeps its bottom-right placement, saved/copied caption and click-to-edit. Rust
owns its eight-second deadline, suspends it during hover and restarts it on
leave. Replacing the image resets its deadline; terminal effects carry the
source handle so they cannot consume a replacement. The SDK transport releases
replaced images and transfers ownership to the editor. A delayed thumbnail close
checks the current handle before closing its window.

The countdown uses the native deadline and second boundaries instead of a
200-ms JavaScript interval. Escape cancels it; closing its window disposes the
pending transport callback. Its keyboard scope works for a picture-in-picture
window, releasing and restoring legacy text focus around the countdown. The
recording indicator also refreshes at second boundaries, replacing the 500-ms
JavaScript interval. Stop emits once while awaiting the backend; a failed stop
can be retried. A failed editor launch similarly restores the thumbnail action.
No native refresh is scheduled for a hovered/consumed thumbnail, cancelled or
completed countdown, inactive recording or pending successful stop.

The global capture/recording controller, media handles, completion callbacks,
settings and recent list still use the existing transport. The short delays
between hiding windows and taking a frame, and between opening the editor and
closing the thumbnail, remain there until the controller migrates. Moving these
scenes does not complete the app or remove JavaScript from the session.

Validation under `build/validation/screenshots-auxiliary/` includes sixteen app
tests, 716 shell tests (seven ignored), the GPU=1 build, platform/ABI checks,
105 historical geometry cases, eight LPK checks and eight switcher scenarios.
All eighteen app GUI scenarios pass (eight auxiliary, eight selector, two editor).
The auxiliary matrix covers Axia/Adwaita, light/dark and 760/1000-px desktops;
one fixture also waits across the real eight-second hover/expiry deadline and
past a closed five-second countdown. All fixtures exercise Escape/text-focus
restoration, background countdown completion, thumbnail replacement/transfer,
elapsed recording time, failed-stop retry, duplicate-stop suppression and SDK
microphone-indicator cleanup. The recording backend is simulated below the SDK;
this does not prove physical screen/audio recording or encoder behavior.
Captures are inspected. Five changed sources match the Fedora execution tree
byte for byte, and eight switcher captures match the preceding selector delivery.
The installed switcher check passes. The earlier isolated facts-fixture failure
documented above remains outside this delivery; the SDK bridge itself is unchanged.
Three installed software/offline idle samples pass the enforced limits, with
medians of 12.350 poll calls/s and 104.453 MiB RSS (limits: 15 calls/s, 120 MiB).
These measurements do not establish a causal performance gain or replace the
physical measurements deferred to the roadmap's end.

## Native markup scene

The toolbar, palette, canvas, native text field and transient save/copy captions
now belong to `editor_view::App`, with no React editor state or JavaScript
pointer/keyboard listeners. The toolbar uses two rows at narrow widths. Text
fields stay inside the canvas while the annotation retains its original image
coordinates. Keyboard history supports Ctrl+Z and Meta+Z; Enter commits text and
Escape cancels it. Losing window focus cancels an unfinished stroke.

A countdown's Escape must win before an app's native text field. The existing
global shortcut transport now scopes `capture.cancel` to the countdown window,
closing it before forwarding that key to the field. Other keys remain usable
while preparing the screen during the timer. The editor GUI reproduces the
failure without this binding (`native-countdown-before-fix.log`), then verifies
typing before/during/after cancellation preserves the same native text draft
in both keyboard profiles. Global shortcut transport remains pending below.

Only drawing revisions emit `captureEditor/annotate`; idle pointer movement,
tool/color changes and save/copy messages do not regenerate the raster. Save and
copy reject duplicate jobs while pending. Source handles and request tokens
guard replies after image replacement; terminal close rejects later actions.
Result captions expire after 2.5 seconds using the native deadline, without a
JavaScript timeout. The transport owns original/preview leases, reuses previews,
releases replacements and ignores completions after unmount. File naming and
SDK image I/O still use the transport until the global controller migrates.

Evidence is in `build/validation/screenshots-editor/`: eighteen app tests, 716
shell tests (seven ignored), 105 historical geometry cases, platform/ABI, GPU=1
build, eight LPK checks, private installation and installed switcher check pass.
All twenty-four app GUI scenarios pass (eight editor, eight selector, eight
auxiliary), plus eight switcher scenarios. The editor matrix covers Axia/Adwaita,
light/dark, 380×360 and 1000×680 scenes; the minimum scene is also offset from
the desktop origin. It exercises all tools, color changes, text Enter/Escape, actual
field placement, Ctrl/Meta+Z, focus loss during a draft, idle raster reuse,
save/copy success, an injected copy failure, caption expiry, replacement during a pending job,
late-result rejection and single release of originals/previews. Original PNG
bytes are compared after saving the annotated copy. Captures are inspected.
Five changed sources match the Fedora execution tree byte for byte; all eight
switcher captures match the preceding auxiliary delivery. Three installed
software/offline idle samples pass the enforced limits: medians 12.350 poll
calls/s and 104.809 MiB RSS, below 15 calls/s and 120 MiB. These results do not
prove a causal performance gain or physical capture/recording behavior. The
previously documented isolated facts fixture remains unchanged.

## Native panel and session facts

The panel reads the authoritative native preference source, preserving timer,
clipboard and microphone options. Patches retain unrelated screenshot keys.
Its four capture cards, controls, protected-window note and five visible recent
files are native and scroll at the minimum size. The existing Privacy menu sends
fresh raw window facts at invocation; Rust chooses the top eligible window,
including visible sticky/picture-in-picture windows, and excludes hidden,
minimized, off-desktop and capture-app windows. Recent actions validate the
selected path and preserve separate photo/video viewer instances.

Panel key input does not enter the selector's Space/Enter/Escape handler. The
shortcut legend spells out Meta+Shift in all three locales so missing symbol
glyphs cannot obscure the keys.

The persistent native model owns pending recording tokens, accepted source ID,
start/completion times and the twenty-entry recent history. A new request either
starts, cancels a pending attempt or stops the accepted recording. Old permission
or job results cannot adopt a source after cancellation, clear a newer pending
attempt or complete another recording. The adapter stops/closes rejected sources
and folds matching completion into Rust. Cancelling a recording countdown
invalidates its token immediately; an old window cleanup cannot cancel a new
attempt. Permission-error feedback is suppressed for superseded attempts.

The late-result scenario also reproduced a shared SDK indicator bug: closing an
old microphone source cleared the marker while a newer tracked source remained.
Commit `41c188d5` keeps the marker until the last tracked source of the same
app/device kind ends. Its dedicated test fails before the fix and passes after;
the native media service tests also pass. Evidence is under
`build/validation/media-devices/`.

Evidence for this delivery is under `build/validation/screenshots-panel/`.
Twenty-three app tests and 716 shell tests pass (seven shell tests ignored),
along with 105 historical geometry cases, the GPU=1 build, platform/ABI checks,
eight LPK contracts, private installation and the installed switcher check.
All thirty-two app GUI scenarios pass: eight each for panel, editor, selector
and auxiliary windows, plus eight switcher scenarios. The panel matrix covers
Axia/Adwaita, light/dark, 380x360 and 440x420 scenes at a nonzero desktop offset.
It exercises preference persistence, unrelated preference keys, panel keyboard
isolation, fresh privacy targets and black pixels throughout a protected-window
capture; full/window/area capture, recent photo/video routing, completion without
a mounted panel, delayed cancelled sources, microphone retention/cleanup and
exactly one close per source. A real five-second countdown is cancelled and
immediately restarted, ensuring old cleanup cannot invalidate the new attempt.
The recording backend is simulated below the SDK; this does not validate physical
recording or encoding. Representative controls and recent-file captures were
inspected, including the minimum-size shortcut legend. Nine source/locale files
match the Fedora execution tree byte for byte; all eight switcher PNGs match the
preceding editor delivery. Three installed software/offline idle samples pass
enforced limits, with medians of 13.200 poll calls/s and 104.449 MiB RSS (limits:
15 calls/s and 120 MiB). These results do not establish a causal performance gain
or replace the physical measurements deferred to the roadmap's end. The earlier
isolated facts-fixture limitation remains unchanged.

## Resident capture controller

`rust/axia-shell/src/capture.rs` is owned by the context's window session. The
root controller exposes capture/session commands; only its bounded effects are
drained through the existing resident service bridge. No panel or native-view
host owns the lifetime of pending work. The old JavaScript `afterHide`,
`withTimer`, countdown callback map and their capture timeouts have been removed.

Rust chooses capture windows from the authoritative window reader, preserves
editors, waits 160 ms for images or 220 ms for recording, reads the native timer
preference, and owns the five/ten-second countdown and 180 ms settling delay.
Deadlines use the monotonic clock. Countdown placement uses the primary native
display's geometry. The view supplies presentation, cancellation and focus
transport; its completion callback is not required to trigger the capture.
Idle and media-permission waits add no periodic timer.

Each request has a generation. Replacing/cancelling it invalidates queued work
before the SDK drain and rejects old asynchronous image handles exactly once.
Old countdown cleanup cannot cancel a newer request or the normal settling
phase. Recording permissions retain their separate source token. Session state
is shared by context, with independent histories in independent contexts.
Capturing during recording hides its badge with the other capture windows, then
restores it only for the same active recording/current generation. A replacement
capture inherits restoration ownership; a stopped recording cannot be revived.

Evidence is under `build/validation/screenshots-controller/`. Five new shell
tests drive the resident scheduler with no scene/JavaScript timer, cancellation
and replacement, cleanup during settling, queued-effect rejection, recording
badge restoration ownership and independent context state. All 721 shell tests
pass (seven ignored), as do 23 app tests, 105 historical geometry cases, GPU=1
build, platform/ABI, eight LPK contracts and the isolated installed switcher
check. Thirty-two app GUI scenarios pass, plus eight switcher scenarios. The
expanded eight-case panel matrix checks an image result arriving after a newer
selector, single release without saving the old image, and a print taken during
recording: the badge is absent at raw frame capture and restored afterward.
Existing selector, thumbnail, countdown, native text input, editor and recording
checks still pass. Recording is simulated below the SDK; image capture/cropping
and protected-window PNG checks use the real compositor. Representative panel
and countdown captures were inspected. Sixteen source/locale files match the
Fedora execution tree byte for byte, and eight switcher PNGs match the preceding
panel delivery. Three installed software/offline idle samples pass enforced
limits: medians 12.500 poll calls/s and 104.117 MiB RSS, below 15 calls/s and
120 MiB. No causal performance improvement or physical recording is established.
The previously documented isolated facts fixture still needs maintenance;
these integration tests exercise the production resident-effect bridge.

## Native output policy

`apps/screenshots/output.rs` owns image/recording/annotation destinations,
translated folder/base names, local timestamps, extensions, numbered collisions,
clipboard choice and recording microphone/app-name options. The resident capture
service reads native preferences and locale; caller preference/time overrides
are not used. `stamp`, `captureDir`, `freeName` and `opts` are removed from JSX.
Directory/stat operations still execute through the existing filesystem adapter,
which also supplies the generic user-directory facts. The server's path/access
checks and media backend's refusal to replace existing files remain authoritative.

Output jobs have unique identities and ordered replies. Candidate paths stay
reserved in the context until completion/release, including recording permission
waits and exports that overlap. A second missing-file response cannot adopt a
path another export just reserved; abandoning a pending stat cannot release
another job's name. After the historical 999 numbered candidates, timestamp
fallbacks remain bounded. Unicode source names and case-insensitive `.png`
suffixes are preserved, and annotations always choose a separate destination.

Rust folds output completion and adds only successfully saved new captures to
recent history. Duplicate/stale completion cannot append it twice. A failed save
or copy has no saved path; its thumbnail displays the native failure caption and
stays available for editing instead of claiming the image was copied. Successful
clipboard captures retain their existing thumbnail and caption. Recording event
transport/terminal-state handling remains pending below.

Evidence is under `build/validation/screenshots-output/`: 28 app tests, 722 shell
tests (seven ignored), 105 historical geometry cases, GPU=1 build, platform/ABI,
eight LPK contracts, private installation and the installed switcher check pass.
New tests cover translated/Unicode names, concurrent missing-file responses,
reservation ownership, release, duplicate/stale callbacks, the collision limit,
clipboard/recording/annotation choices and native preferences/completion history.
All 32 app GUI scenarios and eight switcher scenarios pass. The eight-case panel
matrix additionally injects a failed save, checks the native failure caption and
unchanged history, and opens the same image for editing through its thumbnail.
Failure captures were inspected in Axia light and Adwaita dark. Eighteen
source/locale files match the Fedora execution tree byte for byte; all eight
switcher PNGs match the preceding controller delivery. Three installed
software/offline idle samples pass enforced limits: medians 12.300 poll calls/s
and 104.713 MiB RSS, below 15 calls/s and 120 MiB. Recording remains simulated
below the SDK; image/crop/protected-window PNG checks use the real compositor.
These results do not establish a causal performance improvement or physical
recording. The previously documented isolated facts-fixture limitation remains.

## Native shortcuts, refusal and recording events

`shortcuts.rs` supplies capture key metadata and validates the selected desktop
application. The resident service reads the current preference for each key:
PrintScreen opens the area/window/monitor/desktop selector, direct screen capture
retains its separate key, and the panel key stops/cancels this app's recording
before considering an external application. Escape is scoped to the current
countdown and still precedes a focused native text field. JavaScript transports
the native bindings and effects; registration is restricted to the root session.

`feedback.rs` formats refusal notices with the native locale and structured
error parameters. The resident owner deduplicates each attempt, ignores
superseded replies, and uses the native notification service/preferences for
delivery and toast fallback (DND, hidden banners or blocked delivery). The
Privacy action belongs to native notice identities and opens Settings without
changing the capture permission. The SDK notification bridge retains ordinary
application callbacks and their original receiver.

The session model classifies recording events. `done` with explicit success is
required to append a video; `closed`, failed results and terminal `state`
messages clear the matching recording without inventing a saved result.
Progress/error messages alone do not prematurely terminate it. Duplicate/old
events cannot clear a newer source. The resident service owns accepted sources'
output reservations, releases them at terminal completion, and chooses when to
close the badge. The SDK still subscribes to the media source and executes its
close operation, preserving microphone-indicator cleanup.

This is still a partial app migration. SDK image/recording operations remain
in the transport, along with compiled metadata and wrapper removal below. The
thumbnail-to-editor handoff still has its existing short transport timeout.

## Remaining work

User requirement (2026-11-23), implemented above: **PrintScreen opens the visual capture selector**,
with area, specific window, monitor and full desktop options. It must not save a
full-screen image immediately. The selection highlights the target, Enter
confirms and Escape cancels. Keep direct capture shortcuts separately. Monitor
selection must use actual display geometry and respect scaling and negative
desktop origins; synthetic display fixtures do not prove physical multi-monitor
behavior. The older PrintScreen-to-full-capture binding has been replaced.

User requirement (2026-11-23), preserved: retain the existing saved-capture thumbnail in the
bottom-right corner, including click-to-edit, hover-aware expiry and clipboard
captures. The new selector must not replace or remove that feedback.

The selector snapshot now explicitly covers the logical desktop. The
no-rectangle backend screenshot follows the primary-display/capture-all
setting and can return physical primary-display dimensions; it cannot be treated
as a logical full-desktop image for multi-monitor selection. Check actual source
dimensions and display transforms instead of assuming a 100% single display.

- Done: compiled metadata (`manifest.rs`, registered in `rust/native_apps.rs`)
  and removal of the JSX/JSON entry points. All app scenes are native,
  including the panel, selector, editor and auxiliary windows. The window
  mounting and media/window transport moved unchanged in behavior to the
  shared `runtime/native_capture.js`; the runtime tests import it directly.
- Complete the resident controller's remaining I/O transport. Shortcut routing,
  refusal presentation, privacy actions, window hiding, editor preservation,
  generation checks, countdown and cancellation are native.
- Move the remaining recording subscription and stop/close-call transport into
  native services. Event classification, identity/generations, terminal cleanup
  and recent results belong to Rust; preserve SDK device-indicator cleanup.
- Native image/file services: filename policy, output reservations and result
  folding are native, along with the markup scene, input, tools, colors,
  thickness, preview operations, undo and result deadlines. Migrate the remaining
  SDK filesystem/image transport together with the capture controller,
  preserving original files and exact resource ownership across async replies.
- Required graphical coverage of the complete flows at minimum/default sizes,
  light/dark and Axia/Adwaita; source comparison, build, platform/ABI and LPK
  regression checks, installed checks and idle limits. Physical measurements
  remain deferred to the roadmap's end.
