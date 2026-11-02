/* crypto.c — SHA-256, SHA-512, RSA PKCS#1 v1.5 verification and Ed25519 (see crypto.h).
 *
 * The SHA constants are the FIPS 180-4 ones (generated with tools: fractional parts of the
 * roots of the first primes). RSA uses Montgomery multiplication over 32-bit words; Ed25519
 * follows TweetNaCl's arithmetic (field 2^255-19 in 16 words of 16 bits). Verification handles
 * no secrets, so it doesn't need to be constant-time. */
#include "crypto.h"

#include <stdlib.h>
#include <string.h>

/* ---- SHA-256 ---- */

static const uint32_t K256[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u,
    0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
    0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u,
    0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
    0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu,
    0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
    0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
    0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u,
    0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
    0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u,
    0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
    0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u,
    0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u,
    0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u,
};

#define ROR32(x, n) (((x) >> (n)) | ((x) << (32 - (n))))

/* Portable C: the 64 rounds unrolled 8 at a time (the variables rotate instead of being moved),
 * the message schedule kept in 16 words. */
#define S256_CH(x, y, z)  (((x) & (y)) ^ (~(x) & (z)))
#define S256_MAJ(x, y, z) (((x) & (y)) ^ ((x) & (z)) ^ ((y) & (z)))
#define S256_E0(x) (ROR32(x, 2) ^ ROR32(x, 13) ^ ROR32(x, 22))
#define S256_E1(x) (ROR32(x, 6) ^ ROR32(x, 11) ^ ROR32(x, 25))
#define S256_S0(x) (ROR32(x, 7) ^ ROR32(x, 18) ^ ((x) >> 3))
#define S256_S1(x) (ROR32(x, 17) ^ ROR32(x, 19) ^ ((x) >> 10))
#define S256_R(a, b, c, d, e, f, g, h, i) do { \
        uint32_t t1_ = h + S256_E1(e) + S256_CH(e, f, g) + K256[i] + w[(i) & 15]; \
        d += t1_; h = t1_ + S256_E0(a) + S256_MAJ(a, b, c); } while (0)

static void sha256_block_c(uint32_t st[8], const uint8_t *p) {
    uint32_t w[16];
    for (int i = 0; i < 16; i++) w[i] = (uint32_t)p[4 * i] << 24 | (uint32_t)p[4 * i + 1] << 16 | (uint32_t)p[4 * i + 2] << 8 | p[4 * i + 3];
    uint32_t a = st[0], b = st[1], c = st[2], d = st[3], e = st[4], f = st[5], g = st[6], h = st[7];
    for (int i = 0; i < 64; i += 8) {
        if (i >= 16)
            for (int j = i; j < i + 8; j++)
                w[j & 15] += S256_S1(w[(j - 2) & 15]) + w[(j - 7) & 15] + S256_S0(w[(j - 15) & 15]);
        S256_R(a, b, c, d, e, f, g, h, i);
        S256_R(h, a, b, c, d, e, f, g, i + 1);
        S256_R(g, h, a, b, c, d, e, f, i + 2);
        S256_R(f, g, h, a, b, c, d, e, i + 3);
        S256_R(e, f, g, h, a, b, c, d, i + 4);
        S256_R(d, e, f, g, h, a, b, c, i + 5);
        S256_R(c, d, e, f, g, h, a, b, i + 6);
        S256_R(b, c, d, e, f, g, h, a, i + 7);
    }
    st[0] += a; st[1] += b; st[2] += c; st[3] += d;
    st[4] += e; st[5] += f; st[6] += g; st[7] += h;
}

/* The CPU's own SHA-256 instructions on aarch64, when it has them (checked once at run time: the
 * ARMv8 cryptography extension of the Raspberry Pi 5's Cortex-A76; the Pi 4 has none). Same results
 * bit for bit: sign_test and codesig_test run the NIST vectors through the path the machine takes,
 * and LORD_SHA256_C=1 forces the C one to compare. x86-64 uses the C code: its SHA extensions are
 * not used until they can be tested on such a CPU. */
