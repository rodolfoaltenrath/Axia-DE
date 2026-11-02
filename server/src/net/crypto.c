/* crypto.c — the bare minimum of cryptography the network needs: X25519 (WireGuard keys),
 * base64, SHA-1 and PBKDF2-HMAC-SHA1 (the WPA2 PSK from the password). Our own, no
 * OpenSSL. X25519 follows RFC 7748 with fields of 5 51-bit limbs (like curve25519-donna-c64). */
#include "netpriv.h"

#include <string.h>
#include <sys/random.h>

typedef unsigned __int128 u128;
typedef uint64_t fe[5];
#define MASK51 ((1ull << 51) - 1)

static uint64_t load64(const uint8_t *p) {
    uint64_t v = 0;
    for (int i = 7; i >= 0; i--) v = v << 8 | p[i];
    return v;
}

static void fe_frombytes(fe h, const uint8_t s[32]) {
    h[0] = load64(s) & MASK51;
    h[1] = (load64(s + 6) >> 3) & MASK51;
    h[2] = (load64(s + 12) >> 6) & MASK51;
    h[3] = (load64(s + 19) >> 1) & MASK51;
    h[4] = (load64(s + 24) >> 12) & MASK51;     /* bit 255 is ignored */
}

static void fe_carry(fe h) {
    for (int k = 0; k < 2; k++) {
        uint64_t c;
        c = h[0] >> 51; h[0] &= MASK51; h[1] += c;
        c = h[1] >> 51; h[1] &= MASK51; h[2] += c;
        c = h[2] >> 51; h[2] &= MASK51; h[3] += c;
        c = h[3] >> 51; h[3] &= MASK51; h[4] += c;
        c = h[4] >> 51; h[4] &= MASK51; h[0] += c * 19;
    }
}

static void fe_tobytes(uint8_t s[32], const fe in) {
    fe h;
    memcpy(h, in, sizeof h);
    fe_carry(h);
    /* final reduction: h < 2^255 + something; subtract p if h >= p */
    uint64_t q = (h[0] + 19) >> 51;
    q = (h[1] + q) >> 51;
    q = (h[2] + q) >> 51;
    q = (h[3] + q) >> 51;
    q = (h[4] + q) >> 51;
    h[0] += 19 * q;
    uint64_t c;
    c = h[0] >> 51; h[0] &= MASK51; h[1] += c;
    c = h[1] >> 51; h[1] &= MASK51; h[2] += c;
    c = h[2] >> 51; h[2] &= MASK51; h[3] += c;
    c = h[3] >> 51; h[3] &= MASK51; h[4] += c;
    h[4] &= MASK51;
    uint64_t w[4] = {
        h[0] | h[1] << 51,
        h[1] >> 13 | h[2] << 38,
        h[2] >> 26 | h[3] << 25,
        h[3] >> 39 | h[4] << 12,
    };
    for (int i = 0; i < 4; i++) for (int j = 0; j < 8; j++) s[i * 8 + j] = (uint8_t)(w[i] >> (8 * j));
}

static void fe_add(fe h, const fe f, const fe g) { for (int i = 0; i < 5; i++) h[i] = f[i] + g[i]; }

static void fe_sub(fe h, const fe f, const fe g) {
    /* + 2p so there are no negatives */
    h[0] = f[0] + 0xFFFFFFFFFFFDAull - g[0];
    h[1] = f[1] + 0xFFFFFFFFFFFFEull - g[1];
    h[2] = f[2] + 0xFFFFFFFFFFFFEull - g[2];
    h[3] = f[3] + 0xFFFFFFFFFFFFEull - g[3];
    h[4] = f[4] + 0xFFFFFFFFFFFFEull - g[4];
    fe_carry(h);
}

