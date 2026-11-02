/* kdbx.c — KDBX 3.1 and 4.x reader (see kdbx.h).
 *
 * Format reference: the KeePass 2.x code (KdbxFile.*) and KeePassXC (Kdbx3Reader,
 * Kdbx4Reader, KeePass2RandomStream). Everything decrypted goes in the secrets arena. */
#include "kdbx.h"
#include "smem.h"
#include "vcrypto.h"
#include "vjson.h"
#include "vstb.h"
#include "xmlmini.h"
#include "../pkg/crypto.h"

#include <monocypher.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

uint64_t kdbx_max_aes_rounds = 500000000ull;
uint64_t kdbx_max_argon_kib = 2ull * 1024 * 1024;

#define MAX_PAYLOAD (512u * 1024 * 1024)

static const uint8_t SIG[8] = { 0x03, 0xd9, 0xa2, 0x9a, 0x67, 0xfb, 0x4b, 0xb5 };
static const uint8_t U_AES[16] = { 0x31,0xc1,0xf2,0xe6,0xbf,0x71,0x43,0x50,0xbe,0x58,0x05,0x21,0x6a,0xfc,0x5a,0xff };
static const uint8_t U_CHACHA[16] = { 0xd6,0x03,0x8a,0x2b,0x8b,0x6f,0x4c,0xb5,0xa5,0x24,0x33,0x9a,0x31,0xdb,0xb5,0x9a };
static const uint8_t U_ARGON2D[16] = { 0xef,0x63,0x6d,0xdf,0x8c,0x29,0x44,0x4b,0x91,0xf7,0xa9,0xa4,0x03,0xe3,0x0a,0x0c };
static const uint8_t U_ARGON2ID[16] = { 0x9e,0x29,0x8b,0x19,0x56,0xdb,0x47,0x73,0xb2,0x3d,0xfc,0x3e,0xc6,0xf0,0xa1,0xe6 };
static const uint8_t U_AESKDF[16] = { 0xc9,0xd9,0xf3,0x9a,0x62,0x8a,0x44,0x60,0xbf,0x74,0x0d,0x08,0xc1,0x8a,0x4f,0xea };
/* KeePassXC also writes AES-KDF with this UUID ("KDBX 4") */
static const uint8_t U_AESKDF4[16] = { 0x7c,0x02,0xbb,0x82,0x79,0xa7,0x4a,0xc0,0x92,0x7d,0x11,0x4a,0x00,0x64,0x82,0x38 };

static uint32_t rd16(const uint8_t *p) { return (uint32_t)p[0] | (uint32_t)p[1] << 8; }
static uint32_t rd32(const uint8_t *p) { return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24; }
static uint64_t rd64(const uint8_t *p) { return (uint64_t)rd32(p) | (uint64_t)rd32(p + 4) << 32; }
static uint32_t rd32be(const uint8_t *p) { return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; }
static void wr32be(uint8_t *p, uint32_t v) { p[0] = (uint8_t)(v >> 24); p[1] = (uint8_t)(v >> 16); p[2] = (uint8_t)(v >> 8); p[3] = (uint8_t)v; }
static void wr64(uint8_t *p, uint64_t v) { for (int i = 0; i < 8; i++) p[i] = (uint8_t)(v >> (8 * i)); }

static int fail(ImpList *l, const char *code) { snprintf(l->err, sizeof l->err, "%s", code); return -1; }

int kdbx_is(const uint8_t *d, size_t len) { return len >= 8 && !memcmp(d, SIG, 8); }

/* ---- key file ---- */

static int hexv(int c) { return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1; }

int kdbx_keyfile_key(const uint8_t *kf, size_t n, uint8_t out[32]) {
    /* KeePass XML (1.0: base64; 2.0: hex with Hash = first 4 bytes of the SHA-256) */
    if (n > 8 && n < 65536 && memmem(kf, n, "<KeyFile", 8)) {
        XNode *x = xml_parse((const char *)kf, n, NULL);
        if (x && !strcmp(x->name, "KeyFile")) {
            const char *ver = xml_text(xml_child(xml_child(x, "Meta"), "Version"));
            XNode *data = xml_child(xml_child(x, "Key"), "Data");
            int ok = -1;
            if (data && !strncmp(ver, "2.", 2)) {
                uint8_t k[32];
                size_t m = 0;
                const char *t = xml_text(data);
                for (; *t && m < 64; t++) {
                    int v = hexv((unsigned char)*t);
                    if (v < 0) continue;              /* spaces and line breaks */
                    if (m % 2 == 0) k[m / 2] = (uint8_t)(v << 4); else k[m / 2] |= (uint8_t)v;
                    m++;
                }
                if (m == 64) {
                    const char *hash = xml_attr(data, "Hash");
                    ok = 0;
                    if (hash && *hash) {
                        uint8_t h[32];
                        sha256(k, 32, h);
                        char hx[9];
                        snprintf(hx, sizeof hx, "%02X%02X%02X%02X", h[0], h[1], h[2], h[3]);
                        if (strlen(hash) != 8 || strncasecmp(hash, hx, 8)) ok = -1;
                    }
                    if (!ok) memcpy(out, k, 32);
                }
                sm_wipe(k, sizeof k);
            } else if (data) {
                size_t m = 0;
                uint8_t *k = b64_decode(xml_text(data), strlen(xml_text(data)), &m);
                if (k && m == 32) { memcpy(out, k, 32); ok = 0; }
                sm_free(k);
            }
            xml_free(x);
            if (ok == 0) return 0;
            if (data) return -1;
        } else xml_free(x);
    }
    if (n == 32) { memcpy(out, kf, 32); return 0; }
    if (n == 64) {
        int hex = 1;
        for (size_t i = 0; i < 64; i++) if (hexv(kf[i]) < 0) hex = 0;
        if (hex) { for (int i = 0; i < 32; i++) out[i] = (uint8_t)(hexv(kf[2 * i]) << 4 | hexv(kf[2 * i + 1])); return 0; }
    }
    sha256(kf, n, out);
    return 0;
}