typedef void (*Sha256Blocks)(uint32_t st[8], const uint8_t *p, size_t n);
static void sha256_blocks_c(uint32_t st[8], const uint8_t *p, size_t n) { for (; n; n--, p += 64) sha256_block_c(st, p); }

#if defined(__aarch64__) && (defined(__GNUC__) || defined(__clang__))
#include <arm_neon.h>
#include <sys/auxv.h>
#ifndef HWCAP_SHA2
#define HWCAP_SHA2 (1 << 6)
#endif
__attribute__((target("+crypto")))
static void sha256_blocks_arm(uint32_t st[8], const uint8_t *p, size_t n) {
    uint32x4_t s0 = vld1q_u32(&st[0]), s1 = vld1q_u32(&st[4]);
    for (; n; n--, p += 64) {
        uint32x4_t abcd = s0, efgh = s1, m[4];
        for (int i = 0; i < 4; i++) m[i] = vreinterpretq_u32_u8(vrev32q_u8(vld1q_u8(p + 16 * i)));
        for (int g = 0; g < 16; g++) {
            uint32x4_t w = vaddq_u32(m[g & 3], vld1q_u32(&K256[4 * g]));
            if (g < 12) m[g & 3] = vsha256su1q_u32(vsha256su0q_u32(m[g & 3], m[(g + 1) & 3]), m[(g + 2) & 3], m[(g + 3) & 3]);
            uint32x4_t t = s0;
            s0 = vsha256hq_u32(s0, s1, w);
            s1 = vsha256h2q_u32(s1, t, w);
        }
        s0 = vaddq_u32(s0, abcd);
        s1 = vaddq_u32(s1, efgh);
    }
    vst1q_u32(&st[0], s0);
    vst1q_u32(&st[4], s1);
}
static int sha256_hw(void) { return (getauxval(AT_HWCAP) & HWCAP_SHA2) != 0; }
#define SHA256_HW sha256_blocks_arm
#endif

static Sha256Blocks sha256_blocks;
static void sha256_pick(void) {
    sha256_blocks = sha256_blocks_c;
#ifdef SHA256_HW
    if (sha256_hw() && !getenv("LORD_SHA256_C")) sha256_blocks = SHA256_HW;     /* LORD_SHA256_C=1: tests */
#endif
}
static void sha256_block(Sha256 *s, const uint8_t *p) {
    if (!sha256_blocks) sha256_pick();
    sha256_blocks(s->h, p, 1);
}
/* Tests: 0 forces the C code, 1 goes back to the best path for this CPU. */
void sha256_select(int hw) {
    sha256_pick();
    if (!hw) sha256_blocks = sha256_blocks_c;
}
/* 1 if the CPU's SHA-256 instructions are in use (for the measurements in docs/SIGNING.md). */
int sha256_accelerated(void) {
    if (!sha256_blocks) sha256_pick();
    return sha256_blocks != sha256_blocks_c;
}

void sha256_init(Sha256 *s) {
    static const uint32_t h0[8] = {
    0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
    0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u,
    };
    memcpy(s->h, h0, sizeof h0);
    s->len = 0;
    s->n = 0;
}

void sha256_update(Sha256 *s, const void *data, size_t len) {
    const uint8_t *p = data;
    s->len += len;
    if (s->n) {
        size_t k = 64 - s->n < len ? 64 - s->n : len;
        memcpy(s->buf + s->n, p, k);
        s->n += k; p += k; len -= k;
        if (s->n < 64) return;
        sha256_block(s, s->buf);
        s->n = 0;
    }
    if (len >= 64) {
        if (!sha256_blocks) sha256_pick();
        sha256_blocks(s->h, p, len / 64);
        p += len & ~(size_t)63;
        len &= 63;
    }
    memcpy(s->buf, p, len);
    s->n = len;
}

void sha256_final(Sha256 *s, uint8_t out[32]) {
    uint64_t bits = s->len * 8;
    uint8_t pad[72] = { 0x80 };
    size_t padlen = (s->n < 56 ? 56 : 120) - s->n;
    for (int i = 0; i < 8; i++) pad[padlen + i] = (uint8_t)(bits >> (56 - 8 * i));
    uint64_t keep = s->len;
    sha256_update(s, pad, padlen + 8);
    s->len = keep;
    for (int i = 0; i < 8; i++) for (int j = 0; j < 4; j++) out[4 * i + j] = (uint8_t)(s->h[i] >> (24 - 8 * j));
}

