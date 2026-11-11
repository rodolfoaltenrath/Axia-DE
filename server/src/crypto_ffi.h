/* RustCrypto primitives (rust/axia-crypto), exported through the codecs archive.
 * No heap allocation. Work/output buffers belong to C. Keys/nonces/AD must be
 * disjoint from output/work; message input/output may be equal or disjoint.
 * Functions return 0 on success, -1 on invalid parameters/authentication failure.
 * Failed authentication clears the output. These APIs do not change file formats. */
#ifndef AXIA_CRYPTO_FFI_H
#define AXIA_CRYPTO_FFI_H
#include <stddef.h>
#include <stdint.h>
void axia_crypto_wipe(void *, size_t);
int axia_crypto_verify16(const uint8_t *, const uint8_t *);
/* BLAKE2b: hash size is a parameter, not truncation. Two message spans are joined
 * logically, without allocating a concatenation. Empty key means unkeyed. */
int axia_crypto_blake2b(uint8_t *out, size_t outlen, const uint8_t *key, size_t keylen,
                       const uint8_t *a, size_t alen, const uint8_t *b, size_t blen);
int axia_crypto_seal(uint8_t *out, uint8_t tag[16], const uint8_t key[32], const uint8_t nonce[24],
                    const uint8_t *ad, size_t adlen, const uint8_t *input, size_t len);
int axia_crypto_open(uint8_t *out, const uint8_t tag[16], const uint8_t key[32], const uint8_t nonce[24],
                    const uint8_t *ad, size_t adlen, const uint8_t *input, size_t len);
/* NULL input generates keystream. Counter is measured in 64-byte blocks. */
int axia_crypto_chacha20(uint8_t *out, const uint8_t *input, size_t len,
                        const uint8_t key[32], const uint8_t nonce[12], uint32_t counter);
enum { AXIA_ARGON2_D = 0, AXIA_ARGON2_I = 1, AXIA_ARGON2_ID = 2 };
typedef struct { uint32_t algorithm, memory_kib, passes, lanes; } AxiaArgonConfig;
typedef struct { const uint8_t *password, *salt; uint32_t password_len, salt_len; } AxiaArgonInputs;
typedef struct { const uint8_t *secret, *ad; uint32_t secret_len, ad_len; } AxiaArgonExtras;
/* Includes alignment slack: malloc/smem need not be 64-byte aligned. Argon2 v1.3;
 * output 4..64 bytes. Active work blocks are erased before returning. */
size_t axia_crypto_argon_work_size(uint32_t memory_kib);
int axia_crypto_argon2(uint8_t *out, size_t outlen, void *work, size_t worklen,
                      AxiaArgonConfig, AxiaArgonInputs, AxiaArgonExtras);
#endif
