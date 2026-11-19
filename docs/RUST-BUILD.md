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
new crate manifests and recursive Rust source paths. The compositor and native
benchmark share `server/build/rust` through `RUST_TARGET_DIR`. Benchmark host
builds wait for the compositor archive and reuse its compiled shell and app
dependencies with the same Cargo profile, features and Rust flags. This avoids
compiling the shell twice during desktop and RPM builds.

The benchmark archive linked into the C compositor and testbench stays in
`server/build/bench-rust`. Keep it separate: the native benchmark enables
`native-host`, which bundles engine code and stubs that must not enter those
C consumers. The native benchmark's release integration tests use the shared
directory too, though Cargo still builds dependencies separately when test
profile requirements differ (for example, panic unwinding).

The RPM's `test-platform SKIP_RUST_TESTS=1` keeps C and ABI tests. Rust tests,
including the ignored image runtime test, run in desktop CI before the RPM job.
The RPM additionally runs `tp-test` and `check.py --require-gpu`. Standalone Copr
builds therefore rely on the desktop CI for Rust test coverage. Rust compilation
uses two jobs in the RPM and desktop CI. Fedora's optimization, debug information
and hardening flags are preserved; lowering debug information requires measured
evidence first.

`cargo xtask build-memory --output report.json -- <command> ...` wraps desktop
and RPM builds in CI and uploads
JSON reports even on ordinary build failures. It samples process-tree RSS and
the largest individual `rustc` every 200 ms, warning above 11 GiB total or 5 GiB
per compiler. RSS may count shared pages multiple times and sampling can miss
brief spikes; these figures are not an exact runner memory high-water mark.
The collector and its unit/integration tests are Rust. The measured command's
exit code is preserved, including the `128 + signal` convention. Spawn failures
also write a diagnostic report. SIGINT/SIGTERM cancellation terminates the
private build process group and writes a report; SIGKILL cannot be intercepted.
The initial compilation of the small `xtask` itself precedes measurement.

Future C migrations should enter a dedicated service crate or an existing codec
crate. Add an app to its own crate, register it in the shell, and keep its tests
with it. Avoid new production `#[path]` inclusions of app implementations in the
shell. Review additions above 3,000 lines to the shell for extraction. Split
remaining platform code only if measured compiler memory still exceeds the
5 GiB target. Compare Fedora RPM reports over time; local developer builds with
different flags are not a substitute for those measurements.

## Rust build telemetry, 2026-11-18

The Python collector and its tests were replaced by `rust/xtask/src/build_memory.rs`
and Rust integration tests. Desktop and RPM CI retain the same artifact paths,
200 ms interval and warning thresholds. The collector runs from the existing
lightweight `xtask`; it does not depend on the shell or application crates.

Validation passed 41 Rust tests, including process-tree filtering, `/proc/stat`
parsing, exit code 7, signal exit 143, missing executables and cancellation with
descendant cleanup. A real, initially empty `xtask` build target on Fedora WSL,
with two Cargo jobs and low priority, returned success in 10.11 s and recorded
564,273,152 bytes (538.133 MiB) for the process tree and 306,081,792 bytes
(291.902 MiB) for the largest `rustc`. The report is retained locally at
`build/validation/build-memory-rust/compile.json`. This validates collection;
it is not a desktop performance comparison or a GitHub runner measurement.

The later LTO/codegen-unit comparison is recorded in
[RUST-PROFILE-COMPARISON.md](RUST-PROFILE-COMPARISON.md). All three profiles
linked and ran; the evidence supports keeping Fedora's existing release flags.
Changing the collector alone does not justify changing those flags.

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
less than 30 minutes still needed verification on the actual runner at that
point; see the subsequent runner verification below. The local reports and RPMs are
under the ignored `build/validation/shell-split/` directory.

## GitHub runner verification, 2026-11-18

The successful [workflow 44422355912](https://github.com/rodolfoaltenrath/Axia-DE/actions/runs/44422355912)
at `c5e595f1` produced an `rpm-memory` report with exit code zero:

| Measured command | Elapsed | Peak single rustc RSS | Peak process-tree RSS |
| --- | ---: | ---: | ---: |
| `rpmbuild --rebuild` on Fedora 44 | 1,029.05 s (17 min 9 s) | 2.576 GiB | 4.534 GiB |

This is the measured RPM command, including its build/check/install work;
it excludes workflow setup, the earlier desktop matrix, SRPM preparation and
upload/publishing. It verifies a sub-30-minute RPM command on the actual runner,
not a sub-30-minute end-to-end workflow. The same 200 ms sampling limitations
apply. The downloaded report is preserved under the isolated checkout's ignored
`build/validation/profile-compare-resume/rpm-memory-c5e595f1.zip`.

## Native benchmark artifact reuse, 2026-11-18

A sequential local comparison with the same Fedora release flags and two Cargo
jobs built the shell once, then built the native benchmark with `native-host,gpu`
in an empty separate directory and in the populated shell directory:

| Native benchmark build | Elapsed | Compiled artifacts | Reused artifacts |
| --- | ---: | ---: | ---: |
| Separate `build/bench-native` | 563.96 s | 110 | 0 |
| Shared `build/rust` | 21.19 s | 3 | 107 |

The shared build avoided recompiling `axia-shell` and its dependencies. The
compositor archive's SHA-256 remained unchanged. These are local timings for
this step, with a warm OS filesystem cache, rather than a total CI duration or
a GitHub runner measurement. Commands, Cargo artifact records, archive hashes
and memory reports are under the ignored `build/validation/build-reuse/`.

Validation passed 31 Python tests, the compositor/testbench links, the switcher,
snap and window-model C/Rust ABI tests, and all four native benchmark integration
tests with `GPU=1`. The copied `build/lordbench-test` executable also passed all
four tests. The shell archive remained unchanged after these builds and tests.
