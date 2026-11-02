/* qr.c — our own QR codes: encode and decode (see qr.h). ISO/IEC 18004.
 *
 * Encode: picks the most compact mode (numeric, alphanumeric or byte) and the smallest version
 * that fits with the requested correction level; Reed-Solomon over GF(256) (polynomial 0x11d),
 * interleaved blocks, function patterns, format and version information (BCH) and the 8 masks
 * with the standard's penalty.
 *
 * Decode: luminance → binarization (global Otsu and Bradley local threshold, normal and inverted) →
 * finder patterns by the 1:1:3:1:1 ratio in rows, confirmed in columns → the three
 * best (right triangle) → side from the timing pattern and the version
 * information → grid (affine, or perspective with the alignment pattern) → format, mask,
 * codewords, Reed-Solomon (Berlekamp-Massey + Chien + Forney) → segments. */
#include "qr.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>

/* ================= tables ================= */

static const int8_t ECC_PER_BLOCK[4][41] = {
    { -1, 7,10,15,20,26,18,20,24,30,18,20,24,26,30,22,24,28,30,28,28,28,28,30,30,26,28,30,30,30,30,30,30,30,30,30,30,30,30,30,30 },
    { -1,10,16,26,18,24,16,18,22,22,26,30,22,22,24,24,28,28,26,26,26,26,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28 },
    { -1,13,22,18,26,18,24,18,22,20,24,28,26,24,20,30,24,28,28,26,30,28,30,30,30,30,28,30,30,30,30,30,30,30,30,30,30,30,30,30,30 },
    { -1,17,28,22,16,22,28,26,26,24,28,24,28,22,24,24,30,28,28,26,28,30,24,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30 },
};
static const int8_t NUM_BLOCKS[4][41] = {
    { -1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9,10,12,12,12,13,14,15,16,17,18,19,19,20,21,22,24,25 },
    { -1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9,10,10,11,13,14,16,17,17,18,20,21,23,25,26,28,29,31,33,35,37,38,40,43,45,47,49 },
    { -1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8,10,12,16,12,17,16,18,21,20,23,23,25,27,29,34,34,35,38,40,43,45,48,51,53,56,59,62,65,68 },
    { -1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8,11,11,16,16,18,16,19,21,25,25,25,34,30,32,35,37,40,42,45,48,51,54,57,60,63,66,70,74,77,81 },
};
static const int FORMAT_ECC_BITS[4] = { 1, 0, 3, 2 };     /* L M Q H in the format information */
static const char ALNUM[] = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

static int raw_modules(int ver) {
    int r = (16 * ver + 128) * ver + 64;
    if (ver >= 2) {
        int na = ver / 7 + 2;
        r -= (25 * na - 10) * na - 55;
        if (ver >= 7) r -= 36;
    }
    return r;
}
static int data_codewords(int ver, int ecc) { return raw_modules(ver) / 8 - ECC_PER_BLOCK[ecc][ver] * NUM_BLOCKS[ecc][ver]; }

static int align_pos(int ver, int *pos) {
    if (ver == 1) return 0;
    int na = ver / 7 + 2, size = ver * 4 + 17;
    int step = ver == 32 ? 26 : (ver * 4 + na * 2 + 1) / (na * 2 - 2) * 2;
    pos[0] = 6;
    for (int i = 0, p = size - 7; i < na - 1; i++, p -= step) pos[na - 1 - i] = p;
    return na;
}

static int cc_bits(int mode, int ver) {
    int g = ver <= 9 ? 0 : ver <= 26 ? 1 : 2;
    static const int t[3][3] = { { 10, 12, 14 }, { 9, 11, 13 }, { 8, 16, 16 } };
    return t[mode == 1 ? 0 : mode == 2 ? 1 : 2][g];
}

/* ================= GF(256) and Reed-Solomon ================= */

static uint8_t GEXP[512], GLOG[256];
static int gf_ready;
static void gf_init(void) {
    if (gf_ready) return;
    int x = 1;
    for (int i = 0; i < 255; i++) { GEXP[i] = (uint8_t)x; GLOG[x] = (uint8_t)i; x <<= 1; if (x & 0x100) x ^= 0x11d; }
    for (int i = 255; i < 512; i++) GEXP[i] = GEXP[i - 255];
    gf_ready = 1;
}
static uint8_t gmul(uint8_t a, uint8_t b) { return a && b ? GEXP[GLOG[a] + GLOG[b]] : 0; }
static uint8_t gdiv(uint8_t a, uint8_t b) { return a ? GEXP[(GLOG[a] + 255 - GLOG[b]) % 255] : 0; }

/* Generator of degree d: (x - a^0)(x - a^1)…; coefficients without the leading one (1). */
static void rs_gen(int d, uint8_t *g) {
    memset(g, 0, (size_t)d);
    g[d - 1] = 1;
    uint8_t root = 1;
    for (int i = 0; i < d; i++) {
        for (int j = 0; j < d; j++) { g[j] = gmul(g[j], root); if (j + 1 < d) g[j] ^= g[j + 1]; }
        root = gmul(root, 2);
    }
}
static void rs_rem(const uint8_t *data, int n, const uint8_t *g, int d, uint8_t *out) {
    memset(out, 0, (size_t)d);
    for (int i = 0; i < n; i++) {
        uint8_t f = data[i] ^ out[0];
        memmove(out, out + 1, (size_t)d - 1);
        out[d - 1] = 0;
        for (int j = 0; j < d; j++) out[j] ^= gmul(g[j], f);
    }
}