/* ---- inner stream (protected values) ---- */

typedef struct { int kind; uint8_t key[32], nonce[12], buf[64]; int pos; uint64_t ctr; } Stream;

#define ROTL(x, n) ((uint32_t)((x) << (n)) | ((x) >> (32 - (n))))

static void salsa20_block(const uint8_t key[32], const uint8_t nonce[8], uint64_t ctr, uint8_t out[64]) {
    static const uint8_t c[16] = "expand 32-byte k";
    uint32_t j[16], x[16];
    j[0] = rd32(c); j[5] = rd32(c + 4); j[10] = rd32(c + 8); j[15] = rd32(c + 12);
    for (int i = 0; i < 4; i++) { j[1 + i] = rd32(key + 4 * i); j[11 + i] = rd32(key + 16 + 4 * i); }
    j[6] = rd32(nonce); j[7] = rd32(nonce + 4);
    j[8] = (uint32_t)ctr; j[9] = (uint32_t)(ctr >> 32);
    memcpy(x, j, sizeof x);
#define QR(a, b, c2, d) x[b] ^= ROTL(x[a] + x[d], 7); x[c2] ^= ROTL(x[b] + x[a], 9); x[d] ^= ROTL(x[c2] + x[b], 13); x[a] ^= ROTL(x[d] + x[c2], 18)
    for (int r = 0; r < 10; r++) {
        QR(0, 4, 8, 12); QR(5, 9, 13, 1); QR(10, 14, 2, 6); QR(15, 3, 7, 11);
        QR(0, 1, 2, 3); QR(5, 6, 7, 4); QR(10, 11, 8, 9); QR(15, 12, 13, 14);
    }
#undef QR
    for (int i = 0; i < 16; i++) { uint32_t v = x[i] + j[i]; out[4 * i] = (uint8_t)v; out[4 * i + 1] = (uint8_t)(v >> 8); out[4 * i + 2] = (uint8_t)(v >> 16); out[4 * i + 3] = (uint8_t)(v >> 24); }
    sm_wipe(x, sizeof x);
    sm_wipe(j, sizeof j);
}

static const uint8_t SALSA_IV[8] = { 0xe8, 0x30, 0x09, 0x4b, 0x97, 0x20, 0x5d, 0x2a };

/* id: 2 Salsa20, 3 ChaCha20 (0 = no protection) */
static int stream_init(Stream *s, uint32_t id, const uint8_t *key, size_t klen) {
    memset(s, 0, sizeof *s);
    s->pos = 64;
    s->kind = (int)id;
    if (id == 0) return 0;
    if (id == 2) { sha256(key, klen, s->key); memcpy(s->nonce, SALSA_IV, 8); return 0; }
    if (id == 3) {
        uint8_t h[64];
        Sha512 x;
        sha512_init(&x); sha512_update(&x, key, klen); sha512_final(&x, h);
        memcpy(s->key, h, 32);
        memcpy(s->nonce, h + 32, 12);
        sm_wipe(h, sizeof h);
        return 0;
    }
    return -1;
}

static void stream_xor(Stream *s, uint8_t *p, size_t n) {
    if (!s->kind) return;
    for (size_t i = 0; i < n; i++) {
        if (s->pos == 64) {
            if (s->kind == 2) salsa20_block(s->key, s->nonce, s->ctr, s->buf);
            else crypto_chacha20_ietf(s->buf, NULL, 64, s->key, s->nonce, (uint32_t)s->ctr);
            s->ctr++;
            s->pos = 0;
        }
        p[i] ^= s->buf[s->pos++];
    }
}

/* ---- header ---- */

typedef struct {
    int major, minor;
    size_t hdr_end;
    const uint8_t *cipher, *seed, *iv, *kdf, *tseed, *pskey, *start;
    size_t ivlen, kdflen, pskeylen;
    uint32_t comp, inner_id;
    uint64_t rounds;
    int have_rounds, have_inner;
} Hdr;

static int read_header(const uint8_t *d, size_t len, Hdr *h, ImpList *l) {
    memset(h, 0, sizeof *h);
    if (len < 12) return fail(l, "truncated");
    if (!kdbx_is(d, len)) return fail(l, "corrupt");
    uint32_t ver = rd32(d + 8);
    h->major = (int)(ver >> 16);
    h->minor = (int)(ver & 0xffff);
    if (h->major != 3 && h->major != 4) return fail(l, "unsupported-version");
    size_t p = 12, lsz = h->major == 4 ? 4 : 2;
    for (;;) {
        if (p + 1 + lsz > len) return fail(l, "truncated");
        uint8_t id = d[p];
        size_t sz = lsz == 4 ? rd32(d + p + 1) : rd16(d + p + 1);
        p += 1 + lsz;
        if (sz > len - p) return fail(l, "truncated");
        const uint8_t *v = d + p;
        p += sz;
        switch (id) {
        case 0: h->hdr_end = p; goto done;
        case 2: if (sz != 16) return fail(l, "corrupt"); h->cipher = v; break;
        case 3: if (sz != 4) return fail(l, "corrupt"); h->comp = rd32(v); break;
        case 4: if (sz != 32) return fail(l, "corrupt"); h->seed = v; break;
        case 5: if (sz != 32) return fail(l, "corrupt"); h->tseed = v; break;
        case 6: if (sz != 8) return fail(l, "corrupt"); h->rounds = rd64(v); h->have_rounds = 1; break;
        case 7: h->iv = v; h->ivlen = sz; break;
        case 8: h->pskey = v; h->pskeylen = sz; break;
        case 9: if (sz != 32) return fail(l, "corrupt"); h->start = v; break;
        case 10: if (sz != 4) return fail(l, "corrupt"); h->inner_id = rd32(v); h->have_inner = 1; break;
        case 11: h->kdf = v; h->kdflen = sz; break;
        default: break;                          /* 12: public data, and whatever else comes */
        }
    }
done:
    if (!h->cipher || !h->seed || !h->iv) return fail(l, "corrupt");
    if (h->comp > 1) return fail(l, "corrupt");
    if (memcmp(h->cipher, U_AES, 16) && memcmp(h->cipher, U_CHACHA, 16)) return fail(l, "unsupported-cipher");
    if (!memcmp(h->cipher, U_AES, 16) && h->ivlen != 16) return fail(l, "corrupt");
    if (!memcmp(h->cipher, U_CHACHA, 16) && h->ivlen != 12) return fail(l, "corrupt");
    if (h->major == 4 && !h->kdf) return fail(l, "corrupt");
    if (h->major == 3 && (!h->tseed || !h->have_rounds || !h->start || !h->pskey || !h->have_inner)) return fail(l, "corrupt");
    return 0;
}