static void fe_mul(fe h, const fe f, const fe g) {
    uint64_t f0 = f[0], f1 = f[1], f2 = f[2], f3 = f[3], f4 = f[4];
    uint64_t g0 = g[0], g1 = g[1], g2 = g[2], g3 = g[3], g4 = g[4];
    uint64_t g1_19 = 19 * g1, g2_19 = 19 * g2, g3_19 = 19 * g3, g4_19 = 19 * g4;
    u128 r0 = (u128)f0 * g0 + (u128)f1 * g4_19 + (u128)f2 * g3_19 + (u128)f3 * g2_19 + (u128)f4 * g1_19;
    u128 r1 = (u128)f0 * g1 + (u128)f1 * g0 + (u128)f2 * g4_19 + (u128)f3 * g3_19 + (u128)f4 * g2_19;
    u128 r2 = (u128)f0 * g2 + (u128)f1 * g1 + (u128)f2 * g0 + (u128)f3 * g4_19 + (u128)f4 * g3_19;
    u128 r3 = (u128)f0 * g3 + (u128)f1 * g2 + (u128)f2 * g1 + (u128)f3 * g0 + (u128)f4 * g4_19;
    u128 r4 = (u128)f0 * g4 + (u128)f1 * g3 + (u128)f2 * g2 + (u128)f3 * g1 + (u128)f4 * g0;
    uint64_t c;
    r1 += (uint64_t)(r0 >> 51); uint64_t h0 = (uint64_t)r0 & MASK51;
    r2 += (uint64_t)(r1 >> 51); uint64_t h1 = (uint64_t)r1 & MASK51;
    r3 += (uint64_t)(r2 >> 51); uint64_t h2 = (uint64_t)r2 & MASK51;
    r4 += (uint64_t)(r3 >> 51); uint64_t h3 = (uint64_t)r3 & MASK51;
    c = (uint64_t)(r4 >> 51);   uint64_t h4 = (uint64_t)r4 & MASK51;
    h0 += c * 19;
    c = h0 >> 51; h0 &= MASK51; h1 += c;
    h[0] = h0; h[1] = h1; h[2] = h2; h[3] = h3; h[4] = h4;
}

static void fe_sq(fe h, const fe f) { fe_mul(h, f, f); }

static void fe_mul121666(fe h, const fe f) {
    u128 c = 0;
    for (int i = 0; i < 5; i++) {
        c += (u128)f[i] * 121666;
        h[i] = (uint64_t)c & MASK51;
        c >>= 51;
    }
    h[0] += (uint64_t)c * 19;
    fe_carry(h);
}

static void fe_inv(fe out, const fe z) {
    /* z^(p-2) with the RFC 7748 / donna chain */
    fe z2, z9, z11, z2_5_0, z2_10_0, z2_20_0, z2_50_0, z2_100_0, t;
    fe_sq(z2, z);
    fe_sq(t, z2); fe_sq(t, t);
    fe_mul(z9, t, z);
    fe_mul(z11, z9, z2);
    fe_sq(t, z11);
    fe_mul(z2_5_0, t, z9);
    fe_sq(t, z2_5_0); for (int i = 1; i < 5; i++) fe_sq(t, t);
    fe_mul(z2_10_0, t, z2_5_0);
    fe_sq(t, z2_10_0); for (int i = 1; i < 10; i++) fe_sq(t, t);
    fe_mul(z2_20_0, t, z2_10_0);
    fe_sq(t, z2_20_0); for (int i = 1; i < 20; i++) fe_sq(t, t);
    fe_mul(t, t, z2_20_0);
    fe_sq(t, t); for (int i = 1; i < 10; i++) fe_sq(t, t);
    fe_mul(z2_50_0, t, z2_10_0);
    fe_sq(t, z2_50_0); for (int i = 1; i < 50; i++) fe_sq(t, t);
    fe_mul(z2_100_0, t, z2_50_0);
    fe_sq(t, z2_100_0); for (int i = 1; i < 100; i++) fe_sq(t, t);
    fe_mul(t, t, z2_100_0);
    fe_sq(t, t); for (int i = 1; i < 50; i++) fe_sq(t, t);
    fe_mul(t, t, z2_50_0);
    fe_sq(t, t); for (int i = 1; i < 5; i++) fe_sq(t, t);
    fe_mul(out, t, z11);
}

static void fe_cswap(fe a, fe b, uint64_t swap) {
    uint64_t m = 0 - swap;
    for (int i = 0; i < 5; i++) { uint64_t x = m & (a[i] ^ b[i]); a[i] ^= x; b[i] ^= x; }
}

