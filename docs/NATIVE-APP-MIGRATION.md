# Native app migration

Goal: migrate every application under `apps/` to Rust, preserving its existing
features and service/permission boundaries. Commit each reviewable stage and
continue. Model-only conversion is an intermediate state, not completion.
The compositor rebuild and graphical validation are reserved for the end.

Current compiled registry: `rust/native_apps.rs`. About, About App, Calculator,
Characters, Clock, Files, Force Quit, Help, Logs, Fonts, Scanner, Recorder, Backups, Camera, Notes, Photos, Monitor, Store, Sysprobe, Reminders, Contacts, Calendar, Settings, Screenshots, Passwords, PDF, Terminal and Player have compiled
Rust implementations. Files browser windows and icons always use the native
host; `filesNativeView` and the React browser path are removed. Its shared
selectors and transfer windows still use the React widget adapter, pending
shell roadmap phase 3. The complete application remains part of the final
integration audit.

PDF now has only Rust sources in its folder, including its compiled manifest
(`apps/pdf/manifest.rs`). `native.rs` is the whole window: document states (welcome,
opening, password, error), toolbar, sidebar, page rasters, wheel/keyboard/zoom/rotation/mode
navigation, links, text selection and copy, streamed search, go-to-page, highlight/ink/note
tools, in-place form and note editors, undo, save a copy, print, the unsaved indicator and the
four application menus; `native_ui.rs` builds the scene. The shared
`runtime/native_pdf_services.js` (no UI, no policy) runs the helper calls Rust names and keeps
document sessions and raster handles in the window lease, releasing each raster exactly once.
Tests: 58 crate tests with a fake helper, a real-host mount test in `axia-shell`,
`node server/tests/native_pdf_services_test.mjs`, and the GPU CI runtime test
`pdf_runtime` against the real `axia-doc`. The window does not yet persist its zoom, view
mode and sidebar across a remount of the same window.

Screenshots now has only Rust sources in its folder, including its compiled
manifest (`apps/screenshots/manifest.rs`). Its panel, selector, countdown,
recording badge, thumbnail and editor windows are Rust views; the JSX entry
point and `manifest.json` are gone. The media-handle leases, window-instance
mounting and effect transport live in the shared `runtime/native_capture.js`
(no capture policy, no UI), pending the Phase 6 removal of session JavaScript.
Passwords now has only Rust sources, including its compiled manifest
(`apps/passwords/manifest.rs`, still `process: isolated`, protected and
single-window). The vault's requests/events, the descriptors the system dialog
opened (owned by the window that picked them, used up by one request, closed
with the window), the FIDO permission order and the secret clipboard are the
shared `runtime/native_vault_services.js`; the generic native mount publishes
the vault availability and region and marks the window protected.
`node server/tests/native_vault_services_test.mjs` covers that boundary.
Player now has only Rust sources, including its manifest (`apps/player/manifest.rs`) and
window. Rust owns the library (walk on a worker thread, tag cache compatible with the old
`music.json`, grouping by artist and album), the queue with shuffle and repeat, the
virtual song list, the time and volume bars (hit-tested from the layout geometry), `.srt` and
in-stream subtitles, audio track choice, full screen, picture in picture, file drop, "Open with"
and the Control Center entry. The shared runtime gained `media.probe`, `media.nowPlaying`,
`windows.screen` and `windows.pip`, all transport: the entry's buttons come back as window
events and it leaves with the window. Full screen and picture in picture may mount the window
again; it saves queue, view, volume and position first and resumes from them. Tests: the model
and window suites in `apps/player` and `node server/tests/native_media_test.mjs`. A GPU runtime
test with a real file, the picture in picture frame in the compositor and graphical checks
remain for the final desktop validation.

Text Editor is native: document, commands, highlighting, search, window and confined file
access are Rust (see [EDITOR.md](EDITOR.md)). The runtime gained transport-only services:
save and folder modes for `files.pick`, `windows.closeGuard` and `windows.close`. Graphical and
input-method checks remain for the final desktop validation.

No application folders remain to convert from JavaScript/JSX. Mail's complete
native window is described below. Remaining validation and session adapters
are tracked in `ROADMAP-SHELL-RUST.md`; Rust models alone still do not count
as native-window completion.

Contacts now has only Rust sources, including its manifest, full three-column
window and icon. Rust owns names, multiword search, book-scoped groups, target
book selection, lossless vCard edits, virtualized rows and RTL scrollbar capture,
contact cards, repeated fields/custom types, photos, birthday calendar, Unicode
notes, keyboard navigation, confirmations, localized captions and window state.
The controller owns reload epochs, save/group-join sequencing, imports and
photo-ticket lifetime decisions. A failed group join retries the saved UID;
late results cannot replace a newer editor. Rust selects the PIM event topics
so unrelated mail traffic does not fill its bounded subscription queue.
The shared PIM I/O adapter keeps selected files and returned images in their
window lease; it validates file ownership and releases cancelled/late results.
Native photo nodes bind through the existing sandboxed mail image service.
The 38 Contacts Rust tests and shared civil-date tests pass, including real native-host field editing,
default-book/group creation, membership, Mail composition, photo binding,
import errors and file release. The full `axia-shell` library suite passes
924 tests (13 ignored). Theme, app-host and isolated-app bundles build, with
Contacts generated from the compiled registry. CardDAV/device and graphical
validation remain part of the final desktop check.