void sha256(const void *data, size_t len, uint8_t out[32]) {
    Sha256 s;
    sha256_init(&s);
    sha256_update(&s, data, len);
    sha256_final(&s, out);
}

/* ---- SHA-512 ---- */

static const uint64_t K512[80] = {
    0x428a2f98d728ae22ull, 0x7137449123ef65cdull, 0xb5c0fbcfec4d3b2full, 0xe9b5dba58189dbbcull,
    0x3956c25bf348b538ull, 0x59f111f1b605d019ull, 0x923f82a4af194f9bull, 0xab1c5ed5da6d8118ull,
    0xd807aa98a3030242ull, 0x12835b0145706fbeull, 0x243185be4ee4b28cull, 0x550c7dc3d5ffb4e2ull,
    0x72be5d74f27b896full, 0x80deb1fe3b1696b1ull, 0x9bdc06a725c71235ull, 0xc19bf174cf692694ull,
    0xe49b69c19ef14ad2ull, 0xefbe4786384f25e3ull, 0x0fc19dc68b8cd5b5ull, 0x240ca1cc77ac9c65ull,
    0x2de92c6f592b0275ull, 0x4a7484aa6ea6e483ull, 0x5cb0a9dcbd41fbd4ull, 0x76f988da831153b5ull,
    0x983e5152ee66dfabull, 0xa831c66d2db43210ull, 0xb00327c898fb213full, 0xbf597fc7beef0ee4ull,
    0xc6e00bf33da88fc2ull, 0xd5a79147930aa725ull, 0x06ca6351e003826full, 0x142929670a0e6e70ull,
    0x27b70a8546d22ffcull, 0x2e1b21385c26c926ull, 0x4d2c6dfc5ac42aedull, 0x53380d139d95b3dfull,
    0x650a73548baf63deull, 0x766a0abb3c77b2a8ull, 0x81c2c92e47edaee6ull, 0x92722c851482353bull,
    0xa2bfe8a14cf10364ull, 0xa81a664bbc423001ull, 0xc24b8b70d0f89791ull, 0xc76c51a30654be30ull,
    0xd192e819d6ef5218ull, 0xd69906245565a910ull, 0xf40e35855771202aull, 0x106aa07032bbd1b8ull,
    0x19a4c116b8d2d0c8ull, 0x1e376c085141ab53ull, 0x2748774cdf8eeb99ull, 0x34b0bcb5e19b48a8ull,
    0x391c0cb3c5c95a63ull, 0x4ed8aa4ae3418acbull, 0x5b9cca4f7763e373ull, 0x682e6ff3d6b2b8a3ull,
    0x748f82ee5defb2fcull, 0x78a5636f43172f60ull, 0x84c87814a1f0ab72ull, 0x8cc702081a6439ecull,
    0x90befffa23631e28ull, 0xa4506cebde82bde9ull, 0xbef9a3f7b2c67915ull, 0xc67178f2e372532bull,
    0xca273eceea26619cull, 0xd186b8c721c0c207ull, 0xeada7dd6cde0eb1eull, 0xf57d4f7fee6ed178ull,
    0x06f067aa72176fbaull, 0x0a637dc5a2c898a6ull, 0x113f9804bef90daeull, 0x1b710b35131c471bull,
    0x28db77f523047d84ull, 0x32caab7b40c72493ull, 0x3c9ebe0a15c9bebcull, 0x431d67c49c100d4cull,
    0x4cc5d4becb3e42b6ull, 0x597f299cfc657e2aull, 0x5fcb6fab3ad6faecull, 0x6c44198c4a475817ull,
};

#define ROR64(x, n) (((x) >> (n)) | ((x) << (64 - (n))))