void x25519_clamp(uint8_t k[32]) {
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
}

void x25519(uint8_t out[32], const uint8_t scalar[32], const uint8_t point[32]) {
    uint8_t e[32];
    memcpy(e, scalar, 32);
    x25519_clamp(e);
    fe x1, x2 = {1}, z2 = {0}, x3, z3 = {1}, a, aa, b, bb, ee, c, d, da, cb, t;
    fe_frombytes(x1, point);
    memcpy(x3, x1, sizeof x3);
    uint64_t swap = 0;
    for (int pos = 254; pos >= 0; pos--) {
        uint64_t bit = (e[pos / 8] >> (pos & 7)) & 1;
        swap ^= bit;
        fe_cswap(x2, x3, swap);
        fe_cswap(z2, z3, swap);
        swap = bit;
        fe_add(a, x2, z2);
        fe_sq(aa, a);
        fe_sub(b, x2, z2);
        fe_sq(bb, b);
        fe_sub(ee, aa, bb);
        fe_add(c, x3, z3);
        fe_sub(d, x3, z3);
        fe_mul(da, d, a);
        fe_mul(cb, c, b);
        fe_add(t, da, cb); fe_sq(x3, t);
        fe_sub(t, da, cb); fe_sq(t, t); fe_mul(z3, x1, t);
        fe_mul(x2, aa, bb);
        fe_mul121666(t, ee);                 /* z2 = E·(BB + 121666·E) = E·(AA + 121665·E) */
        fe_add(t, bb, t);
        fe_mul(z2, ee, t);
    }
    fe_cswap(x2, x3, swap);
    fe_cswap(z2, z3, swap);
    fe_inv(z2, z2);
    fe_mul(x2, x2, z2);
    fe_tobytes(out, x2);
    memset(e, 0, sizeof e);
}

void x25519_public(uint8_t pub[32], const uint8_t priv[32]) {
    static const uint8_t base[32] = {9};
    x25519(pub, priv, base);
}

/* ---- base64 ---- */

static const char B64[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

int b64_encode(const uint8_t *in, size_t n, char *out, size_t cap) {
    size_t need = (n + 2) / 3 * 4 + 1, o = 0;
    if (cap < need) return -1;
    for (size_t i = 0; i < n; i += 3) {
        uint32_t v = (uint32_t)in[i] << 16 | (i + 1 < n ? (uint32_t)in[i + 1] << 8 : 0) | (i + 2 < n ? in[i + 2] : 0);
        out[o++] = B64[v >> 18 & 63];
        out[o++] = B64[v >> 12 & 63];
        out[o++] = i + 1 < n ? B64[v >> 6 & 63] : '=';
        out[o++] = i + 2 < n ? B64[v & 63] : '=';
    }
    out[o] = 0;
    return (int)o;
}

int b64_decode(const char *in, uint8_t *out, size_t cap) {
    uint32_t v = 0;
    int bits = 0;
    size_t o = 0;
    for (const char *p = in; *p && *p != '='; p++) {
        const char *k = strchr(B64, *p);
        if (!k || !*p) return -1;
        v = v << 6 | (uint32_t)(k - B64);
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            if (o >= cap) return -1;
            out[o++] = (uint8_t)(v >> bits);
        }
    }
    return (int)o;
}

/* ---- SHA-1, HMAC, PBKDF2 ---- */

typedef struct { uint32_t h[5]; uint64_t len; uint8_t buf[64]; size_t n; } Sha1;

static uint32_t rol(uint32_t x, int s) { return x << s | x >> (32 - s); }

