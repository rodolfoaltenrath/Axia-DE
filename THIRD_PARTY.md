# Third-party components

LordOS's own code is released under the MIT License (see `LICENSE`). It bundles or links
the components below, each under its own license.

## Bundled in this repository (`third_party/`)

| Component | Used for | License |
|---|---|---|
| QuickJS | JavaScript engine for the UI | MIT (`third_party/quickjs/LICENSE`) |
| EFF large wordlist, SecLists common passwords | passphrase generator, weak-password check | CC BY 3.0 (EFF), MIT (SecLists, `third_party/wordlists/LICENSE-SecLists`) |

## Rust codecs and cryptography (`rust/vendor/`)

The stb headers were replaced by `rust/axia-codecs`. Its dependencies are
vendored for offline, locked builds; their upstream license files are retained.

| Component | Used for | License |
|---|---|---|
| image and its PNG/JPEG/GIF/BMP dependencies | bounded image decoding and PNG/JPEG encoding | MIT or Apache-2.0; see each vendored crate |
| fontdue 0.9.5 | glyph metrics and rasterization | MIT or Apache-2.0 or Zlib |
| ttf-parser 0.25.1 | checked font tables, face metrics and GPOS kerning | MIT or Apache-2.0 |
| flate2 / miniz_oxide | bounded gzip and raw DEFLATE | MIT or Apache-2.0 / MIT or Zlib or Apache-2.0 |
| core_maths / libm | portable font arithmetic | MIT |
| RustCrypto argon2, blake2, chacha20, chacha20poly1305, poly1305 and support crates | vault, keyring, KeePass imports and encrypted backups (`rust/axia-crypto`) | MIT or Apache-2.0; see each vendored crate |
| zeroize | erase temporary keys and work buffers | MIT or Apache-2.0 |
| subtle | constant-time 16-byte comparisons | BSD-3-Clause |

## Data files

