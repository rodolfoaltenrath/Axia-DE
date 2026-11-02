/* p256.h — our own NIST P-256 curve, only what CTAP2 needs: the ECDH key agreement of
 * PIN protocol 1 and of the hmac-secret extension (COSE alg -25, ECDH-ES+HKDF-256).
 *
 * Montgomery arithmetic with 32-bit words (for p and n), projective coordinates with the
 * COMPLETE formulas of Renes–Costello–Batina (2016, alg. 4, a = -3: no special cases for
 * infinity or P = Q) and a Montgomery ladder with branch-free conditional swap.
 * There is no ECDSA: our client never verifies signatures (the key coming out of hmac-secret
 * checks itself when decrypting the vault). Scalars and coordinates are 32-byte big-endian. */
#ifndef LORD_VAULT_P256_H
#define LORD_VAULT_P256_H

#include <stdint.h>

/* Random key pair (getrandom). 0 = ok. */
int p256_keypair(uint8_t priv[32], uint8_t x[32], uint8_t y[32]);
/* Public key of priv (1 <= priv < n). 0 = ok, -1 if priv is invalid. */
int p256_public(const uint8_t priv[32], uint8_t x[32], uint8_t y[32]);
/* x of priv * (px, py). -1 if the point is not on the curve or the result is infinity. */
int p256_ecdh(const uint8_t priv[32], const uint8_t px[32], const uint8_t py[32], uint8_t shared_x[32]);
/* 1 if (x, y) is a point on the curve (with x, y < p). */
int p256_on_curve(const uint8_t x[32], const uint8_t y[32]);
/* For tests: k * G in affine form; -1 if it yields infinity (k = 0 or a multiple of n). k may be
 * any 256-bit number (k < n is not required). */
int p256_mul_base(const uint8_t k[32], uint8_t x[32], uint8_t y[32]);

#endif
