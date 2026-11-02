/* p256.c — our own NIST P-256 (see p256.h). */
#include "p256.h"
#include "vcrypto.h"

#include <string.h>

typedef struct { uint32_t v[8]; } Fe;          /* little-endian words */

typedef struct { uint32_t m[8]; uint32_t minv; Fe r2, one; } Mod;

static const uint32_t P_WORDS[8] = { 0xffffffff, 0xffffffff, 0xffffffff, 0x00000000, 0x00000000, 0x00000000, 0x00000001, 0xffffffff };
static const uint32_t N_WORDS[8] = { 0xfc632551, 0xf3b9cac2, 0xa7179e84, 0xbce6faad, 0xffffffff, 0xffffffff, 0x00000000, 0xffffffff };
static const uint8_t B_BYTES[32] = {
    0x5a,0xc6,0x35,0xd8,0xaa,0x3a,0x93,0xe7,0xb3,0xeb,0xbd,0x55,0x76,0x98,0x86,0xbc,
    0x65,0x1d,0x06,0xb0,0xcc,0x53,0xb0,0xf6,0x3b,0xce,0x3c,0x3e,0x27,0xd2,0x60,0x4b };
static const uint8_t GX[32] = {
    0x6b,0x17,0xd1,0xf2,0xe1,0x2c,0x42,0x47,0xf8,0xbc,0xe6,0xe5,0x63,0xa4,0x40,0xf2,
    0x77,0x03,0x7d,0x81,0x2d,0xeb,0x33,0xa0,0xf4,0xa1,0x39,0x45,0xd8,0x98,0xc2,0x96 };
static const uint8_t GY[32] = {
    0x4f,0xe3,0x42,0xe2,0xfe,0x1a,0x7f,0x9b,0x8e,0xe7,0xeb,0x4a,0x7c,0x0f,0x9e,0x16,
    0x2b,0xce,0x33,0x57,0x6b,0x31,0x5e,0xce,0xcb,0xb6,0x40,0x68,0x37,0xbf,0x51,0xf5 };

static Mod MP;
static Fe B_M;            /* b in Montgomery form */
static int ready;

/* ---- 256-bit integers ---- */

static void from_bytes(Fe *r, const uint8_t b[32]) {
    for (int i = 0; i < 8; i++)
        r->v[i] = (uint32_t)b[31 - 4 * i] | (uint32_t)b[30 - 4 * i] << 8 | (uint32_t)b[29 - 4 * i] << 16 | (uint32_t)b[28 - 4 * i] << 24;
}
static void to_bytes(uint8_t b[32], const Fe *a) {
    for (int i = 0; i < 8; i++) {
        b[31 - 4 * i] = (uint8_t)a->v[i]; b[30 - 4 * i] = (uint8_t)(a->v[i] >> 8);
        b[29 - 4 * i] = (uint8_t)(a->v[i] >> 16); b[28 - 4 * i] = (uint8_t)(a->v[i] >> 24);
    }
}

/* r = a - b; returns the borrow (0 or 1) */
static uint32_t sub_raw(uint32_t r[8], const uint32_t a[8], const uint32_t b[8]) {
    uint64_t borrow = 0;
    for (int i = 0; i < 8; i++) {
        uint64_t d = (uint64_t)a[i] - b[i] - borrow;
        r[i] = (uint32_t)d;
        borrow = (d >> 63) & 1;
    }
    return (uint32_t)borrow;
}
static uint32_t add_raw(uint32_t r[8], const uint32_t a[8], const uint32_t b[8]) {
    uint64_t c = 0;
    for (int i = 0; i < 8; i++) { c += (uint64_t)a[i] + b[i]; r[i] = (uint32_t)c; c >>= 32; }
    return (uint32_t)c;
}
/* r = sel ? a : b, branch-free */
static void cmov(uint32_t r[8], const uint32_t a[8], const uint32_t b[8], uint32_t sel) {
    uint32_t m = (uint32_t)0 - (sel & 1);
    for (int i = 0; i < 8; i++) r[i] = (a[i] & m) | (b[i] & ~m);
}

static void mod_add(Fe *r, const Fe *a, const Fe *b, const Mod *M) {
    uint32_t s[8], t[8];
    uint32_t c = add_raw(s, a->v, b->v);
    uint32_t br = sub_raw(t, s, M->m);
    cmov(r->v, t, s, c | (br ^ 1));
}
static void mod_sub(Fe *r, const Fe *a, const Fe *b, const Mod *M) {
    uint32_t s[8], t[8];
    uint32_t br = sub_raw(s, a->v, b->v);
    add_raw(t, s, M->m);
    cmov(r->v, t, s, br);
}

