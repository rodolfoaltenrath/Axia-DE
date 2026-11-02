/* qr_test.c — our own QR codes (src/vault/qr.c):
 *   1. known answers: matrices generated with segno (tests/qr/qr_ref.h), with the mask
 *      chosen by the penalty score and, for large versions, with a fixed mask (hash);
 *   2. round trip: encode → draw (scale 2–8, inverted, rotated, noise, blur) → decode;
 *   3. error correction: modules flipped up to the capacity (decodes) and beyond (fails);
 *   4. real images (segno PNGs in tests/qr/, including a screenshot with UI);
 *   5. "fuzz": random images and damaged QR codes can't break anything.
 * Run from server/ (make test). */
#include "vault/qr.h"
#include "vault/vstb.h"
#include "vault/smem.h"
#include "qr/qr_ref.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

int qr_encode_ex(const uint8_t *data, size_t len, int ecc, int mask, int minver, uint8_t *modules, int *size, int *mask_out);

static int fails, total;
static void check(int ok, const char *what) { total++; if (!ok) { fails++; printf("FAIL: %s\n", what); } }

static uint64_t rs = 0x9e3779b97f4a7c15ULL;
static uint32_t rnd(void) { rs ^= rs << 13; rs ^= rs >> 7; rs ^= rs << 17; return (uint32_t)(rs >> 11); }
static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }

static uint8_t M[QR_MAX_SIZE * QR_MAX_SIZE];

/* Draws the matrix: scale sc, quiet zone of 4, rotation in degrees, inverted. */
static uint32_t *raster(const uint8_t *m, int s, double sc, double deg, int inv, int *W, int *H) {
    double side = (s + 8) * sc, a = deg * M_PI / 180, ca = cos(a), sa = sin(a);
    int w = (int)ceil(side * (fabs(ca) + fabs(sa))) + 4;
    *W = *H = w;
    uint32_t *px = malloc(sizeof(uint32_t) * (size_t)w * w);
    if (!px) return NULL;
    double c = w / 2.0;
    for (int y = 0; y < w; y++) for (int x = 0; x < w; x++) {
        int dark = 0;
        for (int sy = 0; sy < 2; sy++) for (int sx = 0; sx < 2; sx++) {
            double dx = x + 0.25 + sx * 0.5 - c, dy = y + 0.25 + sy * 0.5 - c;
            double u = (dx * ca + dy * sa) / sc + (s + 8) / 2.0 - 4, v = (-dx * sa + dy * ca) / sc + (s + 8) / 2.0 - 4;
            int mu = (int)floor(u), mv = (int)floor(v);
            if (mu >= 0 && mv >= 0 && mu < s && mv < s && m[mv * s + mu]) dark++;
        }
        int l = 255 - dark * 255 / 4;
        if (inv) l = 255 - l;
        px[y * w + x] = 0xff000000u | (uint32_t)l << 16 | (uint32_t)l << 8 | (uint32_t)l;
    }
    return px;
}
static void noise(uint32_t *px, int w, int h, int amount) {
    for (int i = 0; i < w * h; i++) {
        int l = (int)(px[i] & 255) + (int)(rnd() % (2 * amount + 1)) - amount;
        l = l < 0 ? 0 : l > 255 ? 255 : l;
        px[i] = 0xff000000u | (uint32_t)l * 0x010101u;
    }
}
static void blur(uint32_t *px, int w, int h) {
    uint32_t *t = malloc(sizeof(uint32_t) * (size_t)w * h);
    if (!t) return;
    for (int y = 0; y < h; y++) for (int x = 0; x < w; x++) {
        int s = 0, n = 0;
        for (int dy = -1; dy <= 1; dy++) for (int dx = -1; dx <= 1; dx++) {
            int xx = x + dx, yy = y + dy;
            if (xx >= 0 && yy >= 0 && xx < w && yy < h) { s += px[yy * w + xx] & 255; n++; }
        }
        t[y * w + x] = 0xff000000u | (uint32_t)(s / n) * 0x010101u;
    }
    memcpy(px, t, sizeof(uint32_t) * (size_t)w * h);
    free(t);
}
static void rotate90(uint8_t *m, int s, int times) {
    static uint8_t t[QR_MAX_SIZE * QR_MAX_SIZE];
    while (times-- > 0) {
        for (int y = 0; y < s; y++) for (int x = 0; x < s; x++) t[x * s + (s - 1 - y)] = m[y * s + x];
        memcpy(m, t, (size_t)s * s);
    }
}