static void sha512_block(Sha512 *s, const uint8_t *p) {
    uint64_t w[80];
    for (int i = 0; i < 16; i++) {
        w[i] = 0;
        for (int j = 0; j < 8; j++) w[i] = w[i] << 8 | p[8 * i + j];
    }
    for (int i = 16; i < 80; i++) {
        uint64_t s0 = ROR64(w[i - 15], 1) ^ ROR64(w[i - 15], 8) ^ (w[i - 15] >> 7);
        uint64_t s1 = ROR64(w[i - 2], 19) ^ ROR64(w[i - 2], 61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    uint64_t a = s->h[0], b = s->h[1], c = s->h[2], d = s->h[3], e = s->h[4], f = s->h[5], g = s->h[6], h = s->h[7];
    for (int i = 0; i < 80; i++) {
        uint64_t t1 = h + (ROR64(e, 14) ^ ROR64(e, 18) ^ ROR64(e, 41)) + ((e & f) ^ (~e & g)) + K512[i] + w[i];
        uint64_t t2 = (ROR64(a, 28) ^ ROR64(a, 34) ^ ROR64(a, 39)) + ((a & b) ^ (a & c) ^ (b & c));
        h = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
    }
    s->h[0] += a; s->h[1] += b; s->h[2] += c; s->h[3] += d;
    s->h[4] += e; s->h[5] += f; s->h[6] += g; s->h[7] += h;
}

void sha512_init(Sha512 *s) {
    static const uint64_t h0[8] = {
    0x6a09e667f3bcc908ull, 0xbb67ae8584caa73bull, 0x3c6ef372fe94f82bull, 0xa54ff53a5f1d36f1ull,
    0x510e527fade682d1ull, 0x9b05688c2b3e6c1full, 0x1f83d9abfb41bd6bull, 0x5be0cd19137e2179ull,
    };
    memcpy(s->h, h0, sizeof h0);
    s->len = 0;
    s->n = 0;
}

void sha512_update(Sha512 *s, const void *data, size_t len) {
    const uint8_t *p = data;
    s->len += len;
    if (s->n) {
        size_t k = 128 - s->n < len ? 128 - s->n : len;
        memcpy(s->buf + s->n, p, k);
        s->n += k; p += k; len -= k;
        if (s->n < 128) return;
        sha512_block(s, s->buf);
        s->n = 0;
    }
    for (; len >= 128; p += 128, len -= 128) sha512_block(s, p);
    memcpy(s->buf, p, len);
    s->n = len;
}

void sha512_final(Sha512 *s, uint8_t out[64]) {
    uint64_t bits = s->len * 8;
    uint8_t pad[144] = { 0x80 };
    size_t padlen = (s->n < 112 ? 112 : 240) - s->n;
    for (int i = 0; i < 8; i++) pad[padlen + 8 + i] = (uint8_t)(bits >> (56 - 8 * i));  /* 128-bit length */
    uint64_t keep = s->len;
    sha512_update(s, pad, padlen + 16);
    s->len = keep;
    for (int i = 0; i < 8; i++) for (int j = 0; j < 8; j++) out[8 * i + j] = (uint8_t)(s->h[i] >> (56 - 8 * j));
}

/* ---- hexadecimal ---- */

void hex_encode(const uint8_t *in, size_t n, char *out) {
    static const char d[] = "0123456789abcdef";
    for (size_t i = 0; i < n; i++) { out[2 * i] = d[in[i] >> 4]; out[2 * i + 1] = d[in[i] & 15]; }
    out[2 * n] = 0;
}

static int hexval(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

int hex_decode(const char *in, uint8_t *out, size_t n) {
    for (size_t i = 0; i < n; i++) {
        int a = hexval(in[2 * i]), b = a < 0 ? -1 : hexval(in[2 * i + 1]);
        if (b < 0) return -1;
        out[i] = (uint8_t)(a << 4 | b);
    }
    return 0;
}

/* ---- RSA ---- */

#define MAXL 256                        /* up to 8192 bits */

static void from_be(uint32_t *a, int k, const uint8_t *p, size_t n) {
    memset(a, 0, sizeof(uint32_t) * (size_t)k);
    for (size_t i = 0; i < n; i++) {
        size_t bit = (n - 1 - i) * 8;
        if (bit / 32 < (size_t)k) a[bit / 32] |= (uint32_t)p[i] << (bit % 32);
    }
}

static int cmp_n(const uint32_t *a, const uint32_t *b, int k) {
    for (int i = k - 1; i >= 0; i--) if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
    return 0;
}

static void sub_n(uint32_t *a, const uint32_t *b, int k) {
    uint64_t borrow = 0;
    for (int i = 0; i < k; i++) {
        uint64_t d = (uint64_t)a[i] - b[i] - borrow;
        a[i] = (uint32_t)d;
        borrow = (d >> 32) & 1;
    }
}

/* r = a·b·R⁻¹ mod n (CIOS). r may be a or b. */
static void mont_mul(uint32_t *r, const uint32_t *a, const uint32_t *b, const uint32_t *n, uint32_t n0, int k) {
    uint32_t t[MAXL + 2];
    memset(t, 0, sizeof(uint32_t) * (size_t)(k + 2));
    for (int i = 0; i < k; i++) {
        uint64_t c = 0;
        for (int j = 0; j < k; j++) {
            uint64_t x = (uint64_t)t[j] + (uint64_t)a[j] * b[i] + c;
            t[j] = (uint32_t)x;
            c = x >> 32;
        }
        uint64_t x = (uint64_t)t[k] + c;
        t[k] = (uint32_t)x;
        t[k + 1] = (uint32_t)(x >> 32);
        uint32_t m = t[0] * n0;
        c = ((uint64_t)t[0] + (uint64_t)m * n[0]) >> 32;
        for (int j = 1; j < k; j++) {
            x = (uint64_t)t[j] + (uint64_t)m * n[j] + c;
            t[j - 1] = (uint32_t)x;
            c = x >> 32;
        }
        x = (uint64_t)t[k] + c;
        t[k - 1] = (uint32_t)x;
        t[k] = t[k + 1] + (uint32_t)(x >> 32);
    }
    if (t[k] || cmp_n(t, n, k) >= 0) sub_n(t, n, k);
    memcpy(r, t, sizeof(uint32_t) * (size_t)k);
}

int rsa_verify(const uint8_t *n_be, size_t nlen, const uint8_t *e_be, size_t elen,
               const uint8_t *sig, size_t siglen, int hash, const uint8_t *digest) {
    static const uint8_t info256[] = { 0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20 };
    static const uint8_t info512[] = { 0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40 };
    const uint8_t *info = hash == HASH_SHA256 ? info256 : hash == HASH_SHA512 ? info512 : NULL;
    size_t infolen = hash == HASH_SHA256 ? sizeof info256 : sizeof info512, dlen = hash == HASH_SHA256 ? 32 : 64;
    while (nlen && !*n_be) { n_be++; nlen--; }
    while (siglen && !*sig) { sig++; siglen--; }
    if (!info || nlen < 128 || nlen > MAXL * 4 || siglen > nlen || !elen || elen > 8) return 0;   /* at least 1024 bits */
    int k = (int)((nlen + 3) / 4);
    uint32_t n[MAXL], s[MAXL], r2[MAXL], acc[MAXL], sm[MAXL], one[MAXL];
    from_be(n, k, n_be, nlen);
    from_be(s, k, sig, siglen);
    if (!(n[0] & 1) || cmp_n(s, n, k) >= 0) return 0;
    uint32_t inv = n[0];                   /* n[0]⁻¹ mod 2^32 via Newton */
    for (int i = 0; i < 5; i++) inv *= 2 - n[0] * inv;
    uint32_t n0 = (uint32_t)0 - inv;
    /* R² mod n by doubling 2·32·k times */
    memset(r2, 0, sizeof(uint32_t) * (size_t)k);
    r2[0] = 1;
    for (int i = 0; i < 64 * k; i++) {
        uint32_t carry = 0;
        for (int j = 0; j < k; j++) { uint32_t v = r2[j]; r2[j] = v << 1 | carry; carry = v >> 31; }
        if (carry || cmp_n(r2, n, k) >= 0) sub_n(r2, n, k);
    }
    memset(one, 0, sizeof(uint32_t) * (size_t)k);
    one[0] = 1;
    mont_mul(sm, s, r2, n, n0, k);
    mont_mul(acc, one, r2, n, n0, k);      /* R mod n = 1 in Montgomery form */
    for (size_t i = 0; i < elen; i++)
        for (int b = 7; b >= 0; b--) {
            mont_mul(acc, acc, acc, n, n0, k);
            if (e_be[i] >> b & 1) mont_mul(acc, acc, sm, n, n0, k);
        }
    mont_mul(acc, acc, one, n, n0, k);
    /* EM = 00 01 FF…FF 00 DigestInfo hash */
    uint8_t em[MAXL * 4];
    for (size_t i = 0; i < nlen; i++) {
        size_t bit = (nlen - 1 - i) * 8;
        em[i] = (uint8_t)(acc[bit / 32] >> (bit % 32));
    }
    size_t ps = nlen - 3 - infolen - dlen;
    if (nlen < 11 + infolen + dlen || em[0] != 0 || em[1] != 1) return 0;
    for (size_t i = 0; i < ps; i++) if (em[2 + i] != 0xff) return 0;
    return em[2 + ps] == 0 && !memcmp(em + 3 + ps, info, infolen) && !memcmp(em + 3 + ps + infolen, digest, dlen);
}

/* ---- Ed25519 ---- */

typedef int64_t gf[16];

static const gf gf0 = { 0 }, gf1 = { 1 };
static const gf D = { 0x78a3, 0x1359, 0x4dca, 0x75eb, 0xd8ab, 0x4141, 0x0a4d, 0x0070, 0xe898, 0x7779, 0x4079, 0x8cc7, 0xfe73, 0x2b6f, 0x6cee, 0x5203 };
static const gf D2 = { 0xf159, 0x26b2, 0x9b94, 0xebd6, 0xb156, 0x8283, 0x149a, 0x00e0, 0xd130, 0xeef3, 0x80f2, 0x198e, 0xfce7, 0x56df, 0xd9dc, 0x2406 };
static const gf X = { 0xd51a, 0x8f25, 0x2d60, 0xc956, 0xa7b2, 0x9525, 0xc760, 0x692c, 0xdc5c, 0xfdd6, 0xe231, 0xc0a4, 0x53fe, 0xcd6e, 0x36d3, 0x2169 };
static const gf Y = { 0x6658, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666 };
static const gf I = { 0xa0b0, 0x4a0e, 0x1b27, 0xc4ee, 0xe478, 0xad2f, 0x1806, 0x2f43, 0xd7a7, 0x3dfb, 0x0099, 0x2b4d, 0xdf0b, 0x4fc1, 0x2480, 0x2b83 };
static const int64_t L[32] = { 0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
                               0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10 };

static void set25519(gf r, const gf a) { for (int i = 0; i < 16; i++) r[i] = a[i]; }

static void car25519(gf o) {
    for (int i = 0; i < 16; i++) {
        o[i] += 1 << 16;
        int64_t c = o[i] >> 16;
        o[(i + 1) * (i < 15)] += c - 1 + 37 * (c - 1) * (i == 15);
        o[i] -= c * 65536;
    }
}

static void sel25519(gf p, gf q, int b) {
    int64_t c = ~(int64_t)(b - 1);
    for (int i = 0; i < 16; i++) { int64_t t = c & (p[i] ^ q[i]); p[i] ^= t; q[i] ^= t; }
}

static void pack25519(uint8_t *o, const gf n) {
    gf m, t;
    set25519(t, n);
    car25519(t); car25519(t); car25519(t);
    for (int j = 0; j < 2; j++) {
        m[0] = t[0] - 0xffed;
        for (int i = 1; i < 15; i++) {
            m[i] = t[i] - 0xffff - ((m[i - 1] >> 16) & 1);
            m[i - 1] &= 0xffff;
        }
        m[15] = t[15] - 0x7fff - ((m[14] >> 16) & 1);
        int b = (int)((m[15] >> 16) & 1);
        m[14] &= 0xffff;
        sel25519(t, m, 1 - b);
    }
    for (int i = 0; i < 16; i++) { o[2 * i] = (uint8_t)(t[i] & 0xff); o[2 * i + 1] = (uint8_t)(t[i] >> 8); }
}

static int neq25519(const gf a, const gf b) {
    uint8_t c[32], d[32];
    pack25519(c, a);
    pack25519(d, b);
    return memcmp(c, d, 32) != 0;
}

static uint8_t par25519(const gf a) {
    uint8_t d[32];
    pack25519(d, a);
    return d[0] & 1;
}

static void unpack25519(gf o, const uint8_t *n) {
    for (int i = 0; i < 16; i++) o[i] = n[2 * i] + ((int64_t)n[2 * i + 1] << 8);
    o[15] &= 0x7fff;
}

static void fA(gf o, const gf a, const gf b) { for (int i = 0; i < 16; i++) o[i] = a[i] + b[i]; }
static void fZ(gf o, const gf a, const gf b) { for (int i = 0; i < 16; i++) o[i] = a[i] - b[i]; }

static void fM(gf o, const gf a, const gf b) {
    int64_t t[31] = { 0 };
    for (int i = 0; i < 16; i++) for (int j = 0; j < 16; j++) t[i + j] += a[i] * b[j];
    for (int i = 0; i < 15; i++) t[i] += 38 * t[i + 16];
    for (int i = 0; i < 16; i++) o[i] = t[i];
    car25519(o);
    car25519(o);
}

static void fS(gf o, const gf a) { fM(o, a, a); }

static void pow2523(gf o, const gf i) {
    gf c;
    set25519(c, i);
    for (int a = 250; a >= 0; a--) { fS(c, c); if (a != 1) fM(c, c, i); }
    set25519(o, c);
}

static void inv25519(gf o, const gf i) {
    gf c;
    set25519(c, i);
    for (int a = 253; a >= 0; a--) { fS(c, c); if (a != 2 && a != 4) fM(c, c, i); }
    set25519(o, c);
}

static void padd(gf p[4], gf q[4]) {
    gf a, b, c, d, t, e, f, g, h;
    fZ(a, p[1], p[0]); fZ(t, q[1], q[0]); fM(a, a, t);
    fA(b, p[0], p[1]); fA(t, q[0], q[1]); fM(b, b, t);
    fM(c, p[3], q[3]); fM(c, c, D2);
    fM(d, p[2], q[2]); fA(d, d, d);
    fZ(e, b, a); fZ(f, d, c); fA(g, d, c); fA(h, b, a);
    fM(p[0], e, f); fM(p[1], h, g); fM(p[2], g, f); fM(p[3], e, h);
}

static void cswap(gf p[4], gf q[4], int b) { for (int i = 0; i < 4; i++) sel25519(p[i], q[i], b); }

static void ppack(uint8_t *r, gf p[4]) {
    gf tx, ty, zi;
    inv25519(zi, p[2]);
    fM(tx, p[0], zi);
    fM(ty, p[1], zi);
    pack25519(r, ty);
    r[31] ^= (uint8_t)(par25519(tx) << 7);
}

static void scalarmult(gf p[4], gf q[4], const uint8_t *s) {
    set25519(p[0], gf0); set25519(p[1], gf1); set25519(p[2], gf1); set25519(p[3], gf0);
    for (int i = 255; i >= 0; --i) {
        int b = (s[i / 8] >> (i & 7)) & 1;
        cswap(p, q, b);
        padd(q, p);
        padd(p, p);
        cswap(p, q, b);
    }
}

static void scalarbase(gf p[4], const uint8_t *s) {
    gf q[4];
    set25519(q[0], X); set25519(q[1], Y); set25519(q[2], gf1); fM(q[3], X, Y);
    scalarmult(p, q, s);
}

static void modL(uint8_t *r, int64_t x[64]) {
    int64_t carry;
    for (int i = 63; i >= 32; --i) {
        int j;
        carry = 0;
        for (j = i - 32; j < i - 12; ++j) {
            x[j] += carry - 16 * x[i] * L[j - (i - 32)];
            carry = (x[j] + 128) >> 8;
            x[j] -= carry * 256;
        }
        x[j] += carry;
        x[i] = 0;
    }
    carry = 0;
    for (int j = 0; j < 32; j++) {
        x[j] += carry - (x[31] >> 4) * L[j];
        carry = x[j] >> 8;
        x[j] &= 255;
    }
    for (int j = 0; j < 32; j++) x[j] -= carry * L[j];
    for (int i = 0; i < 32; i++) {
        x[i + 1] += x[i] >> 8;
        r[i] = (uint8_t)(x[i] & 255);
    }
}

static void reduce(uint8_t *r) {
    int64_t x[64];
    for (int i = 0; i < 64; i++) x[i] = r[i];
    memset(r, 0, 64);
    modL(r, x);
}

static int unpackneg(gf r[4], const uint8_t p[32]) {
    gf t, chk, num, den, den2, den4, den6;
    set25519(r[2], gf1);
    unpack25519(r[1], p);
    fS(num, r[1]);
    fM(den, num, D);
    fZ(num, num, r[2]);
    fA(den, r[2], den);
    fS(den2, den); fS(den4, den2); fM(den6, den4, den2);
    fM(t, den6, num); fM(t, t, den);
    pow2523(t, t);
    fM(t, t, num); fM(t, t, den); fM(t, t, den); fM(r[0], t, den);
    fS(chk, r[0]); fM(chk, chk, den);
    if (neq25519(chk, num)) fM(r[0], r[0], I);
    fS(chk, r[0]); fM(chk, chk, den);
    if (neq25519(chk, num)) return -1;
    if (par25519(r[0]) == (p[31] >> 7)) fZ(r[0], gf0, r[0]);
    fM(r[3], r[0], r[1]);
    return 0;
}

int ed25519_verify(const uint8_t pub[32], const uint8_t sig[64], const uint8_t *msg, size_t len) {
    /* S < L (no malleable signatures) */
    for (int i = 31; i >= 0; i--) {
        if (sig[32 + i] < L[i]) break;
        if (sig[32 + i] > L[i] || i == 0) return 0;
    }
    gf p[4], q[4];
    if (unpackneg(q, pub)) return 0;
    uint8_t h[64], t[32];
    Sha512 s;
    sha512_init(&s);
    sha512_update(&s, sig, 32);
    sha512_update(&s, pub, 32);
    sha512_update(&s, msg, len);
    sha512_final(&s, h);
    reduce(h);
    scalarmult(p, q, h);
    scalarbase(q, sig + 32);
    padd(p, q);
    ppack(t, p);
    return memcmp(sig, t, 32) == 0;
}

/* ---- Ed25519: signing (the user's own keys and the development one; see docs/SIGNING.md) ----
 * Same as TweetNaCl's crypto_sign (public domain): private key = 32-byte seed. */

static void ed25519_expand(const uint8_t seed[32], uint8_t d[64]) {
    Sha512 s;
    sha512_init(&s);
    sha512_update(&s, seed, 32);
    sha512_final(&s, d);
    d[0] &= 248;
    d[31] &= 127;
    d[31] |= 64;
}

void ed25519_public(const uint8_t seed[32], uint8_t pub[32]) {
    uint8_t d[64];
    gf p[4];
    ed25519_expand(seed, d);
    scalarbase(p, d);
    ppack(pub, p);
    memset(d, 0, sizeof d);
}

void ed25519_sign(const uint8_t seed[32], const uint8_t *msg, size_t len, uint8_t sig[64]) {
    uint8_t d[64], pub[32], r[64], h[64];
    gf p[4];
    ed25519_expand(seed, d);
    scalarbase(p, d);
    ppack(pub, p);
    Sha512 s;
    sha512_init(&s);
    sha512_update(&s, d + 32, 32);
    sha512_update(&s, msg, len);
    sha512_final(&s, r);
    reduce(r);
    scalarbase(p, r);
    ppack(sig, p);
    sha512_init(&s);
    sha512_update(&s, sig, 32);
    sha512_update(&s, pub, 32);
    sha512_update(&s, msg, len);
    sha512_final(&s, h);
    reduce(h);
    int64_t x[64];
    for (int i = 0; i < 64; i++) x[i] = 0;
    for (int i = 0; i < 32; i++) x[i] = (int64_t)r[i];
    for (int i = 0; i < 32; i++) for (int j = 0; j < 32; j++) x[i + j] += (int64_t)h[i] * d[j];
    modL(sig + 32, x);
    memset(d, 0, sizeof d);
    memset(r, 0, sizeof r);
}