/* VariantDictionary (KDBX 4 KDF) */
typedef struct { uint8_t uuid[16]; const uint8_t *salt, *key, *ad; size_t saltlen, keylen, adlen; uint64_t rounds, iter, mem; uint32_t par, ver; int have_uuid; } Kdf;

static int read_kdf(const uint8_t *v, size_t n, Kdf *k) {
    memset(k, 0, sizeof *k);
    if (n < 2 || v[1] != 1) return -1;          /* version 1.x */
    size_t p = 2;
    for (;;) {
        if (p >= n) return -1;
        uint8_t type = v[p++];
        if (type == 0) return 0;
        if (p + 4 > n) return -1;
        size_t kl = rd32(v + p); p += 4;
        if (kl > n - p) return -1;
        const uint8_t *key = v + p; p += kl;
        if (p + 4 > n) return -1;
        size_t vl = rd32(v + p); p += 4;
        if (vl > n - p) return -1;
        const uint8_t *val = v + p; p += vl;
#define K(s) (kl == strlen(s) && !memcmp(key, s, kl))
        if (K("$UUID") && type == 0x42 && vl == 16) { memcpy(k->uuid, val, 16); k->have_uuid = 1; }
        else if (K("R") && type == 0x05 && vl == 8) k->rounds = rd64(val);
        else if (K("S") && type == 0x42) { k->salt = val; k->saltlen = vl; }
        else if (K("I") && type == 0x05 && vl == 8) k->iter = rd64(val);
        else if (K("M") && type == 0x05 && vl == 8) k->mem = rd64(val);
        else if (K("P") && type == 0x04 && vl == 4) k->par = rd32(val);
        else if (K("V") && type == 0x04 && vl == 4) k->ver = rd32(val);
        else if (K("K") && type == 0x42) { k->key = val; k->keylen = vl; }
        else if (K("A") && type == 0x42) { k->ad = val; k->adlen = vl; }
#undef K
    }
}

/* ---- fast AES-KDF ----
 * keepassxc-cli creates KDBX 3.1 databases with AES-KDF calibrated to 1 s with AES-NI: about
 * 30 million rounds (60 million blocks). The bytewise AES in vcrypto.c takes a minute on
 * that, so here is a dedicated path for encryption only: AES-NI on x86-64 if the CPU has it
 * (checked at run time) and otherwise T-tables (like the classic "fast" AES, ~10 times the bytewise one).
 * The round keys come from aes_init (vcrypto.c). */
static uint32_t T0[256], T1[256], T2[256], T3[256];
static uint8_t SBX[256];
static int tables_ready;

static uint8_t gf_mul2(uint8_t x) { return (uint8_t)((x << 1) ^ ((x >> 7) * 0x1b)); }

static void make_tables(void) {
    if (tables_ready) return;
    /* AES S-box: inverse in GF(2^8) (with base-3 logarithms) + affine transform */
    uint8_t lg[256] = { 0 }, ex[256];
    uint8_t x = 1;
    for (int i = 0; i < 255; i++) { ex[i] = x; lg[x] = (uint8_t)i; x = (uint8_t)(x ^ gf_mul2(x)); }
    for (int i = 0; i < 256; i++) {
        uint8_t inv = i ? ex[(255 - lg[i]) % 255] : 0, sb = inv;
        for (int k = 1; k < 5; k++) sb ^= (uint8_t)((inv << k) | (inv >> (8 - k)));
        SBX[i] = (uint8_t)(sb ^ 0x63);
    }
    for (int i = 0; i < 256; i++) {
        uint8_t v = SBX[i], v2 = gf_mul2(v), v3 = (uint8_t)(v2 ^ v);
        T0[i] = (uint32_t)v2 << 24 | (uint32_t)v << 16 | (uint32_t)v << 8 | v3;
        T1[i] = T0[i] >> 8 | T0[i] << 24;
        T2[i] = T0[i] >> 16 | T0[i] << 16;
        T3[i] = T0[i] >> 24 | T0[i] << 8;
    }
    tables_ready = 1;
}

static void ttable_block(const uint32_t *rk, int nr, uint8_t b[16]) {
    uint32_t s0 = rd32be(b) ^ rk[0], s1 = rd32be(b + 4) ^ rk[1], s2 = rd32be(b + 8) ^ rk[2], s3 = rd32be(b + 12) ^ rk[3], t0, t1, t2, t3;
    for (int r = 1; r < nr; r++) {
        const uint32_t *k = rk + 4 * r;
        t0 = T0[s0 >> 24] ^ T1[(s1 >> 16) & 255] ^ T2[(s2 >> 8) & 255] ^ T3[s3 & 255] ^ k[0];
        t1 = T0[s1 >> 24] ^ T1[(s2 >> 16) & 255] ^ T2[(s3 >> 8) & 255] ^ T3[s0 & 255] ^ k[1];
        t2 = T0[s2 >> 24] ^ T1[(s3 >> 16) & 255] ^ T2[(s0 >> 8) & 255] ^ T3[s1 & 255] ^ k[2];
        t3 = T0[s3 >> 24] ^ T1[(s0 >> 16) & 255] ^ T2[(s1 >> 8) & 255] ^ T3[s2 & 255] ^ k[3];
        s0 = t0; s1 = t1; s2 = t2; s3 = t3;
    }
    const uint32_t *k = rk + 4 * nr;
#define LAST(a, b2, c, d) ((uint32_t)SBX[(a) >> 24] << 24 | (uint32_t)SBX[((b2) >> 16) & 255] << 16 | (uint32_t)SBX[((c) >> 8) & 255] << 8 | SBX[(d) & 255])
    t0 = LAST(s0, s1, s2, s3) ^ k[0];
    t1 = LAST(s1, s2, s3, s0) ^ k[1];
    t2 = LAST(s2, s3, s0, s1) ^ k[2];
    t3 = LAST(s3, s0, s1, s2) ^ k[3];
#undef LAST
    wr32be(b, t0); wr32be(b + 4, t1); wr32be(b + 8, t2); wr32be(b + 12, t3);
}