/* Montgomery multiplication (CIOS): r = a*b*R^-1 mod m */
static void mont_mul(Fe *r, const Fe *a, const Fe *b, const Mod *M) {
    uint32_t t[10] = { 0 };
    for (int i = 0; i < 8; i++) {
        uint64_t c = 0;
        for (int j = 0; j < 8; j++) {
            c = (uint64_t)t[j] + (uint64_t)a->v[j] * b->v[i] + (c >> 32);
            t[j] = (uint32_t)c;
        }
        c = (uint64_t)t[8] + (c >> 32);
        t[8] = (uint32_t)c;
        t[9] = (uint32_t)(c >> 32);
        uint32_t mq = t[0] * M->minv;
        c = (uint64_t)t[0] + (uint64_t)mq * M->m[0];
        for (int j = 1; j < 8; j++) {
            c = (uint64_t)t[j] + (uint64_t)mq * M->m[j] + (c >> 32);
            t[j - 1] = (uint32_t)c;
        }
        c = (uint64_t)t[8] + (c >> 32);
        t[7] = (uint32_t)c;
        t[8] = t[9] + (uint32_t)(c >> 32);
    }
    uint32_t u[8];
    uint32_t br = sub_raw(u, t, M->m);
    cmov(r->v, u, t, t[8] | (br ^ 1));
}

static void mod_setup(Mod *M, const uint32_t m[8]) {
    memcpy(M->m, m, sizeof M->m);
    uint32_t inv = 1;
    for (int i = 0; i < 5; i++) inv *= 2 - m[0] * inv;
    M->minv = (uint32_t)0 - inv;
    Fe x = { { 1 } };
    for (int i = 0; i < 512; i++) mod_add(&x, &x, &x, M);   /* 2^512 mod m = R^2 */
    M->r2 = x;
    Fe one = { { 1 } };
    mont_mul(&M->one, &one, &M->r2, M);                     /* R mod m */
}

static void init(void) {
    if (ready) return;
    mod_setup(&MP, P_WORDS);
    Fe b;
    from_bytes(&b, B_BYTES);
    mont_mul(&B_M, &b, &MP.r2, &MP);
    ready = 1;
}

static void to_mont(Fe *r, const Fe *a) { mont_mul(r, a, &MP.r2, &MP); }
static void from_mont(Fe *r, const Fe *a) { Fe one = { { 1 } }; mont_mul(r, a, &one, &MP); }
static void fmul(Fe *r, const Fe *a, const Fe *b) { mont_mul(r, a, b, &MP); }
static void fadd(Fe *r, const Fe *a, const Fe *b) { mod_add(r, a, b, &MP); }
static void fsub(Fe *r, const Fe *a, const Fe *b) { mod_sub(r, a, b, &MP); }

/* a^(p-2) (the exponent is public) */
static void finv(Fe *r, const Fe *a) {
    uint32_t e[8];
    uint32_t two[8] = { 2 };
    sub_raw(e, P_WORDS, two);
    Fe acc = MP.one;
    for (int i = 255; i >= 0; i--) {
        fmul(&acc, &acc, &acc);
        if ((e[i / 32] >> (i % 32)) & 1) fmul(&acc, &acc, a);
    }
    *r = acc;
}

static int fe_is_zero(const Fe *a) { uint32_t z = 0; for (int i = 0; i < 8; i++) z |= a->v[i]; return z == 0; }
static int lt(const uint32_t a[8], const uint32_t m[8]) { uint32_t t[8]; return (int)sub_raw(t, a, m); }

/* ---- projective points (X:Y:Z), Montgomery ---- */

typedef struct { Fe x, y, z; } Pt;

static void pt_add(Pt *r, const Pt *p1, const Pt *p2) {
    Fe t0, t1, t2, t3, t4, x3, y3, z3;
    fmul(&t0, &p1->x, &p2->x);
    fmul(&t1, &p1->y, &p2->y);
    fmul(&t2, &p1->z, &p2->z);
    fadd(&t3, &p1->x, &p1->y);
    fadd(&t4, &p2->x, &p2->y);
    fmul(&t3, &t3, &t4);
    fadd(&t4, &t0, &t1);
    fsub(&t3, &t3, &t4);
    fadd(&t4, &p1->y, &p1->z);
    fadd(&x3, &p2->y, &p2->z);
    fmul(&t4, &t4, &x3);
    fadd(&x3, &t1, &t2);
    fsub(&t4, &t4, &x3);
    fadd(&x3, &p1->x, &p1->z);
    fadd(&y3, &p2->x, &p2->z);
    fmul(&x3, &x3, &y3);
    fadd(&y3, &t0, &t2);
    fsub(&y3, &x3, &y3);
    fmul(&z3, &B_M, &t2);
    fsub(&x3, &y3, &z3);
    fadd(&z3, &x3, &x3);
    fadd(&x3, &x3, &z3);
    fsub(&z3, &t1, &x3);
    fadd(&x3, &t1, &x3);
    fmul(&y3, &B_M, &y3);
    fadd(&t1, &t2, &t2);
    fadd(&t2, &t1, &t2);
    fsub(&y3, &y3, &t2);
    fsub(&y3, &y3, &t0);
    fadd(&t1, &y3, &y3);
    fadd(&y3, &t1, &y3);
    fadd(&t1, &t0, &t0);
    fadd(&t0, &t1, &t0);
    fsub(&t0, &t0, &t2);
    fmul(&t1, &t4, &y3);
    fmul(&t2, &t0, &y3);
    fmul(&y3, &x3, &z3);
    fadd(&y3, &y3, &t2);
    fmul(&x3, &t3, &x3);
    fsub(&x3, &x3, &t1);
    fmul(&z3, &t4, &z3);
    fmul(&t1, &t3, &t0);
    fadd(&z3, &z3, &t1);
    r->x = x3; r->y = y3; r->z = z3;
}