/* Corrects block c of n bytes with ne correction bytes. 0 = ok (or no errors), -1 if it cannot. */
static int rs_correct(uint8_t *c, int n, int ne) {
    uint8_t S[64], L[64], B[64], T[64], O[64];
    int any = 0;
    for (int j = 0; j < ne; j++) {
        uint8_t s = 0, a = GEXP[j];
        for (int k = 0; k < n; k++) s = gmul(s, a) ^ c[k];
        S[j] = s;
        any |= s;
    }
    if (!any) return 0;
    /* Berlekamp-Massey */
    memset(L, 0, sizeof L); memset(B, 0, sizeof B);
    L[0] = B[0] = 1;
    int Ld = 0, m = 1;
    uint8_t b = 1;
    for (int r = 0; r < ne; r++) {
        uint8_t d = S[r];
        for (int i = 1; i <= Ld; i++) d ^= gmul(L[i], S[r - i]);
        if (!d) { m++; continue; }
        memcpy(T, L, sizeof T);
        uint8_t coef = gdiv(d, b);
        for (int i = 0; i + m < 64; i++) L[i + m] ^= gmul(coef, B[i]);
        if (2 * Ld <= r) { Ld = r + 1 - Ld; memcpy(B, T, sizeof B); b = d; m = 1; }
        else m++;
    }
    if (Ld > ne / 2) return -1;
    /* Omega = S * Lambda mod x^ne */
    for (int i = 0; i < ne; i++) { uint8_t v = 0; for (int j = 0; j <= i && j <= Ld; j++) v ^= gmul(L[j], S[i - j]); O[i] = v; }
    /* Chien: positions p = 0..n-1 (power), array index k = n-1-p */
    int found = 0;
    for (int p = 0; p < n; p++) {
        uint8_t xinv = GEXP[(255 - p) % 255];
        uint8_t v = 0, xp = 1;
        for (int i = 0; i <= Ld; i++) { v ^= gmul(L[i], xp); xp = gmul(xp, xinv); }
        if (v) continue;
        /* Forney (first root 0): e = X * Omega(X^-1) / Lambda'(X^-1) */
        uint8_t om = 0, dl = 0; xp = 1;
        for (int i = 0; i < ne; i++) { om ^= gmul(O[i], xp); xp = gmul(xp, xinv); }
        xp = 1;
        for (int i = 1; i <= Ld; i += 2) { dl ^= gmul(L[i], xp); xp = gmul(xp, gmul(xinv, xinv)); }
        if (!dl) return -1;
        uint8_t e = gmul(GEXP[p % 255], gdiv(om, dl));
        c[n - 1 - p] ^= e;
        found++;
    }
    if (found != Ld) return -1;
    for (int j = 0; j < ne; j++) {
        uint8_t s = 0, a = GEXP[j];
        for (int k = 0; k < n; k++) s = gmul(s, a) ^ c[k];
        if (s) return -1;
    }
    return 0;
}

/* ================= building the matrix ================= */

typedef struct { int size; uint8_t *m, *fn; } Grid;
#define AT(g, x, y) (g)->m[(y) * (g)->size + (x)]
#define FN(g, x, y) (g)->fn[(y) * (g)->size + (x)]

static void setf(Grid *g, int x, int y, int dark) { AT(g, x, y) = (uint8_t)(dark != 0); FN(g, x, y) = 1; }

static void draw_finder(Grid *g, int cx, int cy) {
    for (int dy = -4; dy <= 4; dy++) for (int dx = -4; dx <= 4; dx++) {
        int x = cx + dx, y = cy + dy;
        if (x < 0 || y < 0 || x >= g->size || y >= g->size) continue;
        int d = abs(dx) > abs(dy) ? abs(dx) : abs(dy);
        setf(g, x, y, d != 2 && d != 4);
    }
}
static void draw_align(Grid *g, int cx, int cy) {
    for (int dy = -2; dy <= 2; dy++) for (int dx = -2; dx <= 2; dx++) {
        int d = abs(dx) > abs(dy) ? abs(dx) : abs(dy);
        setf(g, cx + dx, cy + dy, d != 1);
    }
}

static int format_bits(int ecc, int mask) {
    int data = FORMAT_ECC_BITS[ecc] << 3 | mask, rem = data;
    for (int i = 0; i < 10; i++) rem = (rem << 1) ^ ((rem >> 9) * 0x537);
    return (data << 10 | rem) ^ 0x5412;
}
static int version_bits(int ver) {
    int rem = ver;
    for (int i = 0; i < 12; i++) rem = (rem << 1) ^ ((rem >> 11) * 0x1f25);
    return ver << 12 | rem;
}

static void draw_format(Grid *g, int bits) {
    int s = g->size;
    for (int i = 0; i <= 5; i++) setf(g, 8, i, bits >> i & 1);
    setf(g, 8, 7, bits >> 6 & 1);
    setf(g, 8, 8, bits >> 7 & 1);
    setf(g, 7, 8, bits >> 8 & 1);
    for (int i = 9; i < 15; i++) setf(g, 14 - i, 8, bits >> i & 1);
    for (int i = 0; i < 8; i++) setf(g, s - 1 - i, 8, bits >> i & 1);
    for (int i = 8; i < 15; i++) setf(g, 8, s - 15 + i, bits >> i & 1);
    setf(g, 8, s - 8, 1);
}

static void draw_functions(Grid *g, int ver) {
    int s = g->size;
    for (int i = 0; i < s; i++) { setf(g, 6, i, i % 2 == 0); setf(g, i, 6, i % 2 == 0); }
    draw_finder(g, 3, 3);
    draw_finder(g, s - 4, 3);
    draw_finder(g, 3, s - 4);
    int pos[7], na = align_pos(ver, pos);
    for (int i = 0; i < na; i++) for (int j = 0; j < na; j++) {
        if ((i == 0 && j == 0) || (i == 0 && j == na - 1) || (i == na - 1 && j == 0)) continue;
        draw_align(g, pos[i], pos[j]);
    }
    draw_format(g, 0);           /* reserve; written later */
    if (ver >= 7) {
        int bits = version_bits(ver);
        for (int i = 0; i < 18; i++) {
            int bit = bits >> i & 1, a = s - 11 + i % 3, b = i / 3;
            setf(g, a, b, bit);
            setf(g, b, a, bit);
        }
    }
}

static int mask_bit(int mask, int x, int y) {
    switch (mask) {
    case 0: return (x + y) % 2 == 0;
    case 1: return y % 2 == 0;
    case 2: return x % 3 == 0;
    case 3: return (x + y) % 3 == 0;
    case 4: return (x / 3 + y / 2) % 2 == 0;
    case 5: return x * y % 2 + x * y % 3 == 0;
    case 6: return (x * y % 2 + x * y % 3) % 2 == 0;
    default: return ((x + y) % 2 + x * y % 3) % 2 == 0;
    }
}
static void apply_mask(Grid *g, int mask) {
    for (int y = 0; y < g->size; y++) for (int x = 0; x < g->size; x++)
        if (!FN(g, x, y) && mask_bit(mask, x, y)) AT(g, x, y) ^= 1;
}

