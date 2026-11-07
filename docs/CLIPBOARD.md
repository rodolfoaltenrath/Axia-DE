# Clipboard transfers, history and panels in Rust

`rust/axia-shell/src/clipboard/` owns native payloads and transfer engines
(`native.rs`), history and persistence (`mod.rs`, `store.rs`), and the quick/full
panel controller and widget trees (`ui.rs`). The panel geometry, shortcuts,
theme tokens and glass/rendering paths stay compatible with the previous UI.

`sdk/clipboard.js` retains the public API, focus/paste effects and QuickJS
projection. `sdk/rustwidgets.jsx` binds the existing shared widgets, text input
and locale formatter to Rust's trees. This is a layout/controller migration;
it does not replace the DE's shared React runtime or C renderer. C keeps Wayland
resource registration, focus checks, X11 wire packets and event-loop integration.
There is no second C text/image store, read/write engine or Latin-1 converter.

Native and history privacy markers remain separate from the actual copied
content: a password can still be pasted into a focused Wayland client while
being excluded from history and X11 exports. Image offers carry the secret hint
as well. Ignored apps are filtered on the worker before image data is saved.
Re-copying suppresses only its exact native generations, not the next copy.

## Native transfers and ownership

Native payloads are owned by Rust on the compositor's event-loop thread.
Readers/writers take ownership of their descriptors, including failure paths.
Wayland only registers/removes event sources and forwards arbitrary MIME types
to a live owner. Rust chooses the cached MIME, handles partial I/O, EAGAIN/EINTR,
EOF, malformed PNG signatures and bounded growth. A failed, oversized or stale
read leaves the last completed copy intact. Clipboard and primary-selection
read epochs are independent; a delayed old owner cannot replace a newer copy.

Each callback handles at most 256 KiB before returning to the event loop. The
limit is 16 MiB per native payload, 64 MiB of charged native payload allocations
(including growth and outstanding snapshots), and 32 concurrent transfer
engines/snapshots. Superseded readers are retired even if their pipes never
become readable. Stalled Wayland transfers expire after ten seconds without
progress; the collector timer is disarmed when no transfer remains.
Outgoing transfers share immutable payloads rather than
cloning one buffer for every paste. Their bytes remain alive if a new copy
replaces the current clipboard during transfer. Text and read scratch buffers
are explicitly wiped when released.

XWayland's property/INCR bridge uses the same bounded Rust accumulators and
snapshots, including Latin-1 conversion. X11 focus/ownership, property messages,
chunk acknowledgements and existing timeouts stay in the X window manager.
Source destruction preserves a completed copy and advertises a fresh fallback
offer; old offer generations remain unreadable. PRIMARY remains independent.

Compressed PNG runtime previews retain `$XDG_RUNTIME_DIR/axia/clip-N.png` and
the last-ten cleanup policy. Preview writing and explicit image re-copy file
loading remain synchronous, as before this migration; this change does not
claim to move those two native operations off the event loop. History/index
I/O remains on the separate worker below. No decoded bitmap is added here.

## Panel ownership and host adapter

Rust owns mode, query/filter, selection, hover, keyboard behavior, clear
confirmation, row windowing, preview geometry, materials, captions and actions.
The host sends only visible row metadata and a bounded selected-text preview;
it never sends the whole clipboard/history back for a hover or key press.
Queries keep using the existing cancellable history worker. Locale formatting,
text-field editing and dispatching paste/focus effects use the shared runtime.
The old JavaScript quick/full panel layouts and interaction state were removed.

A local comparison against `e39ae485`, in private sessions with identical theme,
history and viewport, produced zero differing pixels for both quick and full
panels at 1000×700. This comparison covers those two captured states, not every
theme, scale or physical input device.

## Work and delivery

One session-owned worker survives theme/context changes. It uses a condition
variable, not an idle polling timer. It reads/writes the index, streams compressed
PNG files and prunes images outside the interface thread. Open descriptors keep
previews readable after the compositor removes its temporary name. Image copies
stream compressed bytes without an additional full decoded bitmap.

