# Native app migration

Goal: migrate every application under `apps/` to Rust, preserving its existing
features and service/permission boundaries. Commit each reviewable stage and
continue. Model-only conversion is an intermediate state, not completion.
The compositor rebuild and graphical validation are reserved for the end.

Current compiled registry: `rust/native_apps.rs`. About, About App, Calculator,
Characters, Clock, Files, Force Quit, Help, Logs, Fonts, Monitor and Store have compiled
Rust implementations. Files still has a compatibility mount controlled by
`filesNativeView`; its native implementation and shared dialogs need to remain
part of the final integration audit.

PDF: geometry, navigation, cache, search/selection, annotation/form state and the
sidebar are Rust. The sidebar mounts native widgets. The main window, page
overlays, toolbar, dialogs and gesture transport still have JSX; PDF is incomplete.

Remaining apps (all remain in scope): backup, calendar, camera, contacts, disks,
editor, mail, notes, passwords, photos, player, recorder, reminders,
scanner, screenshots, settings, sysinfo, sysprobe and terminal. Existing Rust
models in some of these apps do not count as native-window completion.

The asynchronous service lease in `runtime/nativeapps.js` is shared transport,
with no application UI. Rust chooses jobs; existing SDK APIs perform I/O. Jobs
and subscriptions belong to a window. The generic broker isolates results,
discards obsolete jobs, fragments large JSON results and suppresses late
publication after close. Logs and Fonts use it. Font image handles remain in the
window lease after delivery and are released by Rust or on close. Native async
request transport is shared in `native_view/async_jobs.rs`. Internal numeric
leases are independent of string WM IDs (`logs`, `fonts#2`); a real React/runtime
mount test covers startup, two windows, close and name reuse:
`node server/tests/native_async_mount_test.mjs`.

Before final completion, audit every app folder and registry entry, native host
registration, remaining JSX/JS policy, complete feature coverage, lifecycle and
resource ownership, tests, packaging/catalogs and the final desktop build. Keep
parallel edits outside each commit by adding explicit filenames and committing
only those paths; never include another agent's staged changes.