/* Zigzag traversal of the data modules: calls fn for each one, in order. */
typedef void (*ZigFn)(Grid *g, int x, int y, void *ud);
static void zigzag(Grid *g, ZigFn fn, void *ud) {
    int s = g->size;
    for (int right = s - 1; right >= 1; right -= 2) {
        if (right == 6) right = 5;
        for (int v = 0; v < s; v++) for (int j = 0; j < 2; j++) {
            int x = right - j, up = ((right + 1) & 2) == 0, y = up ? s - 1 - v : v;
            if (!FN(g, x, y)) fn(g, x, y, ud);
        }
    }
}
typedef struct { const uint8_t *d; int nbits, i; uint8_t *out; } ZigCtx;
static void zig_put(Grid *g, int x, int y, void *ud) {
    ZigCtx *z = ud;
    if (z->i < z->nbits) AT(g, x, y) = z->d[z->i >> 3] >> (7 - (z->i & 7)) & 1;
    z->i++;
}
static void zig_get(Grid *g, int x, int y, void *ud) {
    ZigCtx *z = ud;
    if (z->i < z->nbits && AT(g, x, y)) z->out[z->i >> 3] |= (uint8_t)(0x80 >> (z->i & 7));
    z->i++;
}

/* Penalty from the standard (N1=3, N2=3, N3=40, N4=10). */
static long run_finder_count(const int *h) {
    int n = h[1];
    int core = n > 0 && h[2] == n && h[3] == n * 3 && h[4] == n && h[5] == n;
    return (core && h[0] >= n * 4 && h[6] >= n) + (core && h[6] >= n * 4 && h[0] >= n);
}
static void run_push(int *h, int len, int s) {
    if (h[0] == 0) len += s;                    /* blank area on the left */
    memmove(h + 1, h, 6 * sizeof(int));
    h[0] = len;
}
static long penalty(Grid *g) {
    int s = g->size;
    long r = 0;
    for (int pass = 0; pass < 2; pass++) {
        for (int a = 0; a < s; a++) {
            int color = 0, run = 0, h[7] = { 0 };
            for (int b = 0; b < s; b++) {
                int c = pass ? AT(g, a, b) : AT(g, b, a);
                if (c == color) { run++; if (run == 5) r += 3; else if (run > 5) r++; }
                else {
                    run_push(h, run, s);
                    if (!color) r += run_finder_count(h) * 40;
                    color = c;
                    run = 1;
                }
            }
            /* end of the line */
            if (color) { run_push(h, run, s); run = 0; }
            run += s;
            run_push(h, run, s);
            r += run_finder_count(h) * 40;
        }
    }
    for (int y = 0; y + 1 < s; y++) for (int x = 0; x + 1 < s; x++) {
        int c = AT(g, x, y);
        if (c == AT(g, x + 1, y) && c == AT(g, x, y + 1) && c == AT(g, x + 1, y + 1)) r += 3;
    }
    long dark = 0;
    for (int i = 0; i < s * s; i++) dark += g->m[i];
    long total = (long)s * s;
    long k = (labs(dark * 20 - total * 10) + total - 1) / total - 1;
    r += k * 10;
    return r;
}

/* Data bits: mode + length + content. Returns the length in bits (or -1 if it does not fit). */
typedef struct { uint8_t *b; int n, cap; } Bits;
static void bput(Bits *bb, unsigned v, int nb) {
    for (int i = nb - 1; i >= 0; i--) {
        if (bb->n < bb->cap * 8) { if (v >> i & 1) bb->b[bb->n >> 3] |= (uint8_t)(0x80 >> (bb->n & 7)); }
        bb->n++;
    }
}

static int pick_mode(const uint8_t *d, size_t len) {
    int num = 1, al = 1;
    for (size_t i = 0; i < len; i++) {
        if (d[i] < '0' || d[i] > '9') num = 0;
        if (!d[i] || !strchr(ALNUM, d[i])) al = 0;
    }
    return num ? 1 : al ? 2 : 4;
}
static long seg_bits(int mode, size_t len, int ver) {
    long b = 4 + cc_bits(mode, ver);
    if (mode == 1) b += (long)(len / 3) * 10 + (len % 3 == 2 ? 7 : len % 3 == 1 ? 4 : 0);
    else if (mode == 2) b += (long)(len / 2) * 11 + (len % 2) * 6;
    else b += (long)len * 8;
    if (len >= (size_t)1 << cc_bits(mode, ver)) return 1L << 30;
    return b;
}