#if defined(__x86_64__) && defined(__GNUC__)
#include <immintrin.h>
__attribute__((target("aes,sse2")))
static void aesni_kdf(const uint8_t rkb[15][16], uint8_t k[32], uint64_t rounds) {
    __m128i K[15];
    for (int i = 0; i < 15; i++) K[i] = _mm_loadu_si128((const __m128i *)rkb[i]);
    __m128i a = _mm_loadu_si128((const __m128i *)k), b = _mm_loadu_si128((const __m128i *)(k + 16));
    for (uint64_t r = 0; r < rounds; r++) {
        a = _mm_xor_si128(a, K[0]); b = _mm_xor_si128(b, K[0]);
        for (int i = 1; i < 14; i++) { a = _mm_aesenc_si128(a, K[i]); b = _mm_aesenc_si128(b, K[i]); }
        a = _mm_aesenclast_si128(a, K[14]); b = _mm_aesenclast_si128(b, K[14]);
    }
    _mm_storeu_si128((__m128i *)k, a);
    _mm_storeu_si128((__m128i *)(k + 16), b);
    for (int i = 0; i < 15; i++) K[i] = _mm_setzero_si128();
}
#endif

int kdbx_aes_kdf_mode = 0;      /* 0 automatic, 1 force tables (tests) */

static int aes_kdf(const uint8_t comp[32], const uint8_t seed[32], uint64_t rounds, uint8_t out[32]) {
    Aes a;
    if (aes_init(&a, seed, 256)) return -1;
    uint8_t k[32];
    memcpy(k, comp, 32);
#if defined(__x86_64__) && defined(__GNUC__)
    if (kdbx_aes_kdf_mode == 0 && __builtin_cpu_supports("aes")) {
        uint8_t rkb[15][16];
        for (int i = 0; i < 60; i++) wr32be(rkb[i / 4] + 4 * (i % 4), a.rk[i]);
        aesni_kdf((const uint8_t (*)[16])rkb, k, rounds);
        sm_wipe(rkb, sizeof rkb);
    } else
#endif
    {
        make_tables();
        for (uint64_t r = 0; r < rounds; r++) { ttable_block(a.rk, a.nr, k); ttable_block(a.rk, a.nr, k + 16); }
    }
    sha256(k, 32, out);
    aes_wipe(&a);
    sm_wipe(k, sizeof k);
    return 0;
}

/* reference (vcrypto.c) to compare in tests */
int kdbx_aes_kdf_ref(const uint8_t comp[32], const uint8_t seed[32], uint64_t rounds, uint8_t out[32]) {
    Aes a;
    if (aes_init(&a, seed, 256)) return -1;
    uint8_t k[32];
    memcpy(k, comp, 32);
    for (uint64_t r = 0; r < rounds; r++) { aes_encrypt(&a, k, k); aes_encrypt(&a, k + 16, k + 16); }
    sha256(k, 32, out);
    aes_wipe(&a);
    sm_wipe(k, sizeof k);
    return 0;
}
int kdbx_aes_kdf(const uint8_t comp[32], const uint8_t seed[32], uint64_t rounds, uint8_t out[32]) { return aes_kdf(comp, seed, rounds, out); }

static int transform(const Hdr *h, const uint8_t comp[32], uint8_t out[32], ImpList *l) {
    if (h->major == 3) {
        if (h->rounds > kdbx_max_aes_rounds) return fail(l, "too-big");
        return aes_kdf(comp, h->tseed, h->rounds, out);
    }
    Kdf k;
    if (read_kdf(h->kdf, h->kdflen, &k) || !k.have_uuid) return fail(l, "corrupt");
    if (!memcmp(k.uuid, U_AESKDF, 16) || !memcmp(k.uuid, U_AESKDF4, 16)) {
        if (!k.salt || k.saltlen != 32) return fail(l, "corrupt");
        if (k.rounds > kdbx_max_aes_rounds) return fail(l, "too-big");
        return aes_kdf(comp, k.salt, k.rounds, out);
    }
    int alg = !memcmp(k.uuid, U_ARGON2D, 16) ? CRYPTO_ARGON2_D : !memcmp(k.uuid, U_ARGON2ID, 16) ? CRYPTO_ARGON2_ID : -1;
    if (alg < 0) return fail(l, "unsupported-kdf");
    if (k.ver != 0x13) return fail(l, "unsupported-kdf");      /* Monocypher: only Argon2 1.3 */
    if (!k.salt || k.saltlen < 8 || k.saltlen > 0xffffffffu || k.par < 1 || k.par > 0xffffff || k.iter < 1 || k.iter > 0xffffffffu)
        return fail(l, "corrupt");
    uint64_t kib = k.mem / 1024;
    if (kib > kdbx_max_argon_kib || k.iter * kib > kdbx_max_argon_kib * 64) return fail(l, "too-big");
    if (kib < 8ull * k.par) return fail(l, "corrupt");
    void *work = sm_alloc((size_t)kib * 1024);
    if (!work) return fail(l, "memory");
    crypto_argon2_config cfg = { (uint32_t)alg, (uint32_t)kib, (uint32_t)k.iter, k.par };
    crypto_argon2_inputs in = { comp, k.salt, 32, (uint32_t)k.saltlen };
    crypto_argon2_extras ex = { k.key, k.ad, (uint32_t)k.keylen, (uint32_t)k.adlen };
    crypto_argon2(out, 32, work, cfg, in, ex);
    sm_free(work);
    return 0;
}

