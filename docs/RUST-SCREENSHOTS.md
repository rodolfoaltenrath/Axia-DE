# Screenshots migration

The app is still in migration. `apps/screenshots/index.jsx` remains the
presentation and capture/recording controller. Selection and annotation
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

Remaining work before removing JSX and marking the app complete:

User requirement (2026-11-23): **PrintScreen opens the visual capture selector**,
with area, specific window, monitor and full desktop options. It must not save a
full-screen image immediately. The selection highlights the target, Enter
confirms and Escape cancels. Keep direct capture shortcuts separately. Monitor
selection must use actual display geometry and respect scaling and negative
desktop origins; synthetic display fixtures do not prove physical multi-monitor
behavior. The older PrintScreen-to-full-capture binding is to be replaced during
this migration.

User requirement (2026-11-23): retain the existing saved-capture thumbnail in the
bottom-right corner, including click-to-edit, hover-aware expiry and clipboard
captures. The new selector must not replace or remove that feedback.

The initial selector snapshot must explicitly cover the logical desktop. The
current no-rectangle backend screenshot follows the primary-display/capture-all
setting and can return physical primary-display dimensions; it cannot be treated
as a logical full-desktop image for multi-monitor selection. Check actual source
dimensions and display transforms instead of assuming a 100% single display.

- Native icon, compiled metadata and complete panel: four capture modes,
  persisted timer/clipboard/microphone options, protected-window indication,
  recent files and the menu to protect the front window.
- Capture controller independent of a mounted panel: hide capture windows before
  taking the frame; preserve editors; countdown and cancellation; permission
  refusal with a privacy action and toast fallback; chosen external screenshot
  app and the global shortcuts, including stop-recording priority.
- Native area/window overlay with Space/Escape, topmost window hover, dimensional
  hint and rejected tiny selections. Native thumbnail with hover-aware expiry
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
