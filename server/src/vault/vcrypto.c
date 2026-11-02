/* vcrypto.c — own AES, SHA-1, HMAC and HKDF (see vcrypto.h). Small, direct code: byte-wise
 * AES (FIPS-197), SHA-1 (FIPS 180-4), HMAC (RFC 2104), HKDF (RFC 5869). */
#include "vcrypto.h"
#include "../pkg/crypto.h"

#include <errno.h>
#include <string.h>
#include <sys/random.h>

/* ================= AES ================= */

static const uint8_t SB[256] = {
    0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
    0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
    0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
    0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
    0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
    0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
    0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
    0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
    0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
    0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
    0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
    0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
    0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
    0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
    0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
    0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16,
};
static uint8_t ISB[256];
static int isb_ready;

static void make_isb(void) {
    if (isb_ready) return;
    for (int i = 0; i < 256; i++) ISB[SB[i]] = (uint8_t)i;
    isb_ready = 1;
}

static uint8_t xt(uint8_t x) { return (uint8_t)((x << 1) ^ ((x >> 7) * 0x1b)); }
static uint8_t gmul(uint8_t a, uint8_t b) {
    uint8_t r = 0;
    while (b) { if (b & 1) r ^= a; a = xt(a); b >>= 1; }
    return r;
}

int aes_init(Aes *a, const uint8_t *key, int keybits) {
    int nk = keybits / 32;
    if (keybits != 128 && keybits != 192 && keybits != 256) return -1;
    make_isb();
    a->nr = nk + 6;
    int total = 4 * (a->nr + 1);
    for (int i = 0; i < nk; i++)
        a->rk[i] = (uint32_t)key[4 * i] << 24 | (uint32_t)key[4 * i + 1] << 16 | (uint32_t)key[4 * i + 2] << 8 | key[4 * i + 3];
    uint8_t rcon = 1;
    for (int i = nk; i < total; i++) {
        uint32_t t = a->rk[i - 1];
        if (i % nk == 0) {
            t = (t << 8) | (t >> 24);
            t = (uint32_t)SB[t >> 24] << 24 | (uint32_t)SB[(t >> 16) & 255] << 16 | (uint32_t)SB[(t >> 8) & 255] << 8 | SB[t & 255];
            t ^= (uint32_t)rcon << 24;
            rcon = xt(rcon);
        } else if (nk > 6 && i % nk == 4) {
            t = (uint32_t)SB[t >> 24] << 24 | (uint32_t)SB[(t >> 16) & 255] << 16 | (uint32_t)SB[(t >> 8) & 255] << 8 | SB[t & 255];
        }
        a->rk[i] = a->rk[i - nk] ^ t;
    }
    return 0;
}

void aes_wipe(Aes *a) { explicit_bzero(a, sizeof *a); }

static void add_rk(uint8_t s[16], const uint32_t *rk) {
    for (int c = 0; c < 4; c++) {
        s[4 * c] ^= (uint8_t)(rk[c] >> 24); s[4 * c + 1] ^= (uint8_t)(rk[c] >> 16);
        s[4 * c + 2] ^= (uint8_t)(rk[c] >> 8); s[4 * c + 3] ^= (uint8_t)rk[c];
    }
}

