# Rust cryptography replacing Monocypher

`rust/axia-crypto` replaces every Monocypher call. The C implementation, headers,
build objects and C upstream test program are removed. Runtime primitives are
provided by vendored RustCrypto crates: Argon2d/i/id v1.3, BLAKE2b, IETF ChaCha20
and XChaCha20-Poly1305. `zeroize` and `subtle` provide erasure and constant-time
comparison support. Licensing and the local dependency patch are recorded in
`THIRD_PARTY.md`.

The scope is the cryptography previously provided by Monocypher. Vault file
parsing, backup chunk framing, AES/SHA foreign-format support and the native
consumer programs remain separate components. New crypto tests are Rust.

## Compatibility and memory

Native `.lvault` and LBK1 backup magic, versions, KDF parameters, domain strings,
nonces, tags and associated-data encoding are unchanged. Existing files require
no conversion or re-encryption. BLAKE2b's output length remains a hash parameter;
BLAKE2b-128/256 are not implemented by truncating a 512-bit digest.

Primitives allocate no heap memory. Argon2 accepts caller-owned work storage:
smem for the vault, explicitly erased storage for backups. The required size
includes 63 bytes of alignment slack because RustCrypto blocks require 64-byte
alignment and C malloc/smem do not guarantee it. Active blocks are erased before
returning; consumers retain ownership and erase/free their allocations as before.
Temporary key and hash state uses zeroization-enabled dependencies.

The Argon2 crate's PHC parameter representation limits associated data to 32
bytes. Existing KDBX input permits longer data. An `Axia patch` adds a borrowed-AD
entry point to Argon2 0.6.0 without changing its algorithm or byte encoding.
Regression vectors include 64- and 257-byte associated data with secret inputs,
two lanes and a memory cost not divisible by the lane segment size. The patch
must be retained or replaced by an equivalent upstream API when updating.

`server/src/crypto_ffi.h` defines the narrow ABI. Message input/output may be
identical for in-place use or disjoint; partial overlap is rejected. Failed
AEAD authentication returns an error and clears the output. Callers propagate
failures before writing encrypted files or consuming plaintext.

Crypto is included as an rlib dependency of the codecs archive. Each executable
links one Rust archive/runtime variant: the compositor through axia-shell, the
vault through the secure-memory codecs variant. This avoids mixing the vault's
allocator with a second standalone Rust archive.

## Tests and recorded evidence

```sh
make -C server test-crypto test-crypto-native
make -C server build/vault-test build/import-test build/backup-test build/softfido
```

The Rust suite retains data-only vectors from Monocypher 4.0.2 (official and
libsodium-generated upstream cases): 681 BLAKE2b, 128 ChaCha20, 320 AEAD and 28
Argon2 cases, plus six long-AD cases recorded from the old implementation.
It checks split hashing, in-place processing, bad authentication, invalid scratch
sizes, scratch erasure, overlap rejection and zero heap allocations.

Immutable fixtures generated with the previous backend exercise password and
keyfile slots, JSON body, attachment and a backup crossing the 64 KiB boundary.
The public test credentials and generation procedure live alongside the fixtures.
The native Rust test calls the actual format consumers, with no copied parser.
Recording is explicitly ignored and never part of ordinary test runs.

Validation on x86_64 in the isolated Fedora 44 build container, 2026-11-10:

- All 1163 primitive compatibility vectors passed; the allocation/erasure and
  invalid-input tests passed.
- The new backend opened the old native vault, both slots, its attachment and
  its encrypted backup. A separately preserved old backend also opened fresh
  vaults and backups written by the new backend.
- Existing vault suite: 74/74; KeePass/import suite: 150/150; backup suite:
  220 checks, zero failures. Codec regressions passed in normal and protected
  memory configurations (14 and 11 tests respectively).
- Compositor, vault and software FIDO2 helper compiled. Symbol tables of the
  compositor and vault contain no Monocypher primitive symbols.

Only synthetic test files were used. Existing user vaults and backups were not
opened or rewritten during migration. Interactive session behavior is outside
these crypto regression tests.
