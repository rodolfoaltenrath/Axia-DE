/* kdbx.h — own reader for KeePass 2.x / KeePassXC databases (.kdbx), for importing (see
 * vimport.h and docs/PASSWORDS.md, "Import and export").
 *
 * Versions: KDBX 4.0/4.1 (header with SHA-256 and HMAC, HMAC-SHA256 blocks, inner header
 * with the attachments) and KDBX 3.1 (SHA-256 blocks, attachments in Meta/Binaries). KDF: Argon2d,
 * Argon2id (version 0x13, Monocypher) and AES-KDF; ciphers: AES-256-CBC and ChaCha20 (not Twofish:
 * "unsupported-cipher"); inner stream for protected values: ChaCha20 (4.x) and Salsa20 (3.1).
 * Composite key: password and/or key file (XML 1.0/2.0, 32 bytes, 64 hex or SHA-256 of the
 * file). Errors go in ImpList.err, as in imp_parse. */
#ifndef LORD_VAULT_KDBX_H
#define LORD_VAULT_KDBX_H

#include <stddef.h>
#include <stdint.h>
#include "vimport.h"

int kdbx_is(const uint8_t *d, size_t len);          /* 1 if it starts with the KDBX 2.x+ signature */
/* password NULL or "" with a key file = no password in the composite key. */
int kdbx_parse(const uint8_t *d, size_t len, const char *password, const uint8_t *keyfile, size_t kflen, ImpList *l);
/* 32-byte key from a KeePass key file. 0 ok, -1 if it is an invalid key XML
 * (with a mismatching Hash). */
int kdbx_keyfile_key(const uint8_t *kf, size_t n, uint8_t out[32]);

/* Limits against files requesting an excessive KDF (rejected with "too-big").
 * Default: 500 million AES-KDF rounds and 2 GiB of memory for Argon2. */
extern uint64_t kdbx_max_aes_rounds;
extern uint64_t kdbx_max_argon_kib;

/* AES-KDF (transforming the composite key): AES-NI if the CPU has it, otherwise T-tables.
 * kdbx_aes_kdf_mode = 1 forces the tables; kdbx_aes_kdf_ref uses the bytewise AES from vcrypto.c
 * (to compare in tests). */
extern int kdbx_aes_kdf_mode;
int kdbx_aes_kdf(const uint8_t comp[32], const uint8_t seed[32], uint64_t rounds, uint8_t out[32]);
int kdbx_aes_kdf_ref(const uint8_t comp[32], const uint8_t seed[32], uint64_t rounds, uint8_t out[32]);

#endif
