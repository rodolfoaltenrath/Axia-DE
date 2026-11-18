# Rust build boundaries and memory

Native apps compile as `rust/axia-app-*` crates. Their authored sources remain in
`apps/`, with a crate entry point under `rust/`; asset paths, application IDs and
packaging layout stay stable. Terminal, Sysinfo and PDF currently provide models
to their existing adapters. Their remaining UI migration is separate work.

`axia-native` owns the `NativeApp` contract, async job transport, translations,
date/time utilities, paint, icon descriptors, Markdown and style tokens.
`axia-fs` owns filesystem policy, trash and cancellable operations. `axia-clock`
owns the session clock engine. Apps depend on those services and codecs rather
than the shell or another app.

`axia-shell` retains the scene, native host, platform adapters, app registry and C
bridge. UI-thread callbacks in `axia-native::services` supply app creation,
metadata, desktop catalog snapshots, network requests, window policy and text
measurement/direction. Callbacks are copied out before invocation to permit
nested app/icon requests. The scene and its pointers never cross this interface.
The host installs callbacks before supplying an app's resources.

Small `manifest.rs` files remain the single metadata source used by both apps and
`rust/native_apps.rs`. The packaging `xtask` reads these files without depending
on all app implementations. `libaxia_shell.a` remains the compositor's archive,
including the C entry points defined in dependent crates.

App unit tests compile in their respective crates. Scene/host integration tests
live under `axia-shell/src/native_view`; external runtime tests remain in
`axia-shell/tests`. Run `make check GPU=1` to exercise the apps, shell, platform
checks and packaging smoke checks. `server/Makefile` rebuild inputs include all
new crate manifests and recursive Rust source paths. Benchmark host builds wait
for the compositor archive, preventing two large shell builds at once.

The RPM's `test-platform SKIP_RUST_TESTS=1` keeps C and ABI tests. Rust tests,
including the ignored image runtime test, run in desktop CI before the RPM job.
The RPM additionally runs `tp-test` and `check.py --require-gpu`. Standalone Copr
builds therefore rely on the desktop CI for Rust test coverage. Rust compilation
uses two jobs in the RPM and desktop CI. Fedora's optimization, debug information
and hardening flags are preserved; lowering debug information requires measured
evidence first.

`tools/desktop/build_memory.py` wraps desktop and RPM builds in CI and uploads
JSON reports even on ordinary build failures. It samples process-tree RSS and
the largest individual `rustc` every 200 ms, warning above 11 GiB total or 5 GiB
per compiler. RSS may count shared pages multiple times and sampling can miss
brief spikes; these figures are not an exact runner memory high-water mark.

Future C migrations should enter a dedicated service crate or an existing codec
crate. Add an app to its own crate, register it in the shell, and keep its tests
with it. Avoid new production `#[path]` inclusions of app implementations in the
shell. Review additions above 3,000 lines to the shell for extraction. Split
remaining platform code only if measured compiler memory still exceeds the
5 GiB target. Compare Fedora RPM reports over time; local developer builds with
different flags are not a substitute for those measurements.

## Local validation, 2026-11-18

Fedora 44 under WSL, Rust/Cargo 1.98.1, eight CPUs, 7.7 GiB RAM and 2 GiB
swap. The release builds retained Fedora's `opt-level=3`, `debuginfo=2`,
`codegen-units=1`, `strip=none` and frame pointers.

| Build | Peak single rustc RSS | Peak process-tree RSS |
| --- | ---: | ---: |
| Cold shell release with Fedora Rust flags | 2.62 GiB | 2.66 GiB |
| Complete `rpmbuild --rebuild` | 2.63 GiB | 5.42 GiB |

The complete RPM rebuild passed, including installation, C/ABI checks, touchpad
and `check.py --require-gpu`; the main, debuginfo and debugsource RPMs were
generated. No `cargo test` ran inside the RPM. The 33 C exports moved into
dependent crates remained in `libaxia_shell.a`.

Desktop validation passed `make check GPU=1`, 1,112 workspace Rust tests,
19 xtask tests, 20 Python packaging tests, the three ignored application runtime
integrations, the ignored C layout check, engine tests and staged installation.
Only the local desktop development/test profiles disabled debug information to
save disk space; the RPM used the complete Fedora flags above.

The RPM took 41.19 minutes in this shared WSL session, with other validation
running concurrently and temporarily taking CPU priority. This is not a clean
before/after timing comparison or a GitHub runner benchmark. The CI target of
less than 30 minutes still needs verification on the actual runner. Future CI
artifacts provide the comparable measurements; the local reports and RPMs are
under the ignored `build/validation/shell-split/` directory.
