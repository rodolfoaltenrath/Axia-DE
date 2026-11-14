# Isolated app mode in Rust

`server/src/app/` has no C implementation sources. `rust/axia-shell/src/lib.rs`
includes its Rust module; `server/Makefile` tracks these sources and links the
existing `axia-shell` archive. The public `app.h`, `probe.h` and `apphost_bc.h`
headers remain the ABI used by the compositor and the QuickJS host.

| Source | Responsibility |
| --- | --- |
| `spawn.rs` | Validated package lookup, system-package precedence, the broker socket, inherited channels, orphan process launch and broker registration |
| `client.rs` | App startup and sandbox ordering, 16 toplevels, geometry, shared-memory buffers, frame callbacks, pointer/keyboard/gesture events and polling |
| `wire.rs` | Opaque libwayland-client handles and requests matching the generated protocol ABI |
| `bytecode.rs` | Verified source loading, background compilation, sealed memfd publication, read-only descriptors and source/hash validation before evaluation |
| `probe.rs` | The system `sysprobe` helper, sandbox refusal checks and broker report protocol |
| `tests.rs` | Package selection, manifest bounds, frame policy, titles, inherited environment, seals and broker framing |

The former `appclient.c`, `appspawn.c`, `apphost_bc.c` and `probe.c` were deleted.
`server/src/ui/js_app_host.c` is a small ABI bridge: it converts QuickJS values,
registers `lord.appWindows`, compiles/evaluates QuickJS objects, and accesses the
C renderer structures. It contains no process, window-lifecycle, sandbox,
package-selection or bytecode-validation policy. QuickJS and the existing
renderer remain shared dependencies; their implementations are outside this
migration.

## Preserved boundaries

- Only an installed, validated package ID reaches `/proc/self/exe --app <id>`.
  There are no caller-provided commands or additional arguments.
- A trusted system package precedes an installed user package. The vault channel
  is attached only for the system `passwords` package. User packages do not gain
  the system-package broker identity.
- Environment strings and descriptor arguments are prepared before `fork`.
  The child performs only libc operations before `execve`; inherited channels
  are made executable only in the child, and `DISPLAY` is removed.
- Wayland and fonts are initialized before the existing sandbox is applied;
  application code loads afterward. The `sysprobe` helper still forks before
  the app sandbox and takes the existing helper sandbox itself.
- Bytecode is compiled only from bytes returned by `codesig_load`. Its original
  `LORDQBC1` header remains compatible. Consumers require an immutable sealed
  descriptor, matching source length and SHA-256, and an exact bytecode length.
  A cache miss falls back to parsing source. An exception during evaluation is
  reported without executing the source a second time.
- Buffers and dimensions are bounded; frame callback proxies and mappings are
  released when their window closes. Titles are bounded without splitting UTF-8.

## Validation

Run the Rust policy tests with:

```sh
cargo test --manifest-path rust/Cargo.toml -p axia-shell --lib isolated_app --locked --offline
```

Build and run the C ABI integration fixture with:

```sh
make -C server build/app-mode-ffi-test
server/build/app-mode-ffi-test
```

The fixture links the production Rust archive, real QuickJS, real keyboard
serialization and the ABI bridge. A deterministic compositor drives configure,
frame/release, pointer, keyboard, pinch and swipe listeners. It exercises multiple
windows, capacity limits, node attachment, recovery, sandbox-before-code ordering,
broker registration and descriptor cleanup. Launch syscalls are substituted;
no real isolated application or sandbox probe is launched by this fixture.
Bytecode tests cover successful execution, changed source, corrupt bytecode,
unsealed/ordinary files, retained runtime exceptions and the background cache.

The full compositor rebuild, live Wayland display and real sandbox probe remain
part of the final desktop/VM validation.