int qr_encode_ex(const uint8_t *data, size_t len, int ecc, int mask, int minver, uint8_t *modules, int *size, int *mask_out) {
    if (ecc < 0 || ecc > 3 || !modules || !size || (len && !data) || mask > 7) return -1;
    gf_init();
    int mode = pick_mode(data, len), ver;
    for (ver = minver < 1 ? 1 : minver; ver <= 40; ver++) if (seg_bits(mode, len, ver) <= data_codewords(ver, ecc) * 8) break;
    if (ver > 40) return -1;
    int ndc = data_codewords(ver, ecc), cap = ndc * 8;
    uint8_t dc[3706];
    memset(dc, 0, sizeof dc);
    Bits bb = { dc, 0, ndc };
    bput(&bb, (unsigned)mode, 4);
    bput(&bb, (unsigned)len, cc_bits(mode, ver));
    if (mode == 1) {
        for (size_t i = 0; i < len;) {
            size_t k = len - i >= 3 ? 3 : len - i;
            unsigned v = 0;
            for (size_t j = 0; j < k; j++) v = v * 10 + (unsigned)(data[i + j] - '0');
            bput(&bb, v, k == 3 ? 10 : k == 2 ? 7 : 4);
            i += k;
        }
    } else if (mode == 2) {
        for (size_t i = 0; i < len; i += 2) {
            unsigned a = (unsigned)(strchr(ALNUM, data[i]) - ALNUM);
            if (i + 1 < len) bput(&bb, a * 45 + (unsigned)(strchr(ALNUM, data[i + 1]) - ALNUM), 11);
            else bput(&bb, a, 6);
        }
    } else for (size_t i = 0; i < len; i++) bput(&bb, data[i], 8);
    int term = cap - bb.n < 4 ? cap - bb.n : 4;
    bput(&bb, 0, term);
    bput(&bb, 0, (8 - bb.n % 8) % 8);
    for (int pad = 0xec; bb.n < cap; pad ^= 0xec ^ 0x11) bput(&bb, (unsigned)pad, 8);

    /* blocks and correction, interleaved */
    int nb = NUM_BLOCKS[ecc][ver], ne = ECC_PER_BLOCK[ecc][ver];
    int raw = raw_modules(ver) / 8, nshort = nb - raw % nb, shortlen = raw / nb;
    uint8_t gen[30], blocks[81][160];
    int blen[81];
    rs_gen(ne, gen);
    for (int i = 0, k = 0; i < nb; i++) {
        int dl = shortlen - ne + (i < nshort ? 0 : 1);
        memcpy(blocks[i], dc + k, (size_t)dl);
        rs_rem(dc + k, dl, gen, ne, blocks[i] + dl);
        blen[i] = dl;
        k += dl;
    }
    uint8_t all[3706];
    int na = 0;
    for (int i = 0; i < shortlen + 1 - ne; i++) for (int b = 0; b < nb; b++) if (i < blen[b]) all[na++] = blocks[b][i];
    for (int i = 0; i < ne; i++) for (int b = 0; b < nb; b++) all[na++] = blocks[b][blen[b] + i];

    int s = ver * 4 + 17;
    Grid g = { s, modules, NULL };
    uint8_t *fn = calloc((size_t)s * s, 1);
    if (!fn) return -1;
    g.fn = fn;
    memset(modules, 0, (size_t)s * s);
    draw_functions(&g, ver);
    ZigCtx z = { all, na * 8, 0, NULL };
    zigzag(&g, zig_put, &z);

    int best = mask;
    if (best < 0) {
        long bp = -1;
        for (int k = 0; k < 8; k++) {
            apply_mask(&g, k);
            draw_format(&g, format_bits(ecc, k));
            long p = penalty(&g);
            if (bp < 0 || p < bp) { bp = p; best = k; }
            apply_mask(&g, k);
        }
    }
    apply_mask(&g, best);
    draw_format(&g, format_bits(ecc, best));
    free(fn);
    *size = s;
    if (mask_out) *mask_out = best;
    return ver;
}

int qr_encode(const uint8_t *data, size_t len, int ecc, uint8_t *modules, int *size) {
    return qr_encode_ex(data, len, ecc, -1, 1, modules, size, NULL);
}

/* ================= decoding: from the grid to the bytes ================= */

static int popc(unsigned v) { int n = 0; while (v) { n += v & 1; v >>= 1; } return n; }

/* format: (ecc, mask) with the smallest Hamming distance (≤ 3) */
static int decode_format(int bits, int *ecc, int *mask) {
    int best = 99;
    for (int e = 0; e < 4; e++) for (int m = 0; m < 8; m++) {
        int d = popc((unsigned)(bits ^ format_bits(e, m)));
        if (d < best) { best = d; *ecc = e; *mask = m; }
    }
    return best <= 3 ? best : -1;
}
static int decode_version(int bits) {
    int best = 99, v = -1;
    for (int k = 7; k <= 40; k++) { int d = popc((unsigned)(bits ^ version_bits(k))); if (d < best) { best = d; v = k; } }
    return best <= 3 ? v : -1;
}

/* Reads the format information from the two copies in grid g (already sampled). */
static int read_format(Grid *g, int *ecc, int *mask) {
    int s = g->size, a = 0, b = 0;
    for (int i = 0; i <= 5; i++) a |= AT(g, 8, i) << i;
    a |= AT(g, 8, 7) << 6 | AT(g, 8, 8) << 7 | AT(g, 7, 8) << 8;
    for (int i = 9; i < 15; i++) a |= AT(g, 14 - i, 8) << i;
    for (int i = 0; i < 8; i++) b |= AT(g, s - 1 - i, 8) << i;
    for (int i = 8; i < 15; i++) b |= AT(g, 8, s - 15 + i) << i;
    int e1, m1, e2, m2;
    int d1 = decode_format(a, &e1, &m1), d2 = decode_format(b, &e2, &m2);
    if (d1 < 0 && d2 < 0) return -1;
    if (d2 < 0 || (d1 >= 0 && d1 <= d2)) { *ecc = e1; *mask = m1; } else { *ecc = e2; *mask = m2; }
    return 0;
}

static int read_version_bits(Grid *g) {
    int s = g->size, a = 0, b = 0;
    for (int i = 0; i < 18; i++) { int x = s - 11 + i % 3, y = i / 3; a |= AT(g, x, y) << i; b |= AT(g, y, x) << i; }
    int va = decode_version(a), vb = decode_version(b);
    return va > 0 ? va : vb;
}

typedef struct { const uint8_t *b; int n, p; } BR;
static int brd(BR *r, int k) {
    if (r->p + k > r->n) { r->p = r->n + 1; return -1; }
    int v = 0;
    for (int i = 0; i < k; i++, r->p++) v = v << 1 | (r->b[r->p >> 3] >> (7 - (r->p & 7)) & 1);
    return v;
}