The queue is bounded to 128 commands and 8 MiB of pending text. Under an extreme
copy flood, older queued captures are replaced by newer copies; control commands
are retained. Ordinary clipboard transfers themselves are not affected.
Batches publish the latest projection. Panel queries run on the worker with
coalescing and cancellation between entries; opening/typing in the panel does
not schedule disk writes. The host resolves only the visible IDs requested by Rust. Row summaries and Unicode
character counts are computed once on the worker, so hovering does not scan
large clipboard strings or allocate an array of every character. Changes wake the main poll through an
eventfd. When idle, the history check is an atomic flag, without file access or
extra wakeups. Only changed entries and the current ID order cross to QuickJS. Once a text
ID is delivered, pin/use/re-copy changes send only metadata, preserving the
existing string in the UI instead of transferring its body again;
coalesced replies retain every change since the last consumed projection.
A new theme subscribes to a full projection without reloading history from disk.

Disk writes are debounced for 600 ms on the worker and use a private temporary
file, complete write, fsync and atomic rename. Shutdown explicitly flushes the
worker, with a two-second maximum wait; this is the only synchronous wait at the
UI boundary. Session exit via a signal uses the same flush and pin policy.
A disk failure or a worker still occupied after that deadline can prevent the
latest changes being saved; no unbounded shutdown wait is introduced.

## Existing data and limits

The existing `~/.local/share/axia/clipboard/history.json` version-1 format,
IDs, pins, Unicode text and owned `img-*.png` references are preserved. That
legacy HOME location is intentionally retained, including when XDG_DATA_HOME
is customized; this change does not relocate user data. Saving off keeps the
session history in memory but writes an empty index. Managed images move to
private runtime files; enabling saving moves them back. Files are mode 0600
and new directories are 0700. Only generated files under the managed directories
can be removed. Links, devices, FIFOs, other owners and unrelated image paths
are rejected. A corrupt, unsupported or oversized index is left untouched;
the session can still collect new items in memory. Explicitly clearing history
or disabling saving permits resetting the index.

Limits are 1 MiB of UTF-8 per text item, 16 MiB total retained text, 8,192 entries,
16 MiB per encoded PNG and a 32 MiB index read/write limit. JSON expansion that exceeds this bound
leaves the previous index intact and reports a save failure. User count, age and disk
limits (including the existing 5,000-item setting) are applied within those bounds. Pins retain their exemption from ordinary
count/age/image-budget eviction. Import that exceeds the hard text/count limits
is refused without overwriting the source index. History stores compressed files
and metadata, not decoded image surfaces.

## Validation

`cargo test --manifest-path rust/Cargo.toml --locked --offline clipboard::`
covers classification, legacy import, pins/order, expiration, queue/text limits,
coalescing, re-subscription, blocked worker behavior, private atomic storage,
PNG descriptor lifetime, image cleanup and saving/logout policies.

`make check` additionally runs a real QuickJS/React fixture through the native
Rust transfer store, C bridge, history worker and eventfd. It loads a legacy text/PNG index in a private HOME,
re-copies an image through the real compositor, excludes passwords and ignored
apps, verifies the next normal copy, limits/pins, actual panel keyboard input
and compatible writes. `tools/desktop/clipboard_native_check.py` also starts a
private full desktop and an independent Wayland peer: 1 MiB text, rich MIME
forwarding, owner exit, stale offers, PRIMARY, password hints and PNG transfer.
These fixtures do not modify the live user's clipboard, history or installed package.
Physical Ctrl+C/Ctrl+V between actual applications remains a session validation.


`python3 tools/desktop/clipboard_x11_check.py` uses an independent Xlib peer in
another private full desktop. It checks 1 MiB INCR in both directions, clipboard
retention after source release, UTF-8/Latin-1 conversion and PRIMARY. It requires
Xwayland and the X11 development library; Fedora CI runs it in the GPU build
job. Rust unit tests cover the same shared payload engine without an X server.
