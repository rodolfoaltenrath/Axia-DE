# Rust image, font and compression codecs

`rust/axia-codecs` replaces the three stb headers and their C implementation
units. There is no fallback to the old decoder or rasterizer. It builds offline
from the vendored dependencies and workspace lockfile.

Image parsing, PNG/JPEG encoding, glyph rasterization, font metrics, pair
kerning, raw DEFLATE and gzip decoding are Rust. Mail/contact thumbnail scaling,
premultiplication and JPEG generation are also Rust. Existing C consumers use
`server/src/codecs_ffi.h`; returned buffers retain their existing ownership
(`free`, or `sm_free` for the vault). Text shaping, the text cache, Cairo document
composition and the compositor itself remain separate C consumers.

The image decoder checks dimensions and pixel budgets before reading pixel
buffers. The compositor retains its 64 Mi-pixel, 32768-per-side limit; mail allows
40 million pixels and 16384 per side. The terminal and vault limit sides to 8192. Document conversion allows
120 million pixels, capped by the decoder's 512 MiB allocation budget. PNG,
JPEG, GIF (first frame) and BMP are accepted; the vault accepts PNG/JPEG only.
Gzip validates CRC32 and ISIZE. Both compression paths reject truncation,
trailing bytes and output exceeding the caller's limit while decoding, rather
than checking only after an unbounded allocation.

The font backend uses fontdue and ttf-parser, including GPOS class-pair kerning
used by Inter. Bitmap offsets follow the original top-down baseline convention.
Glyphs and advances are cached by the existing text layer. Different coverage
at glyph edges and small JPEG rounding differences are expected when changing
codec implementations; layout and premultiplied ARGB conventions stay intact.

The vault links a separate `--no-default-features --features secure-memory`
static library, in its own Cargo target directory. A Rust global allocator
routes decoder scratch buffers, vectors and final results through the existing
smem arena; exporting only the final result to smem would leave intermediate
secrets on the ordinary heap. Alignment metadata also lives inside the arena.
The compositor and other helpers must never link this secure variant. A compile-time
guard rejects enabling both `secure-memory` and the default `fonts` feature.

New codec tests are Rust, including tests linked to the real smem allocator:

```sh
make -C server test-codecs test-codecs-secure
```

The existing consumer regression suites additionally exercise package signing,
journal decompression, QR images, KeePass imports and engine rendering. The
Rust image ABI regression test no longer compiles a C decoder:

```sh
make -C server build/axia-server build/axia-doc build/axia-maild build/axia-vault
make -C server build/engine-test build/qr-test build/import-test build/journal-test
make -C server test-rust
```

The vendored ttf-parser package omits its optional `testing-tools/` Qt/C++ font
viewer. The checksum manifest retains the original package checksum and hashes
of every retained file. No C/C++ backend or new C test is introduced by these
codec dependencies.

## Validation (x86_64, 2026-11-10)

The Fedora 44 build container compiled the compositor, document, mail and vault
helpers. Their symbol tables contain no `stbi_` or `stbtt_` symbols.

- Rust codecs: 14 tests passed; protected-memory variant: 11 passed, linked to
  the real smem allocator.
- Shell Rust suite: 436 passed, 11 ignored; the ignored native image ABI test was
  run explicitly and passed (decoding, pixels, cache, malformed files and FIFO).
- Existing QR suite: 36/36; KeePass/import suite: 150/150; signing suite: 55/55.
  Journal decompression also passed its corruption, truncation and random cases.
- Engine: 9549 checks, zero failures. The old font backend was separately rebuilt
  against the same scene engine and passed all 9549 checks against the previous
  references. Only then were x86_64 frame references recorded for fontdue and
  checked by a fresh run, including optimized composition and damage fuzzing.
  Glyph samples with Portuguese accents were inspected at 14, 20, 32 and 48 px.

Builds and regression runs used an isolated source snapshot to avoid sharing
build artifacts with concurrent development. This validates the code and
headless render paths; it does not replace an interactive session check.