/* Segments → bytes. */
static int parse_segments(const uint8_t *d, int nbytes, int ver, uint8_t *out, size_t cap) {
    BR r = { d, nbytes * 8, 0 };
    size_t o = 0;
#define PUT(c) do { if (o < cap) out[o] = (uint8_t)(c); o++; } while (0)
    for (;;) {
        if (r.n - r.p < 4) break;
        int mode = brd(&r, 4);
        if (mode == 0) break;
        if (mode == 7) {                                   /* ECI: skipped */
            int f = brd(&r, 8);
            if (f < 0) return -1;
            if ((f & 0xc0) == 0x80) brd(&r, 8);
            else if ((f & 0xe0) == 0xc0) brd(&r, 16);
            else if (f & 0x80) return -1;
            continue;
        }
        if (mode == 3) { brd(&r, 16); continue; }          /* structured append */
        if (mode == 5) continue;                           /* FNC1 first position */
        if (mode == 9) { brd(&r, 8); continue; }
        int g = ver <= 9 ? 0 : ver <= 26 ? 1 : 2;
        if (mode == 1) {
            int n = brd(&r, (int[]){ 10, 12, 14 }[g]);
            if (n < 0) return -1;
            for (; n >= 3; n -= 3) { int v = brd(&r, 10); if (v < 0 || v > 999) return -1; PUT('0' + v / 100); PUT('0' + v / 10 % 10); PUT('0' + v % 10); }
            if (n == 2) { int v = brd(&r, 7); if (v < 0 || v > 99) return -1; PUT('0' + v / 10); PUT('0' + v % 10); }
            else if (n == 1) { int v = brd(&r, 4); if (v < 0 || v > 9) return -1; PUT('0' + v); }
        } else if (mode == 2) {
            int n = brd(&r, (int[]){ 9, 11, 13 }[g]);
            if (n < 0) return -1;
            for (; n >= 2; n -= 2) { int v = brd(&r, 11); if (v < 0 || v >= 45 * 45) return -1; PUT(ALNUM[v / 45]); PUT(ALNUM[v % 45]); }
            if (n == 1) { int v = brd(&r, 6); if (v < 0 || v >= 45) return -1; PUT(ALNUM[v]); }
        } else if (mode == 4) {
            int n = brd(&r, (int[]){ 8, 16, 16 }[g]);
            if (n < 0) return -1;
            for (int i = 0; i < n; i++) { int v = brd(&r, 8); if (v < 0) return -1; PUT(v); }
        } else if (mode == 8) {                            /* kanji → Shift JIS */
            int n = brd(&r, (int[]){ 8, 10, 12 }[g]);
            if (n < 0) return -1;
            for (int i = 0; i < n; i++) {
                int v = brd(&r, 13);
                if (v < 0) return -1;
                int c = (v / 0xc0) << 8 | (v % 0xc0);
                c += c < 0x1f00 ? 0x8140 : 0xc140;
                PUT(c >> 8); PUT(c & 255);
            }
        } else return -1;
        if (r.p > r.n) return -1;
    }
#undef PUT
    if (o >= cap) return -1;
    out[o] = 0;
    return (int)o;
}

/* Sampled grid (modules of version ver) → bytes. */
static int decode_grid(uint8_t *mods, int ver, uint8_t *out, size_t cap) {
    gf_init();
    int s = ver * 4 + 17;
    Grid g = { s, mods, NULL };
    int ecc, mask;
    if (read_format(&g, &ecc, &mask)) return -1;
    uint8_t *fn = calloc((size_t)s * s, 1), *save = malloc((size_t)s * s);
    if (!fn || !save) { free(fn); free(save); return -1; }
    memcpy(save, mods, (size_t)s * s);
    g.fn = fn;
    draw_functions(&g, ver);                 /* only to mark fn; the modules are restored */
    memcpy(mods, save, (size_t)s * s);
    free(save);
    apply_mask(&g, mask);
    int raw = raw_modules(ver) / 8;
    uint8_t all[3706];
    memset(all, 0, sizeof all);
    ZigCtx z = { NULL, raw * 8, 0, all };
    zigzag(&g, zig_get, &z);
    apply_mask(&g, mask);
    free(fn);
    int nb = NUM_BLOCKS[ecc][ver], ne = ECC_PER_BLOCK[ecc][ver];
    int nshort = nb - raw % nb, shortlen = raw / nb;
    uint8_t blocks[81][160];
    int blen[81], k = 0;
    for (int b = 0; b < nb; b++) blen[b] = shortlen - ne + (b < nshort ? 0 : 1);
    for (int i = 0; i < shortlen + 1 - ne; i++) for (int b = 0; b < nb; b++) if (i < blen[b]) blocks[b][i] = all[k++];
    for (int i = 0; i < ne; i++) for (int b = 0; b < nb; b++) blocks[b][blen[b] + i] = all[k++];
    uint8_t data[3706];
    int nd = 0;
    for (int b = 0; b < nb; b++) {
        if (rs_correct(blocks[b], blen[b] + ne, ne)) return -1;
        memcpy(data + nd, blocks[b], (size_t)blen[b]);
        nd += blen[b];
    }
    return parse_segments(data, nd, ver, out, cap);
}

/* ================= decoding: from the image to the grid ================= */

typedef struct { double x, y, ms; int count; } Cand;
typedef struct { const uint8_t *bin; int w, h; Cand c[64]; int nc; } Img;

#define PX(im, x, y) ((im)->bin[(y) * (im)->w + (x)])

static int ratio_ok(const int *st) {
    int total = 0;
    for (int i = 0; i < 5; i++) { if (!st[i]) return 0; total += st[i]; }
    if (total < 7) return 0;
    double m = total / 7.0, v = m * 0.5;
    return fabs(m - st[0]) < v && fabs(m - st[1]) < v && fabs(3 * m - st[2]) < 3 * v && fabs(m - st[3]) < v && fabs(m - st[4]) < v;
}

/* Counts the runs in one direction from (x, y), which must be dark. */
static int cross(Img *im, double fx, double fy, int dx, int dy, int maxc, double *center, double *ms) {
    int x0 = (int)fx, y0 = (int)fy;
    if (x0 < 0 || y0 < 0 || x0 >= im->w || y0 >= im->h || !PX(im, x0, y0)) return 0;
    int st[5] = { 0 };
    int x = x0, y = y0;
    /* backwards: center (dark), white, dark */
    while (x >= 0 && y >= 0 && x < im->w && y < im->h && PX(im, x, y)) { st[2]++; x -= dx; y -= dy; }
    while (x >= 0 && y >= 0 && x < im->w && y < im->h && !PX(im, x, y) && st[1] <= maxc) { st[1]++; x -= dx; y -= dy; }
    while (x >= 0 && y >= 0 && x < im->w && y < im->h && PX(im, x, y) && st[0] <= maxc) { st[0]++; x -= dx; y -= dy; }
    x = x0 + dx; y = y0 + dy;
    while (x >= 0 && y >= 0 && x < im->w && y < im->h && PX(im, x, y)) { st[2]++; x += dx; y += dy; }
    int ex;
    while (x >= 0 && y >= 0 && x < im->w && y < im->h && !PX(im, x, y) && st[3] <= maxc) { st[3]++; x += dx; y += dy; }
    while (x >= 0 && y >= 0 && x < im->w && y < im->h && PX(im, x, y) && st[4] <= maxc) { st[4]++; x += dx; y += dy; }
    ex = dx ? x : y;                 /* end (exclusive) on the coordinate that moves */
    if (!ratio_ok(st)) return 0;
    *center = ex - st[4] - st[3] - st[2] / 2.0;
    *ms = (st[0] + st[1] + st[2] + st[3] + st[4]) / 7.0;
    return st[0] + st[1] + st[2] + st[3] + st[4];
}