static int composite(const char *password, const uint8_t *kf, size_t kflen, uint8_t out[32], ImpList *l) {
    Sha256 s;
    uint8_t h[32];
    int any = 0;
    sha256_init(&s);
    if (password && (password[0] || !kf)) { sha256(password, strlen(password), h); sha256_update(&s, h, 32); any = 1; }
    if (kf) {
        if (kdbx_keyfile_key(kf, kflen, h)) { sm_wipe(&s, sizeof s); return fail(l, "bad-keyfile"); }
        sha256_update(&s, h, 32);
        any = 1;
    }
    sm_wipe(h, sizeof h);
    if (!any) { sm_wipe(&s, sizeof s); return fail(l, "bad-password"); }
    sha256_final(&s, out);
    return 0;
}

/* ---- body decryption ---- */

static uint8_t *decrypt(const Hdr *h, const uint8_t key[32], const uint8_t *ct, size_t n, size_t *outlen, int *badpad) {
    *badpad = 0;
    uint8_t *out = sm_alloc(n ? n : 1);
    if (!out) return NULL;
    if (!memcmp(h->cipher, U_CHACHA, 16)) {
        crypto_chacha20_ietf(out, ct, n, key, h->iv, 0);
        *outlen = n;
        return out;
    }
    Aes a;
    uint8_t iv[16];
    memcpy(iv, h->iv, 16);
    aes_init(&a, key, 256);
    if (n % 16 || aes_cbc_decrypt(&a, iv, ct, out, n)) { aes_wipe(&a); sm_free(out); *badpad = 1; return NULL; }
    aes_wipe(&a);
    long m = pkcs7_unpad(out, n);
    if (m < 0) { sm_free(out); *badpad = 1; return NULL; }
    *outlen = (size_t)m;
    return out;
}

/* ---- KeePass XML ---- */

typedef struct { const uint8_t *data; size_t len; uint8_t *own; } Bin;
typedef struct {
    ImpList *l;
    Bin *bins; int nbins;
    int v4;
    char recycle[64];
} Walk;

static int64_t kp_time(const char *t) {
    if (!t || !*t) return 0;
    if (strchr(t, '-')) return imp_parse_iso(t);
    size_t n = 0;
    uint8_t *b = b64_decode(t, strlen(t), &n);
    int64_t r = 0;
    if (b && n == 8) {
        uint64_t v = rd64(b);
        r = (int64_t)v - 62135596800ll;
        if (r < 0) r = 0;
    }
    sm_free(b);
    return r;
}

static int add_str(char ***arr, int *n, const char *s) {
    char **a = sm_realloc(*arr, sizeof(char *) * (size_t)(*n + 1));
    if (!a) return -1;
    *arr = a;
    if (!(a[*n] = sm_strdup(s))) return -1;
    (*n)++;
    return 0;
}

static int add_field(ImpEntry *e, const char *k, const char *v, int type) {
    ImpField *f = sm_realloc(e->fields, sizeof(ImpField) * (size_t)(e->nfields + 1));
    if (!f) return -1;
    e->fields = f;
    f[e->nfields].key = sm_strdup(k);
    f[e->nfields].value = sm_strdup(v ? v : "");
    f[e->nfields].type = type;
    if (!f[e->nfields].key || !f[e->nfields].value) return -1;
    e->nfields++;
    return 0;
}

static int is_true(const char *s) { return s && (!strcasecmp(s, "true") || !strcmp(s, "1")); }