void aes_encrypt(const Aes *a, const uint8_t in[16], uint8_t out[16]) {
    uint8_t s[16], t[16];
    memcpy(s, in, 16);
    add_rk(s, a->rk);
    for (int r = 1; r <= a->nr; r++) {
        for (int i = 0; i < 16; i++) s[i] = SB[s[i]];
        /* ShiftRows: row f is shifted by f positions (state in 4-byte columns) */
        for (int c = 0; c < 4; c++) for (int f = 0; f < 4; f++) t[4 * c + f] = s[4 * ((c + f) & 3) + f];
        if (r != a->nr) {
            for (int c = 0; c < 4; c++) {
                uint8_t *x = t + 4 * c, a0 = x[0], a1 = x[1], a2 = x[2], a3 = x[3];
                s[4 * c]     = (uint8_t)(xt(a0) ^ xt(a1) ^ a1 ^ a2 ^ a3);
                s[4 * c + 1] = (uint8_t)(a0 ^ xt(a1) ^ xt(a2) ^ a2 ^ a3);
                s[4 * c + 2] = (uint8_t)(a0 ^ a1 ^ xt(a2) ^ xt(a3) ^ a3);
                s[4 * c + 3] = (uint8_t)(xt(a0) ^ a0 ^ a1 ^ a2 ^ xt(a3));
            }
        } else memcpy(s, t, 16);
        add_rk(s, a->rk + 4 * r);
    }
    memcpy(out, s, 16);
    explicit_bzero(s, 16);
    explicit_bzero(t, 16);
}

void aes_decrypt(const Aes *a, const uint8_t in[16], uint8_t out[16]) {
    uint8_t s[16], t[16];
    memcpy(s, in, 16);
    add_rk(s, a->rk + 4 * a->nr);
    for (int r = a->nr - 1; r >= 0; r--) {
        /* InvShiftRows + InvSubBytes */
        for (int c = 0; c < 4; c++) for (int f = 0; f < 4; f++) t[4 * ((c + f) & 3) + f] = ISB[s[4 * c + f]];
        add_rk(t, a->rk + 4 * r);
        if (r) {
            for (int c = 0; c < 4; c++) {
                uint8_t *x = t + 4 * c, a0 = x[0], a1 = x[1], a2 = x[2], a3 = x[3];
                s[4 * c]     = (uint8_t)(gmul(a0, 14) ^ gmul(a1, 11) ^ gmul(a2, 13) ^ gmul(a3, 9));
                s[4 * c + 1] = (uint8_t)(gmul(a0, 9) ^ gmul(a1, 14) ^ gmul(a2, 11) ^ gmul(a3, 13));
                s[4 * c + 2] = (uint8_t)(gmul(a0, 13) ^ gmul(a1, 9) ^ gmul(a2, 14) ^ gmul(a3, 11));
                s[4 * c + 3] = (uint8_t)(gmul(a0, 11) ^ gmul(a1, 13) ^ gmul(a2, 9) ^ gmul(a3, 14));
            }
        } else memcpy(s, t, 16);
    }
    memcpy(out, s, 16);
    explicit_bzero(s, 16);
    explicit_bzero(t, 16);
}

int aes_cbc_encrypt(const Aes *a, uint8_t iv[16], const uint8_t *in, uint8_t *out, size_t len) {
    if (len % 16) return -1;
    uint8_t b[16];
    for (size_t off = 0; off < len; off += 16) {
        for (int i = 0; i < 16; i++) b[i] = in[off + i] ^ iv[i];
        aes_encrypt(a, b, out + off);
        memcpy(iv, out + off, 16);
    }
    explicit_bzero(b, 16);
    return 0;
}

int aes_cbc_decrypt(const Aes *a, uint8_t iv[16], const uint8_t *in, uint8_t *out, size_t len) {
    if (len % 16) return -1;
    uint8_t c[16], p[16];
    for (size_t off = 0; off < len; off += 16) {
        memcpy(c, in + off, 16);
        aes_decrypt(a, c, p);
        for (int i = 0; i < 16; i++) out[off + i] = p[i] ^ iv[i];
        memcpy(iv, c, 16);
    }
    explicit_bzero(p, 16);
    return 0;
}

long pkcs7_unpad(const uint8_t *buf, size_t len) {
    if (!len || len % 16) return -1;
    uint8_t n = buf[len - 1];
    if (!n || n > 16) return -1;
    uint8_t bad = 0;
    for (size_t i = 0; i < n; i++) bad |= buf[len - 1 - i] ^ n;
    return bad ? -1 : (long)(len - n);
}

/* ================= SHA-1 ================= */

