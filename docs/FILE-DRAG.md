# File drag and drop

The controller and its tests are Rust:
`rust/axia-shell/src/wayland/dnd_protocol.rs`, `dnd_protocol/tests.rs` and
`dnd.rs`. `server/src/wl/dnd.h` is only the ABI used by the remaining C
compositor. The clipboard keeps its existing implementation and offers.

## Contract for the Files UI migration

- A native scene node opts into receiving files through
  `node_set_file_drop(node_id, token)`. Zero unregisters it. Tokens are scoped
  to the receiver's lifetime. `node_file_drop(hit)` resolves the nearest
  registered ancestor using the compositor's existing hit test.
- The high token bit marks a target accepting only native move gestures
  (Trash). The controller removes that bit before delivery. Other targets
  accept external sources only as COPY.
- Starting a native pointer drag calls `dnd_start_files(paths_json, node_id,
  internal_move)`, once after the threshold. It requires a held left button.
  It exports `text/uri-list`, COPY only, to other Wayland clients. The private
  internal move flag never asks an external application to delete a source.
- `dnd_events` drains completed events outside the controller's state borrow.
  `filedrop` carries `{token, paths, native}`; `native` here means the private
  **internal move flag**, not permission to delete an external source.
  `dragend` releases the native source's capture. There are no motion events
  or periodic interface callbacks from this controller.
- The Files Rust model accepts `action: ["dropFiles", destination]`, `paths`
  and `internalMove`. Omitting destination uses its current writable folder.
  It uses the existing bounded transfer workers and progress window. A
  destination of `trash` requires an internal move. Directory tiles, mounted
  volumes and sidebar folder shortcuts advertise their destination as
  `dropPath`; this is model metadata, independent of the renderer.
- The current React mount has a small adapter in `sdk/filedrag.js` and
  `sdk/rustfiles.js`. A native Rust mount can call the controller directly and
  consume its completed events with a Rust callback. The protocol owns no
  React instances, window manager state or filesystem operations. Do not
  replicate the drag state machine in the new renderer.

## Work and resource bounds

Motion uses stable session storage with no Rust allocation, filesystem call,
payload serialization or JavaScript dispatch. The main loop suppresses normal
pointer dispatch during the protocol grab. The existing cursor path displays
small client drag icons; it adds no animation worker.

Wayland-to-Wayland payloads go directly through descriptors. Native URI data is
immutable and shared between writers. Each readiness callback reads/writes at
most 64 KiB; backpressure returns control to the existing Wayland event loop.
There are no stream threads. Limits are 1 MiB of URI payload, 4,096 paths,
8 sessions, 64 offers and 16 streams (at most 8 native writers). Stalled streams
expire after 15 seconds. Post-drop client transactions have a 30-minute ceiling;
this does not time out the separate Files copy workers. The expiration timer is
disarmed when no streams or dropped transactions remain, including while only
an active pointer gesture exists.

Tests use private Wayland resources and socket pairs. They cover both transfer
directions, direct descriptor forwarding, malformed/remote URIs, backpressure,
stale offers, resource destruction, action negotiation, source ownership,
cancellation and 100,000 motions without interface events or storage growth.
Files model regressions use private temporary directories only.

This implements mouse/pointer DND for native Wayland clients and opted-in
shell views. The X11 XDND bridge through XWayland, touch-initiated drags and
portal file-transfer tokens remain separate work. A graphical GTK/Qt session
still needs manual integration testing before claiming broad application
compatibility. The protocol reference is the
[Wayland core specification](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_data_device).