static int walk_entry(Walk *w, const XNode *en, const char *group) {
    ImpEntry *e = imp_add(w->l);
    if (!e) return -1;
    e->kind = sm_strdup("login");
    if (group) e->group = sm_strdup(group);
    const char *totp_seed = NULL, *totp_set = NULL;
    const char *to_secret = NULL, *to_len = NULL, *to_period = NULL, *to_alg = NULL;
    for (int i = 0; i < en->nkids; i++) {
        const XNode *c = en->kids[i];
        if (!strcmp(c->name, "String")) {
            const char *k = xml_text(xml_child(c, "Key"));
            XNode *vn = xml_child(c, "Value");
            const char *v = xml_text(vn);
            int prot = is_true(xml_attr(vn, "Protected")) || is_true(xml_attr(vn, "ProtectInMemory"));
            char **dst = !strcmp(k, "Title") ? &e->title : !strcmp(k, "UserName") ? &e->username : !strcmp(k, "Password") ? &e->password
                       : !strcmp(k, "URL") ? &e->url : !strcmp(k, "Notes") ? &e->notes : !strcmp(k, "otp") ? &e->otp : NULL;
            if (dst) { sm_free(*dst); *dst = sm_strdup(v); if (!*dst) return -1; continue; }
            if (!strcmp(k, "TOTP Seed")) { totp_seed = v; continue; }
            if (!strcmp(k, "TOTP Settings")) { totp_set = v; continue; }
            if (!strcmp(k, "TimeOtp-Secret-Base32")) { to_secret = v; continue; }
            if (!strcmp(k, "TimeOtp-Length")) { to_len = v; continue; }
            if (!strcmp(k, "TimeOtp-Period")) { to_period = v; continue; }
            if (!strcmp(k, "TimeOtp-Algorithm")) { to_alg = v; continue; }
            if (!strncmp(k, "KP2A_URL", 8) && *v) { if (add_str(&e->urls, &e->nurls, v)) return -1; continue; }
            if (!*k) continue;
            if (add_field(e, k, v, prot ? IMPF_HIDDEN : IMPF_TEXT)) return -1;
        } else if (!strcmp(c->name, "Tags")) {
            const char *t = xml_text(c);
            while (*t) {
                while (*t == ' ' || *t == ';' || *t == ',') t++;
                const char *a = t;
                while (*t && *t != ';' && *t != ',') t++;
                const char *b = t;
                while (b > a && b[-1] == ' ') b--;
                if (b > a) {
                    char *tag = sm_strndup(a, (size_t)(b - a));
                    if (!tag || add_str(&e->tags, &e->ntags, tag)) { sm_free(tag); return -1; }
                    sm_free(tag);
                }
            }
        } else if (!strcmp(c->name, "Times")) {
            e->created = kp_time(xml_text(xml_child(c, "CreationTime")));
            e->modified = kp_time(xml_text(xml_child(c, "LastModificationTime")));
        } else if (!strcmp(c->name, "Binary")) {
            const char *name = xml_text(xml_child(c, "Key"));
            XNode *vn = xml_child(c, "Value");
            const char *ref = xml_attr(vn, "Ref");
            const uint8_t *data = NULL;
            size_t dlen = 0;
            uint8_t *own = NULL;
            if (ref) {
                char *endp;
                long r = strtol(ref, &endp, 10);
                if (*endp || r < 0 || r >= w->nbins) continue;
                data = w->bins[r].data;
                dlen = w->bins[r].len;
            } else if (vn && vn->tlen) {
                own = b64_decode(vn->text, vn->tlen, &dlen);
                data = own;
            }
            if (!data && dlen) continue;
            ImpAttach *a = sm_realloc(e->atts, sizeof(ImpAttach) * (size_t)(e->natts + 1));
            if (!a) { sm_free(own); return -1; }
            e->atts = a;
            a[e->natts].name = sm_strdup(*name ? name : "adjunto");
            a[e->natts].data = sm_alloc(dlen ? dlen : 1);
            a[e->natts].len = dlen;
            if (!a[e->natts].name || !a[e->natts].data) { sm_free(own); return -1; }
            if (dlen) memcpy(a[e->natts].data, data, dlen);
            e->natts++;
            sm_free(own);
        }
    }
    /* TOTP in old KeePassXC formats and in KeePass 2.47+ → otpauth URI */
    if (!e->otp && (totp_seed || to_secret)) {
        VjB b;
        vb_init(&b);
        vb_raw(&b, "otpauth://totp/?secret=");
        const char *sec = totp_seed ? totp_seed : to_secret;
        for (const char *p = sec; *p; p++) if (*p != ' ') { char ch[2] = { *p, 0 }; vb_raw(&b, ch); }
        int period = 30, digits = 6, steam = 0;
        if (totp_seed && totp_set) {
            sscanf(totp_set, "%d", &period);
            const char *semi = strchr(totp_set, ';');
            if (semi) { if (semi[1] == 'S') steam = 1; else sscanf(semi + 1, "%d", &digits); }
        }
        if (to_period) sscanf(to_period, "%d", &period);
        if (to_len) sscanf(to_len, "%d", &digits);
        char tail[96];
        snprintf(tail, sizeof tail, "&period=%d&digits=%d", period > 0 && period < 100000 ? period : 30, digits >= 4 && digits <= 10 ? digits : 6);
        vb_raw(&b, tail);
        if (steam) vb_raw(&b, "&encoder=steam");
        if (to_alg && strstr(to_alg, "256")) vb_raw(&b, "&algorithm=SHA256");
        else if (to_alg && strstr(to_alg, "512")) vb_raw(&b, "&algorithm=SHA512");
        if (b.oom) { vb_free(&b); return -1; }
        e->otp = sm_strdup(b.p);
        vb_free(&b);
    }
    return 0;
}

static int walk_group(Walk *w, const XNode *g, const char *path, int depth) {
    if (depth > 200) return 0;
    for (int i = 0; i < g->nkids; i++) {
        const XNode *c = g->kids[i];
        if (!strcmp(c->name, "Entry")) { if (walk_entry(w, c, path)) return -1; }
        else if (!strcmp(c->name, "Group")) {
            const char *uuid = xml_text(xml_child(c, "UUID"));
            if (w->recycle[0] && !strcmp(uuid, w->recycle)) continue;
            const char *name = xml_text(xml_child(c, "Name"));
            size_t pl = path ? strlen(path) : 0, nl = strlen(name);
            char *np = sm_alloc(pl + nl + 2 + (nl ? 0 : 1));
            if (!np) return -1;
            if (path) { memcpy(np, path, pl); np[pl++] = '/'; }
            for (size_t k = 0; k < nl; k++) np[pl + k] = name[k] == '/' ? '-' : name[k];
            if (!nl) np[pl++] = '-';
            np[pl + nl] = 0;
            int r = add_str(&w->l->groups, &w->l->ngroups, np);
            if (!r) r = walk_group(w, c, np, depth + 1);
            sm_free(np);
            if (r) return -1;
        }
        /* History, Times, Name…: not entries */
    }
    return 0;
}

/* Decrypts, in document order, all Protected="True" values. */
static int unprotect(XNode *n, Stream *s) {
    if (is_true(xml_attr(n, "Protected"))) {
        size_t m = 0;
        uint8_t *b = b64_decode(n->text ? n->text : "", n->tlen, &m);
        if (!b) return -1;
        stream_xor(s, b, m);
        char *t = sm_alloc(m + 1);
        if (!t) { sm_free(b); return -1; }
        memcpy(t, b, m);
        t[m] = 0;
        sm_free(b);
        xml_set_text(n, t, strlen(t));        /* up to an embedded NUL: as text */
    }
    for (int i = 0; i < n->nkids; i++) if (unprotect(n->kids[i], s)) return -1;
    return 0;
}