static void sha1_block(Sha1 *s, const uint8_t *p) {
    uint32_t w[80];
    for (int i = 0; i < 16; i++) w[i] = (uint32_t)p[4 * i] << 24 | (uint32_t)p[4 * i + 1] << 16 | (uint32_t)p[4 * i + 2] << 8 | p[4 * i + 3];
    for (int i = 16; i < 80; i++) w[i] = rol(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
    uint32_t a = s->h[0], b = s->h[1], c = s->h[2], d = s->h[3], e = s->h[4];
    for (int i = 0; i < 80; i++) {
        uint32_t f, k;
        if (i < 20) { f = (b & c) | (~b & d); k = 0x5A827999; }
        else if (i < 40) { f = b ^ c ^ d; k = 0x6ED9EBA1; }
        else if (i < 60) { f = (b & c) | (b & d) | (c & d); k = 0x8F1BBCDC; }
        else { f = b ^ c ^ d; k = 0xCA62C1D6; }
        uint32_t t = rol(a, 5) + f + e + k + w[i];
        e = d; d = c; c = rol(b, 30); b = a; a = t;
    }
    s->h[0] += a; s->h[1] += b; s->h[2] += c; s->h[3] += d; s->h[4] += e;
}

static void sha1_init(Sha1 *s) { *s = (Sha1){ .h = { 0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0 } }; }

static void sha1_update(Sha1 *s, const uint8_t *p, size_t n) {
    s->len += n;
    while (n) {
        size_t k = 64 - s->n < n ? 64 - s->n : n;
        memcpy(s->buf + s->n, p, k);
        s->n += k; p += k; n -= k;
        if (s->n == 64) { sha1_block(s, s->buf); s->n = 0; }
    }
}

static void sha1_final(Sha1 *s, uint8_t out[20]) {
    uint64_t bits = s->len * 8;
    uint8_t pad = 0x80, zero = 0;
    sha1_update(s, &pad, 1);
    while (s->n != 56) sha1_update(s, &zero, 1);
    uint8_t l[8];
    for (int i = 0; i < 8; i++) l[i] = (uint8_t)(bits >> (56 - 8 * i));
    sha1_update(s, l, 8);
    for (int i = 0; i < 5; i++) { out[4 * i] = (uint8_t)(s->h[i] >> 24); out[4 * i + 1] = (uint8_t)(s->h[i] >> 16); out[4 * i + 2] = (uint8_t)(s->h[i] >> 8); out[4 * i + 3] = (uint8_t)s->h[i]; }
}

void sha1(const uint8_t *data, size_t n, uint8_t out[20]) {
    Sha1 s;
    sha1_init(&s);
    sha1_update(&s, data, n);
    sha1_final(&s, out);
}

static void hmac_sha1(const uint8_t *key, size_t klen, const uint8_t *msg, size_t mlen, uint8_t out[20]) {
    uint8_t k[64] = {0}, ipad[64], opad[64], inner[20];
    if (klen > 64) sha1(key, klen, k); else memcpy(k, key, klen);
    for (int i = 0; i < 64; i++) { ipad[i] = k[i] ^ 0x36; opad[i] = k[i] ^ 0x5c; }
    Sha1 s;
    sha1_init(&s); sha1_update(&s, ipad, 64); sha1_update(&s, msg, mlen); sha1_final(&s, inner);
    sha1_init(&s); sha1_update(&s, opad, 64); sha1_update(&s, inner, 20); sha1_final(&s, out);
}

void pbkdf2_sha1(const uint8_t *pass, size_t plen, const uint8_t *salt, size_t slen, unsigned iter, uint8_t *out, size_t olen) {
    uint8_t msg[64 + 4], u[20], t[20];
    if (slen > 64) return;
    for (uint32_t block = 1; olen; block++) {
        memcpy(msg, salt, slen);
        msg[slen] = (uint8_t)(block >> 24); msg[slen + 1] = (uint8_t)(block >> 16);
        msg[slen + 2] = (uint8_t)(block >> 8); msg[slen + 3] = (uint8_t)block;
        hmac_sha1(pass, plen, msg, slen + 4, u);
        memcpy(t, u, 20);
        for (unsigned i = 1; i < iter; i++) {
            hmac_sha1(pass, plen, u, 20, u);
            for (int j = 0; j < 20; j++) t[j] ^= u[j];
        }
        size_t k = olen < 20 ? olen : 20;
        memcpy(out, t, k);
        out += k; olen -= k;
    }
}

int random_bytes(void *out, size_t n) {
    size_t o = 0;
    while (o < n) {
        ssize_t r = getrandom((char *)out + o, n - o, 0);
        if (r <= 0) return -1;
        o += (size_t)r;
    }
    return 0;
}
