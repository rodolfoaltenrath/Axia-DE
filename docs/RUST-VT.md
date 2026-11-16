# Rust terminal emulator

`rust/axia-codecs/src/vt/` replaces `server/src/term/vt.c` (2,300 lines of C). The structs of
`server/src/term/term.h` (`Term`, `Cell`, `Line`, `Buffer`, `TermProfile`, `TermImage`, ...) are the ABI:
`term.c`, `term_paint.c` and `ui/js_term.c` read and write them directly, so the Rust declares them
`#[repr(C)]` field for field. The C file is gone; there is no C fallback.

| Piece | File |
|---|---|
| Types, the byte parser, CSI, SGR, modes, characters and graphemes, erase/insert/delete, scrolling, life cycle | `mod.rs` |
| OSC (titles, palette, links, shell integration 133, 1337), DCS, inline images, Sixel | `osc.rs` |
| Text extraction, selection, search, marks, links, words, copy mode, serialization | `text.rs` |
| Resize with reflow | `resize.rs` |

- Lines, rings and every string handed to the caller come from the C allocator, because the callers
  `free()` them (`vt_selection_text`, `vt_serialize`, `vt_url_at`, ...).
- The emulator needs `term_set_clipboard()` (OSC 52, OSC 1337 Copy), which only the compositor defines.
  Instead of a link-time dependency, `term_init()` registers it with `vt_set_clipboard_hook()`.
- Replies to the application (`CSI 6n`, colour queries, ...) are formatted in Rust with the same text.
- The module is behind `cfg(not(feature = "secure-memory"))`.

## A defect fixed in the migration

When a wide character did not fit at the end of a row, `reflow_main` counted that row as full
(`total += ocols`) but copied one cell less, and then read the missing cell **without initialising it**:
whatever an earlier reflow had left in the `malloc` buffer. If that was not blank, a phantom character
appeared after the text (found by the differential test as a stray `]` with underline attributes). The
cell is now blank, which is what the C code produced whenever the buffer was clean.

## Validation

Done against the original C, compiled with every symbol renamed (`c_` prefix) and linked into the same
program (the C reference carries the same one-line fix above, so the rest can be compared exactly). Both get
the same hostile input and, after **every** step, the whole `Term` state must match: cursor, modes, tabs, saved
cursors, parser state, parameters and string buffer, titles and other strings, palette, links, images (with a hash
of the pixels), selection, search matches, dirty flags, the preview generation counter, and every line of both
rings (length, allocation, flags and a hash of all cells) together with every reply, clipboard write and tmux
callback and every return value or returned string.

- inputs: random streams mixing text (ASCII, wide, combining, emoji, ZWJ, flags, skin tones, bad UTF-8),
  control characters, CSI with up to 40 parameters, colons, private markers and intermediates, SGR in every form,
  DEC modes, OSC (0-2, 4, 7, 8, 9, 10-12, 52, 104, 110-112, 133, 1337: marks, badge, images, copy, cell size),
  DCS (Sixel with palettes, HLS and repeats, DECRQSS, tmux control mode), plain ESC sequences, and
  chunks cut at random points;
- operations between writes: resize (including 1 row, 1,200 columns and 600 rows), reset, every selection mode
  (character, word, line, URL, rectangular), selection text, text ranges, search both ways, regular-expression match
  injection, marks, scrolling, scrollback clearing, URL under the cursor, word completion, highlight, logical text,
  serialization, copy mode keys, palette, grapheme tables;
- about 1.3 million comparisons in release, 175,000 more in a debug build (arithmetic overflow checked), no
  differences, and the original `vt-dump` suite passes (`tests/run_suites.sh`);
- the harness was tested by planting ten defects in the Rust (an erase mode, one SGR code, a reflow condition, an
  HLS offset, the search step, the 50-line limit of a command, the skin-tone range, a serialization escape, the copy
  mode half page, the first tab stop). Two got through at first (they needed a tall terminal and a modifier the
  generator never produced); the generator was extended and both are caught now.