static int parse_xml(const uint8_t *xml, size_t xlen, Stream *s, Bin *bins, int nbins, int v4,
                     const uint8_t *hdr, size_t hdrlen, ImpList *l) {
    const char *err = NULL;
    XNode *root = xml_parse((const char *)xml, xlen, &err);
    if (!root) return fail(l, err && !strcmp(err, "memory") ? "memory" : "corrupt");
    if (strcmp(root->name, "KeePassFile")) { xml_free(root); return fail(l, "corrupt"); }
    if (unprotect(root, s)) { xml_free(root); return fail(l, "corrupt"); }
    XNode *meta = xml_child(root, "Meta");
    Walk w = { l, bins, nbins, v4, "" };
    Bin *v3bins = NULL;
    int rc = 0;
    if (meta) {
        const char *rb = xml_text(xml_child(meta, "RecycleBinUUID"));
        if (strcmp(rb, "AAAAAAAAAAAAAAAAAAAAAA==")) snprintf(w.recycle, sizeof w.recycle, "%s", rb);
        /* KDBX 3.1: header hash inside the XML */
        const char *hh = xml_text(xml_child(meta, "HeaderHash"));
        if (!v4 && *hh) {
            uint8_t want[32];
            size_t m = 0;
            uint8_t *b = b64_decode(hh, strlen(hh), &m);
            sha256(hdr, hdrlen, want);
            int ok = b && m == 32 && vc_equal(b, want, 32);
            sm_free(b);
            if (!ok) { xml_free(root); return fail(l, "corrupt"); }
        }
        /* KDBX 3.1: attachments in Meta/Binaries (ID, Compressed) */
        XNode *bs = xml_child(meta, "Binaries");
        if (!v4 && bs && bs->nkids) {
            int maxid = -1;
            for (int i = 0; i < bs->nkids; i++) { const char *id = xml_attr(bs->kids[i], "ID"); int v = id ? atoi(id) : -1; if (v > maxid && v < 100000) maxid = v; }
            if (maxid >= 0) {
                v3bins = sm_alloc(sizeof(Bin) * (size_t)(maxid + 1));
                if (!v3bins) { xml_free(root); return fail(l, "memory"); }
                for (int i = 0; i < bs->nkids; i++) {
                    XNode *b = bs->kids[i];
                    const char *id = xml_attr(b, "ID");
                    int v = id ? atoi(id) : -1;
                    if (v < 0 || v > maxid || v3bins[v].own) continue;
                    size_t m = 0;
                    uint8_t *raw = b64_decode(b->text ? b->text : "", b->tlen, &m);
                    if (!raw) continue;
                    if (is_true(xml_attr(b, "Compressed"))) {
                        size_t z = 0;
                        uint8_t *u = vstb_gunzip(raw, m, MAX_PAYLOAD, &z);
                        sm_free(raw);
                        if (!u) continue;
                        raw = u; m = z;
                    }
                    v3bins[v].own = raw; v3bins[v].data = raw; v3bins[v].len = m;
                }
                w.bins = v3bins;
                w.nbins = maxid + 1;
            }
        }
    }
    XNode *top = xml_child(xml_child(root, "Root"), "Group");
    if (!top) rc = fail(l, "corrupt");
    else if (walk_group(&w, top, NULL, 0)) rc = fail(l, "memory");
    if (v3bins) { for (int i = 0; i < w.nbins; i++) sm_free(v3bins[i].own); sm_free(v3bins); }
    xml_free(root);
    return rc;
}

/* ---- full read ---- */

static int read_v4(const uint8_t *d, size_t len, const Hdr *h, const uint8_t comp[32], ImpList *l) {
    size_t p = h->hdr_end;
    if (p + 64 > len) return fail(l, "truncated");
    uint8_t x[64];
    sha256(d, p, x);
    if (!vc_equal(x, d + p, 32)) return fail(l, "corrupt");
    uint8_t tk[32], hk[64], mk[32], bk[64], idx[8];
    if (transform(h, comp, tk, l)) return -1;
    Sha512 s5;
    uint8_t one = 1;
    sha512_init(&s5); sha512_update(&s5, h->seed, 32); sha512_update(&s5, tk, 32); sha512_update(&s5, &one, 1); sha512_final(&s5, hk);
    Sha256 s2;
    sha256_init(&s2); sha256_update(&s2, h->seed, 32); sha256_update(&s2, tk, 32); sha256_final(&s2, mk);
    sm_wipe(tk, sizeof tk);
    wr64(idx, ~0ull);
    sha512_init(&s5); sha512_update(&s5, idx, 8); sha512_update(&s5, hk, 64); sha512_final(&s5, bk);
    hmac_sha256(bk, 64, d, h->hdr_end, x);
    int rc = -1;
    uint8_t *ct = NULL, *pt = NULL, *plain = NULL;
    size_t ctn = 0, ctcap = 0;
    if (!vc_equal(x, d + p + 32, 32)) { fail(l, "bad-password"); goto out; }
    p += 64;
    for (uint64_t i = 0;; i++) {
        if (p + 36 > len) { fail(l, "truncated"); goto out; }
        const uint8_t *mac = d + p;
        uint32_t sz = rd32(d + p + 32);
        p += 36;
        if (sz > len - p) { fail(l, "truncated"); goto out; }
        wr64(idx, i);
        sha512_init(&s5); sha512_update(&s5, idx, 8); sha512_update(&s5, hk, 64); sha512_final(&s5, bk);
        const uint8_t *parts[4] = { idx, d + p - 4, d + p, NULL };
        size_t lens[4] = { 8, 4, sz, 0 };
        hmac_sha256_v(bk, 64, parts, lens, x);
        if (!vc_equal(x, mac, 32)) { fail(l, "corrupt"); goto out; }
        if (!sz) break;
        if (ctn + sz > MAX_PAYLOAD) { fail(l, "too-big"); goto out; }
        if (ctn + sz > ctcap) {
            size_t nc = ctcap ? ctcap : 65536;
            while (nc < ctn + sz) nc *= 2;
            uint8_t *nb = sm_realloc(ct, nc);
            if (!nb) { fail(l, "memory"); goto out; }
            ct = nb; ctcap = nc;
        }
        memcpy(ct + ctn, d + p, sz);
        ctn += sz;
        p += sz;
    }
    int badpad;
    size_t ptn = 0;
    pt = decrypt(h, mk, ct ? ct : (const uint8_t *)"", ctn, &ptn, &badpad);
    if (!pt) { fail(l, badpad ? "corrupt" : "memory"); goto out; }
    const uint8_t *body = pt;
    size_t bn = ptn;
    if (h->comp == 1) {
        plain = vstb_gunzip(pt, ptn, MAX_PAYLOAD, &bn);
        if (!plain) { fail(l, "corrupt"); goto out; }
        body = plain;
    }
    /* inner header */
    Bin *bins = NULL;
    int nbins = 0;
    uint32_t sid = 0;
    const uint8_t *skey = NULL;
    size_t sklen = 0, q = 0;
    for (;;) {
        if (q + 5 > bn) { fail(l, "corrupt"); goto bins_out; }
        uint8_t id = body[q];
        uint32_t sz = rd32(body + q + 1);
        q += 5;
        if (sz > bn - q) { fail(l, "corrupt"); goto bins_out; }
        if (id == 0) { q += sz; break; }
        if (id == 1 && sz == 4) sid = rd32(body + q);
        else if (id == 2) { skey = body + q; sklen = sz; }
        else if (id == 3 && sz >= 1) {
            if (nbins >= 100000) { fail(l, "too-big"); goto bins_out; }
            Bin *nb = sm_realloc(bins, sizeof(Bin) * (size_t)(nbins + 1));
            if (!nb) { fail(l, "memory"); goto bins_out; }
            bins = nb;
            bins[nbins].data = body + q + 1;
            bins[nbins].len = sz - 1;
            bins[nbins].own = NULL;
            nbins++;
        }
        q += sz;
    }
    Stream st;
    if (!skey && sid) { fail(l, "corrupt"); goto bins_out; }
    if (stream_init(&st, sid, skey, sklen)) { fail(l, "unsupported-cipher"); goto bins_out; }
    rc = parse_xml(body + q, bn - q, &st, bins, nbins, 1, d, h->hdr_end, l);
    sm_wipe(&st, sizeof st);
bins_out:
    sm_free(bins);
out:
    sm_wipe(hk, sizeof hk); sm_wipe(mk, sizeof mk); sm_wipe(bk, sizeof bk); sm_wipe(&s5, sizeof s5); sm_wipe(&s2, sizeof s2);
    sm_free(ct); sm_free(pt); sm_free(plain);
    return rc;
}

