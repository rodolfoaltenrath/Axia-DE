# Native app migration

Goal: migrate every application under `apps/` to Rust, preserving its existing
features and service/permission boundaries. Commit each reviewable stage and
continue. Model-only conversion is an intermediate state, not completion.
The compositor rebuild and graphical validation are reserved for the end.

Current compiled registry: `rust/native_apps.rs`. About, About App, Calculator,
Characters, Clock, Files, Force Quit, Help, Logs, Fonts, Scanner, Recorder, Backups, Camera, Monitor and Store have compiled
Rust implementations. Files still has a compatibility mount controlled by
`filesNativeView`; its native implementation and shared dialogs need to remain
part of the final integration audit.

PDF: geometry, navigation, cache, search/selection, annotation/form state and the
sidebar are Rust. The sidebar mounts native widgets. The main window, page
overlays, toolbar, dialogs and gesture transport still have JSX; PDF is incomplete.

Remaining apps (all remain in scope): calendar, contacts, disks,
editor, mail, notes, passwords, photos, player, reminders,
screenshots, settings, sysinfo, sysprobe and terminal. Existing Rust
models in some of these apps do not count as native-window completion.

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

Before final completion, audit every app folder and registry entry, native host
registration, remaining JSX/JS policy, complete feature coverage, lifecycle and
resource ownership, tests, packaging/catalogs and the final desktop build. Keep
parallel edits outside each commit by adding explicit filenames and committing
only those paths; never include another agent's staged changes.
