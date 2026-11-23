# Screenshots migration

The app is still in migration. `apps/screenshots/index.jsx` remains the
presentation and capture/recording controller. Its selection and annotation
coordinate calculations now call `axia-app-screenshots::geometry` through the
compiled native model bridge. There is no JavaScript geometry fallback.

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

Remaining work before removing JSX and marking the app complete:

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
- Native markup state and editor: arrow, box, text, blur and line; six colors,
  thickness, drag preview, text submit/cancel, undo, save/copy and transient result
  captions. Preserve originals, reuse/release preview handles, and dispose image
  resources exactly once across close, replacement and pending async replies.
- Required graphical coverage of the complete flows at minimum/default sizes,
  light/dark and Axia/Adwaita; source comparison, build, platform/ABI and LPK
  regression checks, installed checks and idle limits. Physical measurements
  remain deferred to the roadmap's end.