#define ROL(x, n) (((x) << (n)) | ((x) >> (32 - (n))))

static void sha1_block(Sha1 *s, const uint8_t *p) {
    uint32_t w[80], a = s->h[0], b = s->h[1], c = s->h[2], d = s->h[3], e = s->h[4];
    for (int i = 0; i < 16; i++) w[i] = (uint32_t)p[4 * i] << 24 | (uint32_t)p[4 * i + 1] << 16 | (uint32_t)p[4 * i + 2] << 8 | p[4 * i + 3];
    for (int i = 16; i < 80; i++) w[i] = ROL(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
    for (int i = 0; i < 80; i++) {
        uint32_t f, k;
        if (i < 20) { f = (b & c) | (~b & d); k = 0x5a827999; }
        else if (i < 40) { f = b ^ c ^ d; k = 0x6ed9eba1; }
        else if (i < 60) { f = (b & c) | (b & d) | (c & d); k = 0x8f1bbcdc; }
        else { f = b ^ c ^ d; k = 0xca62c1d6; }
        uint32_t t = ROL(a, 5) + f + e + k + w[i];
        e = d; d = c; c = ROL(b, 30); b = a; a = t;
    }
    s->h[0] += a; s->h[1] += b; s->h[2] += c; s->h[3] += d; s->h[4] += e;
    explicit_bzero(w, sizeof w);
}

void sha1_init(Sha1 *s) {
    static const uint32_t iv[5] = { 0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0 };
    memcpy(s->h, iv, sizeof iv);
    s->len = 0;
    s->n = 0;
}

void sha1_update(Sha1 *s, const void *data, size_t len) {
    const uint8_t *p = data;
    s->len += len;
    while (len) {
        size_t k = 64 - s->n < len ? 64 - s->n : len;
        memcpy(s->buf + s->n, p, k);
        s->n += k; p += k; len -= k;
        if (s->n == 64) { sha1_block(s, s->buf); s->n = 0; }
    }
}

void sha1_final(Sha1 *s, uint8_t out[20]) {
    uint64_t bits = s->len * 8;
    uint8_t pad = 0x80, z = 0;
    sha1_update(s, &pad, 1);
    while (s->n != 56) sha1_update(s, &z, 1);
    uint8_t l[8];
    for (int i = 0; i < 8; i++) l[i] = (uint8_t)(bits >> (56 - 8 * i));
    sha1_update(s, l, 8);
    for (int i = 0; i < 5; i++) { out[4 * i] = (uint8_t)(s->h[i] >> 24); out[4 * i + 1] = (uint8_t)(s->h[i] >> 16); out[4 * i + 2] = (uint8_t)(s->h[i] >> 8); out[4 * i + 3] = (uint8_t)s->h[i]; }
    explicit_bzero(s, sizeof *s);
}

void sha1(const void *data, size_t len, uint8_t out[20]) { Sha1 s; sha1_init(&s); sha1_update(&s, data, len); sha1_final(&s, out); }

/* ================= HMAC ================= */

void hmac_sha1(const uint8_t *key, size_t klen, const uint8_t *msg, size_t len, uint8_t out[20]) {
    uint8_t k[64] = { 0 }, pad[64], inner[20];
    if (klen > 64) sha1(key, klen, k); else memcpy(k, key, klen);
    Sha1 s;
    for (int i = 0; i < 64; i++) pad[i] = k[i] ^ 0x36;
    sha1_init(&s); sha1_update(&s, pad, 64); sha1_update(&s, msg, len); sha1_final(&s, inner);
    for (int i = 0; i < 64; i++) pad[i] = k[i] ^ 0x5c;
    sha1_init(&s); sha1_update(&s, pad, 64); sha1_update(&s, inner, 20); sha1_final(&s, out);
    explicit_bzero(k, sizeof k); explicit_bzero(pad, sizeof pad); explicit_bzero(inner, sizeof inner);
}

void hmac_sha256_v(const uint8_t *key, size_t klen, const uint8_t *const *parts, const size_t *lens, uint8_t out[32]) {
    uint8_t k[64] = { 0 }, pad[64], inner[32];
    if (klen > 64) sha256(key, klen, k); else memcpy(k, key, klen);
    Sha256 s;
    for (int i = 0; i < 64; i++) pad[i] = k[i] ^ 0x36;
    sha256_init(&s); sha256_update(&s, pad, 64);
    for (int i = 0; parts[i]; i++) sha256_update(&s, parts[i], lens[i]);
    sha256_final(&s, inner);
    for (int i = 0; i < 64; i++) pad[i] = k[i] ^ 0x5c;
    sha256_init(&s); sha256_update(&s, pad, 64); sha256_update(&s, inner, 32); sha256_final(&s, out);
    explicit_bzero(k, sizeof k); explicit_bzero(pad, sizeof pad); explicit_bzero(inner, sizeof inner); explicit_bzero(&s, sizeof s);
}

void hmac_sha256(const uint8_t *key, size_t klen, const uint8_t *msg, size_t len, uint8_t out[32]) {
    const uint8_t *parts[2] = { msg, NULL };
    size_t lens[2] = { len, 0 };
    hmac_sha256_v(key, klen, parts, lens, out);
}

void hmac_sha512(const uint8_t *key, size_t klen, const uint8_t *msg, size_t len, uint8_t out[64]) {
    uint8_t k[128] = { 0 }, pad[128], inner[64];
    Sha512 s;
    if (klen > 128) { sha512_init(&s); sha512_update(&s, key, klen); sha512_final(&s, k); } else memcpy(k, key, klen);
    for (int i = 0; i < 128; i++) pad[i] = k[i] ^ 0x36;
    sha512_init(&s); sha512_update(&s, pad, 128); sha512_update(&s, msg, len); sha512_final(&s, inner);
    for (int i = 0; i < 128; i++) pad[i] = k[i] ^ 0x5c;
    sha512_init(&s); sha512_update(&s, pad, 128); sha512_update(&s, inner, 64); sha512_final(&s, out);
    explicit_bzero(k, sizeof k); explicit_bzero(pad, sizeof pad); explicit_bzero(inner, sizeof inner); explicit_bzero(&s, sizeof s);
}

int hkdf_sha256(const uint8_t *salt, size_t slen, const uint8_t *ikm, size_t ilen,
                const uint8_t *info, size_t infolen, uint8_t *out, size_t outlen) {
    if (outlen > 255 * 32) return -1;
    uint8_t zero[32] = { 0 }, prk[32], t[32];
    if (!salt || !slen) { salt = zero; slen = 32; }
    hmac_sha256(salt, slen, ikm, ilen, prk);
    size_t done = 0, tlen = 0;
    for (uint8_t i = 1; done < outlen; i++) {
        const uint8_t *parts[4] = { t, info, &i, NULL };
        size_t lens[4] = { tlen, infolen, 1, 0 };
        if (!info) { parts[1] = &i; lens[1] = 1; parts[2] = NULL; }
        hmac_sha256_v(prk, 32, parts, lens, t);
        tlen = 32;
        size_t k = outlen - done < 32 ? outlen - done : 32;
        memcpy(out + done, t, k);
        done += k;
    }
    explicit_bzero(prk, sizeof prk);
    explicit_bzero(t, sizeof t);
    return 0;
}

/* ================= misc ================= */

int vc_random(void *buf, size_t n) {
    uint8_t *p = buf;
    while (n) {
        ssize_t r = getrandom(p, n, 0);
        if (r < 0) { if (errno == EINTR) continue; return -1; }
        p += r; n -= (size_t)r;
    }
    return 0;
}

int vc_equal(const void *a, const void *b, size_t n) {
    const uint8_t *x = a, *y = b;
    uint8_t d = 0;
    for (size_t i = 0; i < n; i++) d |= x[i] ^ y[i];
    return d == 0;
}
