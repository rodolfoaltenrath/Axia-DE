/* crypt.h — optional encryption of backups, with Monocypher (vendored in third_party/monocypher;
 * see docs/SYSTEM.md, "Encryption").
 *
 * Each file in the backup is:
 *   "LBK1" | base nonce (24 bytes) | 64 KiB chunks encrypted with XChaCha20-Poly1305 (+16 MAC)
 * Each chunk's nonce is the base nonce with the chunk number mixed into its last 8 bytes, and the
 * associated data is that number plus whether it is the last chunk: chunks cannot be reordered,
 * removed or truncated without failing verification. An empty file is a single empty final chunk.
 * The key comes from the passphrase with Argon2id (64 MiB, 3 passes) and a per-destination salt.
 * File names and symlink targets are NOT encrypted. */
#ifndef LORD_BACKUP_CRYPT_H
#define LORD_BACKUP_CRYPT_H

#include <stddef.h>
#include <stdint.h>

#define BKC_CHUNK 65536u
#define BKC_HDR 28u
#define BKC_MAC 16u
#define BKC_KEY 32u
#define BKC_SALT 16u
#define BKC_FILE "encryption.json"      /* in the machine folder: salt and key check */

uint64_t bkc_enc_size(uint64_t plain);
int  bkc_plain_size(uint64_t enc, uint64_t *plain);   /* -1 if no encrypted file can have that size */

/* tick(ud, bytes) between chunks: != 0 stops. Return 0 ok, -1 I/O, -2 out of space, 1 interrupted;
 * decrypt also returns -3 if a chunk fails verification (tampered file or wrong key). */
int  bkc_encrypt_fd(int in, int out, const uint8_t key[BKC_KEY], int (*tick)(void *ud, size_t n), void *ud);
int  bkc_decrypt_fd(int in, int out, const uint8_t key[BKC_KEY], int (*tick)(void *ud, size_t n), void *ud);

int  bkc_derive(const char *pass, const uint8_t salt[BKC_SALT], uint8_t key[BKC_KEY]);   /* slow: ~0.3 s */
void bkc_check(const uint8_t key[BKC_KEY], uint8_t out[16]);    /* key check value (does not reveal the key) */
int  bkc_random(uint8_t *buf, size_t n);

/* <machine>/encryption.json: write (new salt + key check) or read. 0 ok. */
int  bkc_info_write(const char *machine, const uint8_t salt[BKC_SALT], const uint8_t check[16]);
int  bkc_info_read(const char *machine, uint8_t salt[BKC_SALT], uint8_t check[16]);   /* -1: not encrypted */
/* Derives the key and checks it against encryption.json: 0 ok (key filled), -1 wrong passphrase, -2 no file. */
int  bkc_unlock(const char *machine, const char *pass, uint8_t key[BKC_KEY]);
/* Sets up encryption for a new destination: 0 ok. */
int  bkc_setup(const char *machine, const char *pass, uint8_t key[BKC_KEY]);
void bkc_wipe(void *p, size_t n);

#endif