| Data | Used for | License |
|---|---|---|
| DB-IP Lite Country (`assets/monitor/geoip-country.mmdb`) | offline IP→country in Monitor | CC BY 4.0 — IP geolocation by DB-IP (https://db-ip.com) |
| Natural Earth (`assets/monitor/land.bin`) | world map in Monitor | public domain |
| Monocypher 4.0.2 test vectors (`rust/axia-crypto/tests/fixtures/`) | compatibility regression data only; no Monocypher code is built or bundled | CC0-1.0 or BSD-2-Clause (`LICENSE-Monocypher-vectors.md` alongside fixtures) |
| Unicode Character Database / CLDR (generated tables) | text, bidi, emoji names | Unicode License v3 |
| Inter (`assets/fonts/Inter-*.ttf`) | UI text | SIL OFL 1.1 (`assets/fonts/Inter-LICENSE.txt`) |
| Plus Jakarta Sans 2.7.1 (`assets/fonts/PlusJakartaSans-ExtraBold.ttf`, `-Medium.ttf`) | the LordOS wordmark: live text in the UI (fontFamily `brand`) and outlined in `assets/brand/*.svg` | SIL OFL 1.1 (`assets/fonts/PlusJakartaSans-LICENSE.txt`) |
| JetBrains Mono (`assets/fonts/JetBrainsMono-*.ttf`) | terminal and code text | SIL OFL 1.1 (`assets/fonts/JetBrainsMono-LICENSE.txt`) |
| Noto Sans Arabic, Devanagari, Thai and CJK SC, subset (`server/src/bench/fonts/`) | built into `axia-bench` for LordBench's shaping workload (subset by `tools/gen-lbfonts.py`) | SIL OFL 1.1 (`server/src/bench/fonts/OFL.txt`) |

### Local changes to bundled components

Each change is marked `LordOS patch` in the source and has to be applied again when the component is
updated. Nothing else in `third_party/` differs from upstream.

| Component (upstream version) | Change | Files | Why | Since |
|---|---|---|---|---|
| QuickJS 2025-04-26 (`third_party/quickjs/VERSION`) | Adds `size_t JS_GetMallocSize(JSRuntime *rt)`, which returns `rt->malloc_state.malloc_size`: the same `malloc_size` that `JS_ComputeMemoryUsage` reports, without walking every object, string and shape of the runtime. No existing function or structure changes. | `quickjs.c` (7 lines, just before `JS_ComputeMemoryUsage`), `quickjs.h` (its prototype, after `JS_ComputeMemoryUsage`'s) | `lord.stats()` (`f_stats` in `server/src/ui/js.c`) gives `jsKB` from it in O(1); About, Settings → General and System Information poll it while open, and the full walk grew with the number of objects. Checked equal to `JS_ComputeMemoryUsage`'s `malloc_size` on a runtime with 100,000 objects, a 50,000-entry Map and after GC | 0.8.6 |

The Rust Argon2 crate has one local API extension, marked `Axia patch` in
`rust/vendor/argon2-0.6.0/src/lib.rs`: `hash_password_into_with_memory_and_ad`
borrows arbitrary-length associated data rather than using the PHC string's
32-byte parameter field. It preserves existing KDBX inputs; the Argon2 algorithm
and encoding are unchanged. The vendored checksum manifest records the patched
file hash and upstream package identity. Reapply and rerun compatibility vectors
when updating that crate; see `docs/RUST-CRYPTO.md`.

## npm packages (bundled into the theme JavaScript at build time)

| Package | License |
|---|---|
| react, react-reconciler, scheduler | MIT |

## System libraries (Debian packages, dynamically linked)

These are not part of this repository; they are installed from Debian.

| Library | Linked by | License |
|---|---|---|
| libwayland-server / libwayland-client, libxkbcommon | axia-server | MIT |
| Mesa (libgbm, libEGL, libGLESv2) | axia-server (GPU path) | MIT |
| OpenSSL 3 | lord-sessiond, axia-maild | Apache-2.0 |
| libcrypt | lord-sessiond | LGPL-2.1+ |
| Poppler (glib), cairo, freetype, fontconfig | axia-doc | GPL-2.0-or-later / LGPL / FTL |
| libarchive | axia-doc | BSD-2-Clause |
| FFmpeg (libavcodec, libavformat, libavutil, libswscale, libswresample), libva | axia-media | LGPL-2.1+; Debian's build is GPL-2.0-or-later |

Note: `axia-doc` and `axia-media` are separate helper processes. Because they link Poppler and
Debian's FFmpeg build, those two binaries as distributed are covered by the GPL; the MIT-licensed
LordOS source they are built from is GPL-compatible. `axia-server` itself does not link them.
On the Raspberry Pi image, and on a Pi updated to 0.8.7, `axia-media` runs on Raspberry Pi's build of FFmpeg (`+rpt`, the
same package names), pinned from its archive for the Pi 5's hardware HEVC (`docs/WALLPAPERS.md`).

These come with `lordos` for Files (0.8.7): **rclone** (MIT), **fuse3** (LGPL-2.1, GPL-2.0) and openssh-client (BSD). The Store's Disk Formats item, not preinstalled, brings **ntfs-3g** (GPL-2.0), **exfatprogs** (GPL-2.0), dosfstools (GPL-3.0) and, on amd64, **apfs-dkms** (GPL-2.0).

## Tools used to make website media (not shipped in LordOS)

| Tool | Used for | License |
|---|---|---|
| Kokoro-82M (model and voices, run with kokoro-onnx) | the English narration of the LordOS Imager guide video on lordos.org/imager (voice `am_michael`) | Apache-2.0 |

## Rust dependencies for Arquivos

The source archive includes the exact crates in `rust/Cargo.lock`, under
`rust/vendor`, so Fedora/Copr builds run with Cargo's `--locked --offline`.
Every crate retains its upstream manifest, checksums and license files. The
installer also includes their license/notice files under
`share/licenses/axia-de/rust/`.

Dependencies include libc, serde/serde_json, regex and image, plus the versions
of their transitive dependencies listed in the lockfile. MIT is selected where
an MIT alternative is offered; moxcms and pxfm use BSD-3-Clause, zlib-rs uses
Zlib, and unicode-ident additionally requires Unicode-3.0. All upstream
attribution is retained. GIO/GLib are dynamically linked system libraries;
GVfs and its protocol backends are provided by Fedora.
