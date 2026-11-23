# Screenshots migration

The app is still in migration. `apps/screenshots/index.jsx` retains the panel,
markup presentation, auxiliary windows and capture/recording transport. The icon
and visual selector are native Rust views in `selector.rs`. Selection and annotation
coordinate calculations call `axia-app-screenshots::geometry` through the
compiled native model bridge. `markup::Editor` owns the tool, color, operation
history, draft stroke, text input and undo. Each editor has a separate native
model. There is no JavaScript geometry or markup-policy fallback.

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

- Compiled metadata and native panel: four capture modes,
  persisted timer/clipboard/microphone options, protected-window indication,
  recent files and the menu to protect the front window.
- Capture controller independent of a mounted panel: hide capture windows before
  taking the frame; preserve editors; countdown and cancellation; permission
  refusal with a privacy action and toast fallback; chosen external screenshot
  app and the global shortcuts, including stop-recording priority.
- Native thumbnail presentation with hover-aware expiry
  and explicit image ownership transfer into the editor.
- Native countdown and recording indicator, elapsed time, stop and completion,
  recent recording results, SDK microphone indicator cleanup and late completion
  handling. No unnecessary work while idle.
- Native markup presentation and image transport: state, tools, colors,
  thickness, preview operations, text submit/cancel and undo are already native.
  Migrate the actual toolbar/canvas, save/copy and transient result captions.
  Preserve originals, reuse/release preview handles, and dispose image
  resources exactly once across close, replacement and pending async replies.
- Required graphical coverage of the complete flows at minimum/default sizes,
  light/dark and Axia/Adwaita; source comparison, build, platform/ABI and LPK
  regression checks, installed checks and idle limits. Physical measurements
  remain deferred to the roadmap's end.
