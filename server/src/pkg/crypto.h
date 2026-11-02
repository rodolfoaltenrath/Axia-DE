/* crypto.h — just enough to verify Debian's signature: SHA-256, SHA-512, RSA (PKCS#1 v1.5)
 * and Ed25519. Our own code. Signing with Ed25519 is only used by .lpk packages (the user's own
 * key for "Share my version", see docs/SIGNING.md). */
#ifndef LORD_PKG_CRYPTO_H
#define LORD_PKG_CRYPTO_H

#include <stddef.h>
#include <stdint.h>

typedef struct { uint32_t h[8]; uint64_t len; uint8_t buf[64]; size_t n; } Sha256;
typedef struct { uint64_t h[8]; uint64_t len; uint8_t buf[128]; size_t n; } Sha512;

void sha256_init(Sha256 *s);
void sha256_update(Sha256 *s, const void *data, size_t len);
void sha256_final(Sha256 *s, uint8_t out[32]);
void sha256(const void *data, size_t len, uint8_t out[32]);
void sha256_select(int hw);           /* tests: 0 = C code only, 1 = best for this CPU */
int  sha256_accelerated(void);          /* 1: the CPU's SHA-256 instructions are used (x86 SHA, ARMv8 crypto) */
void sha512_init(Sha512 *s);
void sha512_update(Sha512 *s, const void *data, size_t len);
void sha512_final(Sha512 *s, uint8_t out[64]);

void hex_encode(const uint8_t *in, size_t n, char *out);          /* out: 2n+1 */
int  hex_decode(const char *in, uint8_t *out, size_t n);          /* 0 ok */

enum { HASH_SHA256 = 8, HASH_SHA512 = 10 };                       /* OpenPGP numbers */

/* 1 if sig is a valid PKCS#1 v1.5 signature of digest (SHA-256 or SHA-512) with the key (n, e). */
int rsa_verify(const uint8_t *n, size_t nlen, const uint8_t *e, size_t elen,
               const uint8_t *sig, size_t siglen, int hash, const uint8_t *digest);
/* 1 if sig (R||S) signs msg with the public key pub. */
int ed25519_verify(const uint8_t pub[32], const uint8_t sig[64], const uint8_t *msg, size_t len);

/* Ed25519 with the 32-byte seed (RFC 8032): public key and signature of msg. */
void ed25519_public(const uint8_t seed[32], uint8_t pub[32]);
void ed25519_sign(const uint8_t seed[32], const uint8_t *msg, size_t len, uint8_t sig[64]);

#endif
