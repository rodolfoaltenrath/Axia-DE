/* vcrypto.h — own primitives that Monocypher lacks and that foreign formats require:
 *   - AES-128/256 (block, CBC): CTAP2 (PIN protocol 1), KDBX (AES-256-CBC and AES-KDF) and the
 *     "dh-ietf1024-sha256-aes128-cbc-pkcs7" encryption of Secret Service;
 *   - SHA-1 and HMAC-SHA1 (TOTP, RFC 6238/4226), HMAC-SHA256 (CTAP2, KDBX, TOTP-SHA256),
 *     HMAC-SHA512 (TOTP-SHA512) and HKDF-SHA256 (RFC 5869).
 * The native vault uses only Monocypher (XChaCha20-Poly1305, Argon2id, BLAKE2b).
 * SHA-256/512 are those of server/src/pkg/crypto.c. */
#ifndef LORD_VAULT_VCRYPTO_H
#define LORD_VAULT_VCRYPTO_H

#include <stddef.h>
#include <stdint.h>

/* ---- AES ---- */
typedef struct { uint32_t rk[60]; int nr; } Aes;          /* round keys (encrypt and decrypt) */
int  aes_init(Aes *a, const uint8_t *key, int keybits);   /* 128, 192 or 256; 0 on success */
void aes_encrypt(const Aes *a, const uint8_t in[16], uint8_t out[16]);
void aes_decrypt(const Aes *a, const uint8_t in[16], uint8_t out[16]);
void aes_wipe(Aes *a);
/* CBC without padding: len a multiple of 16 (in and out may be the same buffer). iv is updated. */
int  aes_cbc_encrypt(const Aes *a, uint8_t iv[16], const uint8_t *in, uint8_t *out, size_t len);
int  aes_cbc_decrypt(const Aes *a, uint8_t iv[16], const uint8_t *in, uint8_t *out, size_t len);
/* Strips PKCS#7 padding from an already decrypted block: unpadded length or -1 if invalid. */
long pkcs7_unpad(const uint8_t *buf, size_t len);

/* ---- SHA-1 ---- */
typedef struct { uint32_t h[5]; uint64_t len; uint8_t buf[64]; size_t n; } Sha1;
void sha1_init(Sha1 *s);
void sha1_update(Sha1 *s, const void *data, size_t len);
void sha1_final(Sha1 *s, uint8_t out[20]);
void sha1(const void *data, size_t len, uint8_t out[20]);

/* ---- HMAC ---- */
void hmac_sha1(const uint8_t *key, size_t klen, const uint8_t *msg, size_t len, uint8_t out[20]);
void hmac_sha256(const uint8_t *key, size_t klen, const uint8_t *msg, size_t len, uint8_t out[32]);
/* Multi-part HMAC-SHA256 (KDBX: index || length || block). parts/lens end with NULL. */
void hmac_sha256_v(const uint8_t *key, size_t klen, const uint8_t *const *parts, const size_t *lens, uint8_t out[32]);
void hmac_sha512(const uint8_t *key, size_t klen, const uint8_t *msg, size_t len, uint8_t out[64]);
/* HKDF-SHA256 (RFC 5869); out of up to 255*32 bytes. 0 on success. */
int  hkdf_sha256(const uint8_t *salt, size_t slen, const uint8_t *ikm, size_t ilen,
                 const uint8_t *info, size_t infolen, uint8_t *out, size_t outlen);

/* ---- misc ---- */
int  vc_random(void *buf, size_t n);                       /* getrandom; 0 on success */
int  vc_equal(const void *a, const void *b, size_t n);     /* 1 if equal (constant time) */

#endif