static uint64_t fnv(const uint8_t *m, int s) {
    uint64_t h = 0xcbf29ce484222325ULL;
    for (int i = 0; i < s * s; i++) { h ^= m[i]; h *= 0x100000001b3ULL; }
    return h;
}

static uint8_t *readfile(const char *p, size_t *n) {
    FILE *f = fopen(p, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long l = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *b = malloc((size_t)l + 1);
    if (b && fread(b, 1, (size_t)l, f) != (size_t)l) { free(b); b = NULL; }
    fclose(f);
    *n = (size_t)l;
    return b;
}

static const char *ALN = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

int main(void) {
    uint8_t out[4096];
    int s, mask;

    /* ---- 1. known answers ---- */
    for (size_t k = 0; k < sizeof KAT / sizeof KAT[0]; k++) {
        const QrKat *q = &KAT[k];
        int v = qr_encode_ex((const uint8_t *)q->data, strlen(q->data), q->ecc, -1, q->ver, M, &s, &mask);
        char what[96];
        snprintf(what, sizeof what, "KAT «%s» version %d", q->data, q->ver);
        check(v == q->ver, what);
        snprintf(what, sizeof what, "KAT «%s» chosen mask %d (segno %d)", q->data, mask, q->mask);
        check(mask == q->mask, what);
        qr_encode_ex((const uint8_t *)q->data, strlen(q->data), q->ecc, q->mask, q->ver, M, &s, NULL);
        int same = 1;
        for (int y = 0; y < s; y++) for (int x = 0; x < s; x++) if ((q->rows[y][x] == '1') != M[y * s + x]) same = 0;
        snprintf(what, sizeof what, "KAT «%s» matrix identical to segno", q->data);
        check(same, what);
    }
    {
        const char *base = "lordos contrasenas qr prueba ";
        static uint8_t txt[3000];
        for (int i = 0; i < 3000; i++) txt[i] = (uint8_t)base[i % 29];
        for (size_t k = 0; k < sizeof BIG / sizeof BIG[0]; k++) {
            const QrBig *b = &BIG[k];
            int v = qr_encode_ex(txt, (size_t)b->len, b->ecc, b->mask, b->ver, M, &s, NULL);
            char what[96];
            snprintf(what, sizeof what, "version %d ecc %d mask %d: hash identical to segno", b->ver, b->ecc, b->mask);
            check(v == b->ver && fnv(M, s) == b->hash, what);
            /* one more byte doesn't fit in that version */
            v = qr_encode_ex(txt, (size_t)b->len + 1, b->ecc, b->mask, b->ver, M, &s, NULL);
            check(v != b->ver, "one more byte bumps the version");
        }
    }
    check(qr_encode((const uint8_t *)"x", 1, QR_ECC_L, M, &s) == 1 && s == 21, "version 1");
    {
        static uint8_t big[3000];
        memset(big, 'a', sizeof big);
        check(qr_encode(big, 2954, QR_ECC_L, M, &s) == -1, "doesn't fit in version 40");
    }

    /* ---- 2. round trip ---- */
    int rt_ok = 0, rt_n = 0, rot_ok = 0, rot_n = 0;
    for (int it = 0; it < 360; it++) {
        int ecc = (int)(rnd() % 4), kind = (int)(rnd() % 3);
        static const int caps[4] = { 2953, 2331, 1663, 1273 };
        int maxlen = it < 300 ? (int)(rnd() % 400) + 1 : caps[ecc];
        int len = (int)(rnd() % (unsigned)maxlen) + 1;
        if (kind == 0) len = len * 2 > 7089 ? 7089 : len * 2;
        if (len > caps[ecc]) len = caps[ecc];
        static uint8_t data[7100];
        for (int i = 0; i < len; i++) data[i] = kind == 0 ? (uint8_t)('0' + rnd() % 10) : kind == 1 ? (uint8_t)ALN[rnd() % 45] : (uint8_t)(rnd() % 255 + 1);
        int v = qr_encode(data, (size_t)len, ecc, M, &s);
        if (v < 0) { check(0, "encode random data"); continue; }
        int extra = (int)(rnd() % 10);
        double sc = 2 + rnd() % 7;
        while ((s + 8) * sc > 900 && sc > 2) sc--;
        double deg = 0;
        int inv = extra == 1;
        if (extra == 2 || extra == 3) { deg = (int)(rnd() % 21) - 10; if (sc < 3) sc = 3; }
        if (extra == 4) rotate90(M, s, 1 + (int)(rnd() % 3));
        int w, h;
        uint32_t *px = raster(M, s, sc, deg, inv, &w, &h);
        if (extra == 5) noise(px, w, h, 40);
        if (extra == 6 && sc >= 3) blur(px, w, h);
        int n = qr_decode(px, w, h, out, sizeof out);
        int ok = n == len && !memcmp(out, data, (size_t)len);
        if (deg != 0) { rot_n++; rot_ok += ok; } else { rt_n++; rt_ok += ok; }
        if (!ok) printf("  not decoded: version %d ecc %d length %d scale %.0f rotation %.0f inv %d extra %d\n", v, ecc, len, sc, deg, inv, extra);
        free(px);
    }
    printf("round trip: %d/%d upright, %d/%d rotated ±10°\n", rt_ok, rt_n, rot_ok, rot_n);
    check(rt_ok == rt_n, "round trip (all upright ones)");
    check(rot_ok * 100 >= rot_n * 95, "round trip rotated (≥ 95 %)");

    /* ---- 3. error correction (version 1-H: 17 correction codewords, up to 8 words) ---- */
    {
        const char *msg = "LORDOS-2FA";
        int good = 0, bad_fail = 0, bad_wrong = 0;
        for (int it = 0; it < 60; it++) {
            qr_encode((const uint8_t *)msg, strlen(msg), QR_ECC_H, M, &s);
            int flips = it < 40 ? 1 + it % 8 : 40 + it;
            for (int f = 0; f < flips; f++) {
                int x, y;
                do { x = (int)(rnd() % (unsigned)s); y = (int)(rnd() % (unsigned)s); }
                while ((x < 9 && y < 9) || (x >= s - 8 && y < 9) || (x < 9 && y >= s - 8) || x == 6 || y == 6 || x == 8 || y == 8);
                M[y * s + x] ^= 1;
            }
            int w, h;
            uint32_t *px = raster(M, s, 4, 0, 0, &w, &h);
            int n = qr_decode(px, w, h, out, sizeof out);
            free(px);
            int ok = n == (int)strlen(msg) && !memcmp(out, msg, strlen(msg));
            if (it < 40) good += ok;
            else if (n < 0) bad_fail++;
            else if (!ok) bad_wrong++;
            else bad_fail++;                         /* recovered anyway (repeated flips): counts as clean */
        }
        printf("correction: %d/40 with up to 8 bad modules; with 80–100 bad: %d not decoded, %d decoded wrong\n", good, bad_fail, bad_wrong);
        check(good == 40, "corrects up to the capacity");
        check(bad_wrong <= 1, "beyond the capacity it fails cleanly");
    }

    /* ---- 4. real images ---- */
    {
        struct { const char *file, *want; } real[] = {
            { "tests/qr/wifi.png", "WIFI:T:WPA;S:Casa Lord;P:clave-segura-123;;" },
            { "tests/qr/otpauth.png", "otpauth://totp/LordOS:lord@example.org?secret=JBSWY3DPEHPK3PXP&issuer=LordOS&algorithm=SHA1&digits=6&period=30" },
            { "tests/qr/url-v11.png", "https://example.org/contrasenas/una/ruta/bastante/larga/para/llegar/a/la/version/diez?con=parametros&y=mas&valores=1234567890" },
            { "tests/qr/captura.png", "otpauth://totp/Banco:ana?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Banco" },
        };
        for (size_t k = 0; k < sizeof real / sizeof real[0]; k++) {
            size_t n;
            uint8_t *f = readfile(real[k].file, &n);
            int w = 0, h = 0;
            uint32_t *px = f ? vstb_decode_argb(f, n, &w, &h) : NULL;
            char what[96];
            snprintf(what, sizeof what, "real image %s", real[k].file);
            double t0 = now();
            int r = px ? qr_decode(px, w, h, out, sizeof out) : -1;
            double t1 = now();
            check(r == (int)strlen(real[k].want) && !memcmp(out, real[k].want, (size_t)r), what);
            printf("  %s: %dx%d, %.1f ms\n", real[k].file, w, h, (t1 - t0) * 1e3);
            sm_free(px);
            free(f);
        }
    }

    /* ---- 5. fuzz ---- */
    {
        int crashes = 0;
        for (int it = 0; it < 60; it++) {
            int w = 21 + (int)(rnd() % 300), h = 21 + (int)(rnd() % 300);
            uint32_t *px = malloc(sizeof(uint32_t) * (size_t)w * h);
            for (int i = 0; i < w * h; i++) px[i] = it % 2 ? rnd() : (rnd() & 1 ? 0xff000000u : 0xffffffffu);
            qr_decode(px, w, h, out, sizeof out);
            free(px);
        }
        for (int it = 0; it < 80; it++) {
            int len = 1 + (int)(rnd() % 200);
            uint8_t data[200];
            for (int i = 0; i < len; i++) data[i] = (uint8_t)rnd();
            qr_encode(data, (size_t)len, (int)(rnd() % 4), M, &s);
            for (int f = 0; f < 5 + it * 3; f++) M[rnd() % (unsigned)(s * s)] ^= 1;
            int w, h;
            uint32_t *px = raster(M, s, 3, 0, it % 2, &w, &h);
            /* covered rectangles */
            for (int r = 0; r < it % 4; r++) {
                int x0 = (int)(rnd() % (unsigned)w), y0 = (int)(rnd() % (unsigned)h), rw = (int)(rnd() % 40), rh = (int)(rnd() % 40);
                for (int y = y0; y < y0 + rh && y < h; y++) for (int x = x0; x < x0 + rw && x < w; x++) px[y * w + x] = rnd();
            }
            qr_decode(px, w, h, out, sizeof out);
            qr_decode(px, w, h, out, 3);          /* small buffer: never overrun */
            free(px);
        }
        check(crashes == 0, "fuzz without breaking");
        check(qr_decode(NULL, 0, 0, out, sizeof out) == -1, "null input");
    }

    /* ---- timings ---- */
    {
        const char *wifi = "WIFI:T:WPA;S:Casa Lord;P:clave-segura-123;;";
        double t0 = now();
        for (int i = 0; i < 1000; i++) qr_encode((const uint8_t *)wifi, strlen(wifi), QR_ECC_M, M, &s);
        printf("time: generating the QR of a Wi-Fi network: %.3f ms\n", (now() - t0));
    }

    printf("qr-test: %d/%d\n", total - fails, total);
    return fails ? 1 : 0;
}
