# Rust mail parsers

`rust/axia-mail` replaces the C parsers of `axia-maild` that read data from
outside: MIME and character sets (`mime.c`, `charset_tables.h`), the WebDAV XML
reader (`xml.c`), the HTML sanitizer (`html.c`, `html_entities.h`), iCalendar
(`ical.c`) and vCard (`vcard.c`). Those files are gone; there is no C fallback.

## Boundary

The headers in `server/src/mail/` (`mime.h`, `xml.h`, `html.h`, `ical.h`,
`vcard.h`) are unchanged and remain the contract. Every function keeps its name,
arguments and ownership, and every struct (`MimeMsg`, `MxNode`, `IcComp`,
`IcEvent`, `IcBuild`, `HtmlData`, …) is `#[repr(C)]` with the same fields, so
the C callers (`maild.c`, `imap.c`, `smtp.c`, `msgview.c`, `pim.c`, `dav.c`,
`store.c`) did not change.

- Returned text comes from `malloc` and C releases it with `free()`, as before.
  `MxNode` and `IcComp` trees are released only by `mx_free` and `ic_free`;
  `IcEvent` arrays by `ical_events_free`.
- `JBuf` outputs (`ic_line`, `ical_event_json`, `vcard_json`) are written with
  the growth rules and the 16 MiB limit of `doc/wire.c`, so C keeps using
  `jb_*` on the same buffer.
- Time zones still go through the C TZif reader (`server/src/ui/tzif.c`):
  Rust calls `tz_zone_valid` and `tzif_lookup`. The Clock uses the same reader.
- `vcard_build` reads the C `JVal` tree of `sec/json.c` directly.
- The IMAP response parser (`imapp.c`) and the network code stay C.

`axia_mail_layout()` reports the size and key offsets of every exported struct.
`tests/mail_test.c` compares them with the headers' `sizeof`/`offsetof`, so a
header change that is not mirrored in Rust fails the build's test.

## Build

The crate is a dependency of `axia-codecs` behind its `mail` feature. Only
`axia-maild` and `build/mail-test` link that variant
(`build/mail-codecs/release/libaxia_codecs.a`, `$(MAIL_CODEC_LIB)`). The
compositor and the vault keep their archives without these symbols: both define
their own `b64_encode`/`b64_decode`. Enabling `mail` together with
`secure-memory` is a compile error.

```sh
make -C server test-mail-rust build/mail-test
make -C server mail-asan          # the C side under ASan/UBSan
make -C server build/axia-maild   # needs the OpenSSL headers
```

## Behaviour

The port is faithful to the C code, including its limits (input sizes, depth,
part/node/property counts), truncation sizes of fixed fields, C-string
semantics at embedded NULs and the exact JSON and iCalendar/vCard text it
produced. Numbers in CSS and HTML attributes are read with libc `strtod`, as
before. No input panics: indexing is checked, and the one place where the C code
read outside a table (a month before year 0 in `RRULE` expansion) yields no
occurrence.

Two bugs were fixed on the way:

- `ic_unescape` removed the last continuation bytes of its output whenever the
  buffer was exactly full. `ic_unescape_dup` always sizes it exactly, so any
  vCard name, event title or note ending in a multibyte character (`José`)
  ended in a lone lead byte and showed `Jos�`. It now drops a partial character
  only when the text was truncated.
- The HTML sanitizer did not reset the nesting counter of a slot reused for a
  hidden element (`display:none`, `<svg>`, …). After a closed `<ol>` at the same
  depth, the hidden element's own close tag was taken as nested, and the visible
  text after it was dropped.

`tests/mail_test.c` covers both.

## Validation (x86_64, 2026-11-11)

- `build/mail-test`: 30,976/30,976 checks against the Rust parsers (the 30,953
  checks of the C version plus layout and the two regressions).
- `cargo test -p axia-mail`: 10 passed, including a hostile-input loop.
- Differential run before removing the C files: the old C parsers, compiled
  with their symbols renamed, against the Rust ones on every API, over the test
  fixtures and 200,000 mutated inputs (1,000,110 comparisons). The outputs were
  byte-identical, apart from the two fixes above (applied to both sides) and
  `DTSTAMP`/`REV` stamps, which carry the current time.
- The `axia-maild` sources compile against the unchanged headers and every parser
  symbol they use resolves to the Rust archive, with no duplicate symbols.