static int read_v3(const uint8_t *d, size_t len, const Hdr *h, const uint8_t comp[32], ImpList *l) {
    uint8_t tk[32], mk[32];
    if (transform(h, comp, tk, l)) return -1;
    Sha256 s2;
    sha256_init(&s2); sha256_update(&s2, h->seed, 32); sha256_update(&s2, tk, 32); sha256_final(&s2, mk);
    sm_wipe(tk, sizeof tk);
    int rc = -1, badpad;
    size_t ptn = 0;
    uint8_t *pt = NULL, *blocks = NULL, *plain = NULL;
    if (h->hdr_end >= len) { fail(l, "truncated"); goto out; }
    pt = decrypt(h, mk, d + h->hdr_end, len - h->hdr_end, &ptn, &badpad);
    if (!pt) { fail(l, badpad ? ((len - h->hdr_end) % 16 ? "truncated" : "bad-password") : "memory"); goto out; }
    if (ptn < 32 || !vc_equal(pt, h->start, 32)) { fail(l, "bad-password"); goto out; }
    /* hashed blocks: u32 index, SHA-256, u32 length, data */
    size_t p = 32, bn = 0;
    blocks = sm_alloc(ptn);
    if (!blocks) { fail(l, "memory"); goto out; }
    for (uint32_t i = 0;; i++) {
        if (p + 40 > ptn) { fail(l, "truncated"); goto out; }
        uint32_t id = rd32(pt + p), sz = rd32(pt + p + 36);
        const uint8_t *hash = pt + p + 4;
        p += 40;
        if (id != i || sz > ptn - p) { fail(l, "corrupt"); goto out; }
        if (!sz) {
            uint8_t z = 0;
            for (int k = 0; k < 32; k++) z |= hash[k];
            if (z) { fail(l, "corrupt"); goto out; }
            break;
        }
        uint8_t x[32];
        sha256(pt + p, sz, x);
        if (!vc_equal(x, hash, 32)) { fail(l, "corrupt"); goto out; }
        memcpy(blocks + bn, pt + p, sz);
        bn += sz;
        p += sz;
    }
    const uint8_t *body = blocks;
    if (h->comp == 1) {
        size_t z = 0;
        plain = vstb_gunzip(blocks, bn, MAX_PAYLOAD, &z);
        if (!plain) { fail(l, "corrupt"); goto out; }
        body = plain;
        bn = z;
    }
    Stream st;
    if (stream_init(&st, h->inner_id, h->pskey, h->pskeylen)) { fail(l, "unsupported-cipher"); goto out; }
    rc = parse_xml(body, bn, &st, NULL, 0, 0, d, h->hdr_end, l);
    sm_wipe(&st, sizeof st);
out:
    sm_wipe(mk, sizeof mk); sm_wipe(&s2, sizeof s2);
    sm_free(pt); sm_free(blocks); sm_free(plain);
    return rc;
}

int kdbx_parse(const uint8_t *d, size_t len, const char *password, const uint8_t *keyfile, size_t kflen, ImpList *l) {
    Hdr h;
    if (read_header(d, len, &h, l)) return -1;
    uint8_t comp[32];
    if (composite(password, keyfile, kflen, comp, l)) return -1;
    int rc = h.major == 4 ? read_v4(d, len, &h, comp, l) : read_v3(d, len, &h, comp, l);
    sm_wipe(comp, sizeof comp);
    return rc;
}