static void pt_cswap(Pt *a, Pt *b, uint32_t bit) {
    uint32_t m = (uint32_t)0 - (bit & 1);
    uint32_t *pa = (uint32_t *)a, *pb = (uint32_t *)b;
    for (size_t i = 0; i < sizeof(Pt) / 4; i++) { uint32_t t = (pa[i] ^ pb[i]) & m; pa[i] ^= t; pb[i] ^= t; }
}

/* r = k * p (k big-endian, 256 bits), Montgomery ladder */
static void pt_mul(Pt *r, const uint8_t k[32], const Pt *p) {
    Pt r0, r1 = *p;
    memset(&r0, 0, sizeof r0);
    r0.y = MP.one;                                  /* infinity (0:1:0) */
    for (int i = 255; i >= 0; i--) {
        uint32_t bit = (k[31 - i / 8] >> (i % 8)) & 1;
        pt_cswap(&r0, &r1, bit);
        pt_add(&r1, &r0, &r1);
        pt_add(&r0, &r0, &r0);
        pt_cswap(&r0, &r1, bit);
    }
    *r = r0;
    explicit_bzero(&r0, sizeof r0);
    explicit_bzero(&r1, sizeof r1);
}

static int pt_affine(const Pt *p, uint8_t x[32], uint8_t y[32]) {
    if (fe_is_zero(&p->z)) return -1;
    Fe zi, ax, ay;
    finv(&zi, &p->z);
    fmul(&ax, &p->x, &zi);
    fmul(&ay, &p->y, &zi);
    from_mont(&ax, &ax);
    from_mont(&ay, &ay);
    if (x) to_bytes(x, &ax);
    if (y) to_bytes(y, &ay);
    return 0;
}

/* (x, y) in affine form (bytes) → validated point */
static int pt_load(Pt *r, const uint8_t xb[32], const uint8_t yb[32]) {
    Fe x, y;
    from_bytes(&x, xb);
    from_bytes(&y, yb);
    if (!lt(x.v, P_WORDS) || !lt(y.v, P_WORDS)) return -1;
    Fe xm, ym, l, rr, t;
    to_mont(&xm, &x);
    to_mont(&ym, &y);
    fmul(&l, &ym, &ym);                 /* y^2 */
    fmul(&t, &xm, &xm);
    fmul(&rr, &t, &xm);                 /* x^3 */
    fadd(&t, &xm, &xm);
    fadd(&t, &t, &xm);                  /* 3x */
    fsub(&rr, &rr, &t);
    fadd(&rr, &rr, &B_M);               /* x^3 - 3x + b */
    uint32_t d = 0;
    for (int i = 0; i < 8; i++) d |= l.v[i] ^ rr.v[i];
    if (d) return -1;
    r->x = xm; r->y = ym; r->z = MP.one;
    return 0;
}

static int scalar_ok(const uint8_t k[32]) {
    Fe s;
    from_bytes(&s, k);
    return !fe_is_zero(&s) && lt(s.v, N_WORDS);
}

int p256_on_curve(const uint8_t x[32], const uint8_t y[32]) {
    init();
    Pt p;
    return pt_load(&p, x, y) == 0;
}

int p256_mul_base(const uint8_t k[32], uint8_t x[32], uint8_t y[32]) {
    init();
    Pt g, r;
    if (pt_load(&g, GX, GY)) return -1;
    pt_mul(&r, k, &g);
    return pt_affine(&r, x, y);
}

int p256_public(const uint8_t priv[32], uint8_t x[32], uint8_t y[32]) {
    if (!scalar_ok(priv)) return -1;
    return p256_mul_base(priv, x, y);
}

int p256_keypair(uint8_t priv[32], uint8_t x[32], uint8_t y[32]) {
    for (int tries = 0; tries < 64; tries++) {
        if (vc_random(priv, 32)) return -1;
        if (scalar_ok(priv)) return p256_public(priv, x, y);
    }
    return -1;
}

int p256_ecdh(const uint8_t priv[32], const uint8_t px[32], const uint8_t py[32], uint8_t shared_x[32]) {
    init();
    if (!scalar_ok(priv)) return -1;
    Pt q, r;
    if (pt_load(&q, px, py)) return -1;
    pt_mul(&r, priv, &q);
    int rc = pt_affine(&r, shared_x, NULL);
    explicit_bzero(&r, sizeof r);
    return rc;
}