static void add_cand(Img *im, double x, double y, double ms) {
    for (int i = 0; i < im->nc; i++) {
        Cand *c = &im->c[i];
        if (fabs(c->x - x) <= c->ms * 2 && fabs(c->y - y) <= c->ms * 2 && fabs(c->ms - ms) <= c->ms * 0.5 + 1) {
            int n = c->count;
            c->x = (c->x * n + x) / (n + 1);
            c->y = (c->y * n + y) / (n + 1);
            c->ms = (c->ms * n + ms) / (n + 1);
            c->count++;
            return;
        }
    }
    if (im->nc < 64) im->c[im->nc++] = (Cand){ x, y, ms, 1 };
}

static void found(Img *im, const int *st, int y, int xend) {
    double cx = xend - st[4] - st[3] - st[2] / 2.0;
    int total = st[0] + st[1] + st[2] + st[3] + st[4];
    double cy, msv, cx2, msh;
    if (!cross(im, cx, y, 0, 1, st[2], &cy, &msv)) return;
    if (!cross(im, cx, cy, 1, 0, st[2], &cx2, &msh)) return;
    double ms = (msv + msh) / 2;
    if (msv * 7 > total * 2 || msv * 7 * 2 < total) return;
    /* diagonal (discards stripes) */
    double dc, dms;
    if (!cross(im, cx2, cy, 1, 1, st[2] * 2, &dc, &dms)) return;
    add_cand(im, cx2, cy, ms);
}

static void find_finders(Img *im) {
    im->nc = 0;
    for (int y = 0; y < im->h; y++) {
        int st[5] = { 0 }, cur = 0;
        for (int x = 0; x < im->w; x++) {
            if (PX(im, x, y)) {
                if (cur & 1) cur++;
                st[cur]++;
            } else {
                if (cur == 0 && st[0] == 0) continue;
                if (!(cur & 1)) {
                    if (cur == 4) {
                        if (ratio_ok(st)) { found(im, st, y, x); memset(st, 0, sizeof st); cur = 0; continue; }
                        st[0] = st[2]; st[1] = st[3]; st[2] = st[4]; st[3] = 1; st[4] = 0; cur = 3;
                        continue;
                    }
                    cur++;
                    st[cur]++;
                } else st[cur]++;
            }
        }
        if (cur == 4 && ratio_ok(st)) found(im, st, y, im->w);
    }
}

static double dist(const Cand *a, const Cand *b) { return hypot(a->x - b->x, a->y - b->y); }

/* Homography: from (u, v) in modules to (x, y) in the image, from 4 points (8x8 system). */
typedef struct { double a[9]; } Hom;
static int hom_solve(const double *u, const double *v, const double *x, const double *y, Hom *H) {
    double M[8][9];
    for (int i = 0; i < 4; i++) {
        double r1[9] = { u[i], v[i], 1, 0, 0, 0, -u[i] * x[i], -v[i] * x[i], x[i] };
        double r2[9] = { 0, 0, 0, u[i], v[i], 1, -u[i] * y[i], -v[i] * y[i], y[i] };
        memcpy(M[2 * i], r1, sizeof r1);
        memcpy(M[2 * i + 1], r2, sizeof r2);
    }
    for (int c = 0; c < 8; c++) {
        int p = c;
        for (int r = c + 1; r < 8; r++) if (fabs(M[r][c]) > fabs(M[p][c])) p = r;
        if (fabs(M[p][c]) < 1e-12) return -1;
        if (p != c) for (int k = 0; k < 9; k++) { double t = M[c][k]; M[c][k] = M[p][k]; M[p][k] = t; }
        for (int r = 0; r < 8; r++) {
            if (r == c) continue;
            double f = M[r][c] / M[c][c];
            for (int k = c; k < 9; k++) M[r][k] -= f * M[c][k];
        }
    }
    for (int i = 0; i < 8; i++) H->a[i] = M[i][8] / M[i][i];
    H->a[8] = 1;
    return 0;
}
static void hom_map(const Hom *H, double u, double v, double *x, double *y) {
    double w = H->a[6] * u + H->a[7] * v + H->a[8];
    *x = (H->a[0] * u + H->a[1] * v + H->a[2]) / w;
    *y = (H->a[3] * u + H->a[4] * v + H->a[5]) / w;
}

static int sample(Img *im, double x, double y) {
    int ix = (int)floor(x), iy = (int)floor(y);
    if (ix < 0 || iy < 0 || ix >= im->w || iy >= im->h) return 0;
    return PX(im, ix, iy);
}

