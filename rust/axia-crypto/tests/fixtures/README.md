# Public crypto compatibility fixtures

These files contain **only synthetic, publicly known test data**.

- `monocypher-4.0.2.json`: data extracted from the previously vendored release's
  `tests/vectors.h`: BLAKE2b, IETF ChaCha20, XChaCha20-Poly1305 and Argon2. Its
  upstream header identifies official vectors and cases generated with libsodium.
  Six additional Argon2d/i/id cases (64/257-byte associated data, secret input,
  two lanes and 35 KiB of memory) were recorded from that release's compiled
  implementation. The upstream license is retained alongside this data.
- `legacy.lvault`: generated with the previous native backend, containing a
  password slot, a keyfile slot, a JSON body and an encrypted attachment.
- `legacy.lbk`: previous-backend LBK1 encryption of 65537 deterministic bytes,
  crossing the 64 KiB chunk boundary.

The public password, test key, salt, JSON and plaintext pattern are defined in
`../formats.rs`. Never replace these files with real user vaults or backups.
Recording requires an explicit ignored test and an explicitly supplied legacy
shared library; ordinary tests only read the fixtures. New output is also tested
against the preserved old backend during migration, without retaining its C code
in the repository.