Calendar now has only Rust sources, including its manifest and complete native
day/week/month window. Rust owns calendar visibility and colors, local calendar
creation/editing/deletion, date navigation, overlapping timed events, all-day
events, responsive sidebars, keyboard shortcuts and persisted window state.
The event editor handles recurrence and occurrence/series deletion, alarms,
time zones, attendees, organizer, URL/status and Unicode notes. Failed saves
retain the draft; request generations prevent late results from replacing a
newer editor or visible range. Civil-date arithmetic is shared with Contacts.
All 27 Calendar Rust tests pass, including native-host editing, saving, alarm
opening, failure recovery and recurring-event deletion. The full `axia-shell`
library suite passes 951 tests (13 ignored). Theme, app-host and isolated-app
bundles build; Calendar is generated from the compiled Rust registry.
CalDAV/device and graphical validation remain part of the final desktop check.

The asynchronous service lease in `runtime/nativeapps.js` is shared transport,
with no application UI. Rust chooses jobs; existing SDK APIs perform I/O. Jobs
and subscriptions belong to a window. The generic broker isolates results,
discards obsolete jobs, fragments large JSON results and suppresses late
publication after close. Logs, Fonts, Scanner, Recorder, Backups and Camera use it. Document image handles remain in the
window lease after delivery and are released by Rust or on close. Native async
request transport is shared in `native_view/async_jobs.rs`. Internal numeric
leases are independent of string WM IDs (`logs`, `fonts#2`); a real React/runtime
mount test covers startup, two windows, close and name reuse:
`node server/tests/native_async_mount_test.mjs`.

Scanner now mounts its entire native window and icon. Rust owns LAN device
selection, capabilities/defaults, preview/crop capture, scan settings, localized
non-overwriting names, conversion and save completion. Existing SDK APIs retain
eSCL validation, sandboxed image decoding/conversion and printing. A direct copy
reports success only after its filesystem job succeeds. Frozen original-policy
fixtures cover scan options/crops and regional date/time captions; native-host
tests exercise discovery, device selection, preview and image release. Physical
scanner and graphical checks remain reserved for the final desktop validation.

Recorder now mounts its complete native window and waveform raster. Rust owns
record/play/pause/stop, microphone choice, live levels, file list/rename, keyboard
shortcuts, trim capture, delayed seek and transcoding policy. SDK adapters retain
microphone permission prompts and helper I/O. Window leases close sessions and
listeners, including permission results arriving after close. Raw sound/media
events are reduced in Rust; active playback can keep a 100 ms background cadence
to enforce the trim endpoint. Fixtures compare 80 waveforms against original JSX
pixels; native-host tests cover selection, playback and trimmed WAV export.
Audio-device and graphical validation remain part of the final desktop check.

Backups now mounts its complete native window. Rust owns status captions and
progress, destination/schedule/exclusion settings, encryption form validation,
snapshot/date navigation, directory generations, restore options, confirmations
and opening snapshots in a new Files window. SDK APIs retain the session engine,
notifications and encrypted-storage operations. Status subscriptions close with
the window. Native password fields mask text and prevent copying/cutting secrets.
Fixtures compare 73 status captions with the original window and 426 byte-size
captions with the SDK, including decimals and terabytes. Tests exercise the
native browse/restore/settings flow and configuration-before-key-creation order.
Actual backup devices and final graphical/build checks remain pending.

Camera now mounts its complete native window. Rust owns device choice and the
3 s hot-plug poll, preferences (mirror, device) written through `settings.patch`,
photo/video mode, collision-free names captured at shutter time with the mirror
setting of that moment, the record timer, flash, recent shots and thumbnail
requests. SDK adapters keep permission prompts, V4L2 capture and encoding. The
media lease releases the camera and, for an active or pending recording, its
microphone indicator before closing. Rust tests cover naming, mirroring, device
switches and late results; service tests cover session ownership and release.
A physical webcam and graphical checks remain part of the final desktop check.

Notes now has only Rust sources, including its manifest and emoji catalog.
Rust owns the complete native window and desktop cards, multiline Unicode editor,
selection/undo/Markdown formatting, reader/outline/tasks, note search, file picker,
attachments/duplicates, and the original Markdown/YAML persistence. The shared
session publishes widget metadata at startup; it continues polling attachment
workers and flushing saves while no window is open. Generic runtime services
only carry clipboard, input-method, WM menus/events and printing I/O. Tests cover
frozen original storage outputs, Unicode and long-document editing, attachment
publication, multiple mounts, widget geometry, composition, native-host focus
and saving, and session restoration before opening a window. Graphical and input
device checks remain reserved for the final desktop validation.

Mail now has only Rust sources. Rust owns both windows: accounts and folders, the
virtual message list, the sanitized HTML reading pane, invitations, attachments, the
sheets for accounts, certificates, passwords and links, and composing with
suggestions, drafts, scheduled sending and a close guard. The body editor is the
Notes editor with the theme's colors. Transport services carry helper requests, the
accounts file and keyring, leased descriptors and attachment opening. Model and
window tests run against a pretend helper; the compositor test replaces `lord.mail`.
Real IMAP, SMTP and keyring use and graphical checks remain part of the final
desktop check.

Before final completion, audit every app folder and registry entry, native host
registration, remaining JSX/JS policy, complete feature coverage, lifecycle and
resource ownership, tests, packaging/catalogs and the final desktop build. Keep
parallel edits outside each commit by adding explicit filenames and committing
only those paths; never include another agent's staged changes.