/* Alignment pattern near (ex, ey): a dark module surrounded by a light ring and a dark one. */
static int find_align(Img *im, double ex, double ey, double ms, double *ax, double *ay) {
    int r = (int)(ms * 4 + 2), best = 0;
    double bd = 1e18;
    for (int y = (int)ey - r; y <= (int)ey + r; y++) {
        if (y < 0 || y >= im->h) continue;
        for (int x = (int)ex - r; x <= (int)ex + r; x++) {
            if (x < 0 || x >= im->w || !PX(im, x, y)) continue;
            /* horizontal and vertical runs: dark (~1), light (~1) on each side, dark outside */
            int ok = 1;
            double c[2];
            for (int d = 0; d < 2 && ok; d++) {
                int dx = d == 0, dy = d == 1, n0 = 0, l1 = 0, r1 = 0, xx, yy;
                int lim = (int)(ms * 2.5) + 1;
                for (xx = x, yy = y; xx >= 0 && yy >= 0 && PX(im, xx, yy) && n0 < lim; xx -= dx, yy -= dy) n0++;
                int left = d ? yy : xx;
                for (; xx >= 0 && yy >= 0 && !PX(im, xx, yy) && l1 < lim; xx -= dx, yy -= dy) l1++;
                if (xx < 0 || yy < 0 || !PX(im, xx, yy)) { ok = 0; break; }
                for (xx = x + dx, yy = y + dy; xx < im->w && yy < im->h && PX(im, xx, yy) && n0 < lim * 2; xx += dx, yy += dy) n0++;
                int right = d ? yy : xx;
                for (; xx < im->w && yy < im->h && !PX(im, xx, yy) && r1 < lim; xx += dx, yy += dy) r1++;
                if (xx >= im->w || yy >= im->h || !PX(im, xx, yy)) { ok = 0; break; }
                if (fabs(n0 - ms) > ms * 0.7 + 1 || fabs(l1 - ms) > ms * 0.7 + 1 || fabs(r1 - ms) > ms * 0.7 + 1) ok = 0;
                c[d] = (left + 1 + right) / 2.0;
            }
            if (!ok) continue;
            double dd = (c[0] - ex) * (c[0] - ex) + (c[1] - ey) * (c[1] - ey);
            if (dd < bd) { bd = dd; *ax = c[0]; *ay = c[1]; best = 1; }
        }
    }
    return best;
}

