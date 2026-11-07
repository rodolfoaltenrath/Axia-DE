# Clipboard history in Rust

`rust/axia-shell/src/clipboard/` owns the session history: identity, text/image
classification, query/filter/pin ordering and counts, deduplication, limits, expiration, ignored origins,
persistence and managed image cleanup. `sdk/clipboard.js` retains the existing
public API, QuickJS projection and React panels. The panel geometry, shortcuts,
theme tokens and glass/rendering paths are unchanged.

The C terminal/Wayland clipboard remains the transfer authority. On a completed
copy the compositor sends the text or an opened image descriptor to Rust.
Password fields and the Wayland password-manager hint are rejected before
image descriptors are opened, and Rust checks sensitivity again. Ignored apps
are filtered on the worker before any image data is read or saved. Re-copying
suppresses only the exact native generations it generated, not the next copy.
The old JavaScript history engine and synchronous C history image-copy/deletion
functions were removed. Application image transfer (`imageData`, `useImage`)
remains in the existing compositor and is not a second history implementation.

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
not schedule disk writes. React resolves visible IDs and memoizes the result. Row summaries and Unicode
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

`make check` additionally runs a real QuickJS/React fixture through the C capture,
Rust worker and eventfd. It loads a legacy text/PNG index in a private HOME,
re-copies an image through the real compositor, excludes passwords and ignored
apps, verifies the next normal copy, limits/pins, panels and compatible writes.
It does not modify the live user's clipboard, history or installed package.
Physical Ctrl+C/Ctrl+V between actual applications remains a session validation.