/* Counts the runs of the timing pattern between p0 and p1 (in pixels). */
static int timing_runs(Img *im, double x0, double y0, double x1, double y1, double ms) {
    double len = hypot(x1 - x0, y1 - y0);
    int steps = (int)(len / (ms / 4)) + 1, runs = 0, prev = -1;
    for (int i = 0; i <= steps; i++) {
        double t = (double)i / steps;
        int v = sample(im, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
        if (v != prev) { runs++; prev = v; }
    }
    return runs;
}

static int try_grid(Img *im, const Cand *B, const Cand *A, const Cand *C, int ver, int use_align, uint8_t *out, size_t cap) {
    int s = ver * 4 + 17;
    double ms = (A->ms + B->ms + C->ms) / 3;
    double u[4] = { 3.5, s - 3.5, 3.5, 0 }, v[4] = { 3.5, 3.5, s - 3.5, 0 };
    double x[4] = { B->x, A->x, C->x, 0 }, y[4] = { B->y, A->y, C->y, 0 };
    /* fourth point: affine corner (or the alignment pattern) */
    double k = (s - 6.5 - 3.5) / (s - 7.0);
    u[3] = s - 6.5; v[3] = s - 6.5;
    x[3] = B->x + (A->x - B->x) * k + (C->x - B->x) * k;
    y[3] = B->y + (A->y - B->y) * k + (C->y - B->y) * k;
    if (ver >= 2 && use_align) {
        double ax = 0, ay = 0;
        if (!find_align(im, x[3], y[3], ms, &ax, &ay)) return -1;
        x[3] = ax; y[3] = ay;
    }
    Hom H;
    if (hom_solve(u, v, x, y, &H)) return -1;
    uint8_t *mods = malloc((size_t)s * s);
    if (!mods) return -1;
    for (int r = 0; r < s; r++) for (int c = 0; c < s; c++) {
        double px, py;
        hom_map(&H, c + 0.5, r + 0.5, &px, &py);
        mods[r * s + c] = (uint8_t)sample(im, px, py);
    }
    int n = -1;
    if (ver >= 7) {
        Grid g = { s, mods, NULL };
        int vv = read_version_bits(&g);
        if (vv > 0 && vv != ver) { free(mods); return -2 - vv; }    /* different version: try that one */
    }
    n = decode_grid(mods, ver, out, cap);
    free(mods);
    return n;
}

static int try_triplet(Img *im, Cand B, Cand A, Cand C, uint8_t *out, size_t cap) {
    double ms = (A.ms + B.ms + C.ms) / 3;
    double dab = dist(&B, &A), dbc = dist(&B, &C);
    int est = (int)lround((dab + dbc) / 2 / ms) + 7;
    /* timing pattern: row 6 (from B to A) and column 6 (from B to C) */
    double ux = (C.x - B.x) / dbc, uy = (C.y - B.y) / dbc, vx = (A.x - B.x) / dab, vy = (A.y - B.y) / dab;
    int t1 = timing_runs(im, B.x + ux * 3 * ms, B.y + uy * 3 * ms, A.x + ux * 3 * ms, A.y + uy * 3 * ms, ms) + 12;
    int t2 = timing_runs(im, B.x + vx * 3 * ms, B.y + vy * 3 * ms, C.x + vx * 3 * ms, C.y + vy * 3 * ms, ms) + 12;
    int tried[16], nt = 0;
    int cands[8] = { t1, t2, est, est + 1, est - 1, est + 2, est - 2, est + 4 };
    for (int i = 0; i < 8; i++) {
        int d = cands[i];
        if (d % 4 != 1) continue;
        int ver = (d - 17) / 4;
        if (ver < 1 || ver > 40) continue;
        for (int pass = 0; pass < 3; pass++) {
            int seen = 0;
            for (int j = 0; j < nt; j++) if (tried[j] == ver) seen = 1;
            if (seen || nt >= 16) break;
            tried[nt++] = ver;
            int n = try_grid(im, &B, &A, &C, ver, 1, out, cap);
            if (n == -1 && ver >= 2) n = try_grid(im, &B, &A, &C, ver, 0, out, cap);
            if (n >= 0) return n;
            if (n <= -9) { ver = -2 - n; continue; }        /* the version information said otherwise */
            break;
        }
    }
    return -1;
}

static int decode_bin(Img *im, uint8_t *out, size_t cap) {
    find_finders(im);
    if (im->nc < 3) return -1;
    /* most confirmed first */
    for (int i = 0; i < im->nc; i++) for (int j = i + 1; j < im->nc; j++)
        if (im->c[j].count > im->c[i].count) { Cand t = im->c[i]; im->c[i] = im->c[j]; im->c[j] = t; }
    int n = im->nc < 12 ? im->nc : 12;
    typedef struct { double score; int a, b, c; } Tri;
    Tri tri[220];
    int nt = 0;
    for (int i = 0; i < n; i++) for (int j = i + 1; j < n; j++) for (int k = j + 1; k < n; k++) {
        const Cand *p[3] = { &im->c[i], &im->c[j], &im->c[k] };
        double mn = fmin(p[0]->ms, fmin(p[1]->ms, p[2]->ms)), mx = fmax(p[0]->ms, fmax(p[1]->ms, p[2]->ms));
        if (mx > mn * 1.5) continue;
        double d01 = dist(p[0], p[1]), d02 = dist(p[0], p[2]), d12 = dist(p[1], p[2]);
        int b; double hyp, l1, l2;
        if (d12 >= d01 && d12 >= d02) { b = 0; hyp = d12; l1 = d01; l2 = d02; }
        else if (d02 >= d01) { b = 1; hyp = d02; l1 = d01; l2 = d12; }
        else { b = 2; hyp = d01; l1 = d02; l2 = d12; }
        double ms = (mn + mx) / 2;
        if (l1 < ms * 12 || l2 < ms * 12) continue;
        double legs = fabs(l1 - l2) / fmax(l1, l2), right = fabs(hyp * hyp - l1 * l1 - l2 * l2) / (hyp * hyp);
        if (legs > 0.25 || right > 0.25) continue;
        int idx[3] = { i, j, k };
        int a = idx[(b + 1) % 3], c = idx[(b + 2) % 3];
        tri[nt++] = (Tri){ legs + right + (mx - mn) / mx, idx[b], a, c };
        if (nt == 220) goto enough;
    }
enough:
    for (int i = 0; i < nt; i++) for (int j = i + 1; j < nt; j++) if (tri[j].score < tri[i].score) { Tri t = tri[i]; tri[i] = tri[j]; tri[j] = t; }
    for (int t = 0; t < nt && t < 6; t++) {
        Cand B = im->c[tri[t].a], A = im->c[tri[t].b], C = im->c[tri[t].c];
        /* orientation: with y pointing down, (A-B) x (C-B) > 0 */
        double cr = (A.x - B.x) * (C.y - B.y) - (A.y - B.y) * (C.x - B.x);
        if (cr < 0) { Cand x = A; A = C; C = x; }
        int r = try_triplet(im, B, A, C, out, cap);
        if (r >= 0) return r;
    }
    return -1;
}

int qr_decode(const uint32_t *argb, int w, int h, uint8_t *out, size_t outcap) {
    if (!argb || w < 21 || h < 21 || w > 16384 || h > 16384 || !out || !outcap) return -1;
    size_t n = (size_t)w * h;
    uint8_t *lum = malloc(n), *bin = malloc(n);
    uint32_t *integ = malloc(sizeof(uint32_t) * (size_t)(w + 1) * (h + 1));
    if (!lum || !bin || !integ) { free(lum); free(bin); free(integ); return -1; }
    long hist[256] = { 0 };
    for (size_t i = 0; i < n; i++) {
        uint32_t p = argb[i];
        unsigned a = p >> 24, r = p >> 16 & 255, g = p >> 8 & 255, b = p & 255;
        unsigned l = (r * 77 + g * 150 + b * 29) >> 8;
        l = (l * a + 255 * (255 - a)) / 255;             /* over a white background */
        lum[i] = (uint8_t)l;
        hist[l]++;
    }
    /* Otsu */
    double sum = 0, sumb = 0, wb = 0, best = -1;
    int th = 128;
    for (int i = 0; i < 256; i++) sum += (double)i * hist[i];
    for (int i = 0; i < 256; i++) {
        wb += hist[i];
        if (!wb) continue;
        double wf = (double)n - wb;
        if (wf <= 0) break;
        sumb += (double)i * hist[i];
        double mb = sumb / wb, mf = (sum - sumb) / wf, v = wb * wf * (mb - mf) * (mb - mf);
        if (v > best) { best = v; th = i; }
    }
    /* integral image for the local threshold */
    for (int x = 0; x <= w; x++) integ[x] = 0;
    for (int y = 0; y < h; y++) {
        uint32_t row = 0;
        integ[(size_t)(y + 1) * (w + 1)] = 0;
        for (int x = 0; x < w; x++) { row += lum[(size_t)y * w + x]; integ[(size_t)(y + 1) * (w + 1) + x + 1] = integ[(size_t)y * (w + 1) + x + 1] + row; }
    }
    int win = (w > h ? w : h) / 8;
    if (win < 16) win = 16;
    int res = -1;
    Img *im = malloc(sizeof *im);
    if (!im) goto done;
    im->bin = bin; im->w = w; im->h = h;
    for (int attempt = 0; attempt < 4 && res < 0; attempt++) {
        int invert = attempt & 1, local = attempt >= 2;
        for (int y = 0; y < h; y++) {
            int y0 = y - win / 2 < 0 ? 0 : y - win / 2, y1 = y + win / 2 >= h ? h - 1 : y + win / 2;
            for (int x = 0; x < w; x++) {
                size_t i = (size_t)y * w + x;
                int l = lum[i], dark;
                if (!local) dark = l <= th;
                else {
                    int x0 = x - win / 2 < 0 ? 0 : x - win / 2, x1 = x + win / 2 >= w ? w - 1 : x + win / 2;
                    uint32_t s = integ[(size_t)(y1 + 1) * (w + 1) + x1 + 1] - integ[(size_t)y0 * (w + 1) + x1 + 1]
                               - integ[(size_t)(y1 + 1) * (w + 1) + x0] + integ[(size_t)y0 * (w + 1) + x0];
                    long cnt = (long)(x1 - x0 + 1) * (y1 - y0 + 1);
                    dark = invert ? (long)l * cnt * 100 > (long)s * 115 : (long)l * cnt * 100 < (long)s * 85;
                }
                if (!local && invert) dark = !dark;
                bin[i] = (uint8_t)dark;
            }
        }
        res = decode_bin(im, out, outcap);
    }
done:
    free(im);
    free(lum);
    free(bin);
    free(integ);
    return res;
}
