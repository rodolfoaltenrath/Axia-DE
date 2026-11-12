/* canvas_shadow and canvas_rrect_gradient in Rust (rust/axia-shell/src/canvas_shadow.rs,
 * canvas_ops.rs) against the C they replaced, copied below as it was: the same pixels for random boxes, radii, blurs, offsets, colors, positions and
 * clips, over canvases with transparent, translucent and opaque pixels. */
#include "../src/canvas.h"
#include "../src/shadow_ffi.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <stddef.h>
#include <string.h>
#include <time.h>

/* ---- the former C (canvas.c before the move), renamed old_ ---- */
static inline uint32_t old_over_alpha(uint32_t sa, uint32_t da) { return sa + (((da * (255 - sa) + 128) * 257) >> 16); }
static inline uint32_t old_blend(uint32_t dst, uint32_t src, uint32_t a) {
    uint32_t ia = 255 - a;
    uint32_t rb = (((src & 0xff00ff) * a + (dst & 0xff00ff) * ia) >> 8) & 0xff00ff;
    uint32_t g  = (((src & 0x00ff00) * a + (dst & 0x00ff00) * ia) >> 8) & 0x00ff00;
    return old_over_alpha(a, dst >> 24) << 24 | rb | g;
}
static inline void old_put(uint32_t *p, uint32_t argb, uint32_t a) {
    if (a >= 255) *p = argb | 0xff000000u;
    else if (a) *p = old_blend(*p, argb, a);
}
static inline int old_clampi(int v, int lo, int hi) { return v < lo ? lo : v > hi ? hi : v; }
static inline float old_rr_sdf(float px, float py, float cx, float cy, float hw, float hh, float r) {
    float qx = fabsf(px - cx) - (hw - r), qy = fabsf(py - cy) - (hh - r);
    float ox = qx > 0 ? qx : 0, oy = qy > 0 ? qy : 0;
    float in = qx > qy ? qx : qy;
    return (ox || oy ? sqrtf(ox * ox + oy * oy) : 0) + (in < 0 ? in : 0) - r;
}
static inline float old_cov_of(float d) { float c = 0.5f - d; return c <= 0 ? 0 : c >= 1 ? 1 : c; }
static float old_fix_radius(float r, float w, float h) { float m = (w < h ? w : h) * 0.5f; return r < 0 ? 0 : r > m ? m : r; }
static int old_bounds(const Canvas *c, float x, float y, float w, float h, int *x0, int *y0, int *x1, int *y1) {
    *x0 = old_clampi((int)floorf(x), c->clip.x0, c->clip.x1);
    *y0 = old_clampi((int)floorf(y), c->clip.y0, c->clip.y1);
    *x1 = old_clampi((int)ceilf(x + w), c->clip.x0, c->clip.x1);
    *y1 = old_clampi((int)ceilf(y + h), c->clip.y0, c->clip.y1);
    return *x0 < *x1 && *y0 < *y1;
}
static inline uint32_t old_lerp_color(uint32_t a, uint32_t b, float t) {
    int ti = (int)(t * 256);
    uint32_t out = 0;
    for (int s = 0; s < 32; s += 8) {
        int ca = (a >> s) & 255, cb = (b >> s) & 255;
        out |= (uint32_t)(ca + (((cb - ca) * ti) >> 8)) << s;
    }
    return out;
}
static void old_gradient(Canvas *c, float x, float y, float w, float h, float r,
                           uint32_t from, uint32_t to, float angle_deg, float opacity) {
    int x0, y0, x1, y1;
    if (w <= 0 || h <= 0 || !old_bounds(c, x, y, w, h, &x0, &y0, &x1, &y1)) return;
    r = old_fix_radius(r, w, h);
    float cx = x + w * 0.5f, cy = y + h * 0.5f, hw = w * 0.5f, hh = h * 0.5f;
    /* Same as CSS: 180deg = top to bottom. */
    float rad = angle_deg * 3.14159265f / 180.0f, dx = sinf(rad), dy = -cosf(rad);
    float len = fabsf(w * dx) + fabsf(h * dy);
    if (len <= 0) len = 1;
    for (int j = y0; j < y1; j++) {
        uint32_t *row = c->px + (size_t)j * c->stride;
        float py = j + 0.5f;
        for (int i = x0; i < x1; i++) {
            float px = i + 0.5f;
            float cov = old_cov_of(old_rr_sdf(px, py, cx, cy, hw, hh, r));
            if (cov <= 0) continue;
            float t = 0.5f + ((px - cx) * dx + (py - cy) * dy) / len;
            t = t < 0 ? 0 : t > 1 ? 1 : t;
            uint32_t col = old_lerp_color(from, to, t);
            old_put(&row[i], col, (uint32_t)((col >> 24) * cov * opacity + 0.5f));
        }
    }
}

static void old_shadow(Canvas *c, float x, float y, float w, float h, float r, float dx, float dy, float blur, uint32_t argb) {
    if (!(argb >> 24) || w <= 0 || h <= 0) return;
    int b = blur < 1 ? 1 : (int)roundf(blur);
    int W = (int)roundf(w), H = (int)roundf(h), R = (int)roundf(r), DX = (int)roundf(dx), DY = (int)roundf(dy);
    int mx0 = (DX < 0 ? DX : 0) - b, my0 = (DY < 0 ? DY : 0) - b, mw = W + abs(DX) + 2 * b, mh = H + abs(DY) + 2 * b;
    uint8_t *a = calloc((size_t)mw * mh, 1);
    assert(a);
    float fr = old_fix_radius((float)R, (float)W, (float)H), fb = (float)b;
    float bcx = W * 0.5f, bcy = H * 0.5f, scx = bcx + DX, scy = bcy + DY, hw = W * 0.5f, hh = H * 0.5f;
    for (int j = 0; j < mh; j++)
        for (int i = 0; i < mw; i++) {
            float px = mx0 + i + 0.5f, py = my0 + j + 0.5f;
            float inside = old_cov_of(old_rr_sdf(px, py, bcx, bcy, hw, hh, fr));
            if (inside >= 1) continue;
            float t = (old_rr_sdf(px, py, scx, scy, hw, hh, fr) + fb) / (2 * fb);
            if (t >= 1) continue;
            if (t < 0) t = 0;
            float f = (1 - t * t * (3 - 2 * t)) * (1 - inside);
            a[(size_t)j * mw + i] = (uint8_t)(f * 255 + 0.5f);
        }
    int X = (int)roundf(x) + mx0, Y = (int)roundf(y) + my0;
    uint32_t ca = argb >> 24;
    int x0 = old_clampi(X, c->clip.x0, c->clip.x1), x1 = old_clampi(X + mw, c->clip.x0, c->clip.x1);
    int y0 = old_clampi(Y, c->clip.y0, c->clip.y1), y1 = old_clampi(Y + mh, c->clip.y0, c->clip.y1);
    for (int j = y0; j < y1; j++)
        for (int i = x0; i < x1; i++) {
            uint8_t m = a[(size_t)(j - Y) * mw + (i - X)];
            if (m) old_put(&c->px[(size_t)j * c->stride + i], argb, (m * ca + 127) / 255);
        }
    free(a);
}

static unsigned seed = 12345;
static unsigned rnd(unsigned n) { seed = seed * 1103515245u + 12345u; return (seed >> 8) % n; }

int main(void) {
    enum { W = 700, H = 520 };
    uint32_t *a = malloc(W * H * 4), *b = malloc(W * H * 4);
    assert(a && b);
    long pixels = 0;
    for (int k = 0; k < 600; k++) {
        for (int i = 0; i < W * H; i++) {
            unsigned v = rnd(0x7fffffff) * 2654435761u;
            a[i] = i % 3 == 0 ? 0 : i % 3 == 1 ? (v | 0xff000000u) : (v & 0x7f7f7f7fu);
        }
        memcpy(b, a, W * H * 4);
        Canvas ca, cb;
        canvas_init(&ca, a, W, H, W);
        canvas_init(&cb, b, W, H, W);
        Rect clip = { (int)rnd(60), (int)rnd(60), W - (int)rnd(60), H - (int)rnd(60) };
        canvas_set_clip(&ca, clip);
        canvas_set_clip(&cb, clip);
        float x = (float)rnd(500) - 40 + rnd(100) / 100.f, y = (float)rnd(380) - 40 + rnd(100) / 100.f;
        float w = 1 + (float)rnd(420) + rnd(100) / 100.f, h = 1 + (float)rnd(300) + rnd(100) / 100.f;
        float r = (float)rnd(60) - 3, blur = (float)rnd(48) + rnd(100) / 100.f;
        float dx = (float)rnd(31) - 15, dy = (float)rnd(31) - 15;
        uint32_t argb = (uint32_t)rnd(256) << 24 | (uint32_t)rnd(0x1000000);
        old_shadow(&ca, x, y, w, h, r, dx, dy, blur, argb);
        canvas_shadow(&cb, x, y, w, h, r, dx, dy, blur, argb);
        if (memcmp(a, b, W * H * 4)) {
            for (int i = 0; i < W * H; i++)
                if (a[i] != b[i]) { fprintf(stderr, "case %d: pixel %d,%d %08x != %08x\n", k, i % W, i / W, a[i], b[i]); break; }
            return 1;
        }
        pixels += W * H;
    }
    /* the dragged window's layer (ui/paint.c): px starts before the buffer, the canvas is wider than
     * its stride and the clip is the layer's box in screen coordinates */
    for (int k = 0; k < 200; k++) {
        enum { LW = 360, LH = 260 };
        int ox = 40 + (int)rnd(800), oy = 30 + (int)rnd(500);
        for (int i = 0; i < LW * LH; i++) a[i] = i % 2 ? (rnd(0x7fffffff) | 0xff000000u) : 0;
        memcpy(b, a, LW * LH * 4);
        Canvas ca, cb;
        canvas_init(&ca, a - ((ptrdiff_t)oy * LW + ox), ox + LW, oy + LH, LW);
        canvas_init(&cb, b - ((ptrdiff_t)oy * LW + ox), ox + LW, oy + LH, LW);
        Rect clip = { ox, oy, ox + LW, oy + LH };
        canvas_set_clip(&ca, clip);
        canvas_set_clip(&cb, clip);
        float x = ox + (float)rnd(300) - 40, y = oy + (float)rnd(200) - 40, w = 20 + (float)rnd(300), h = 20 + (float)rnd(200);
        float r = (float)rnd(20), blur = 1 + (float)rnd(30), dx = (float)rnd(9) - 4, dy = (float)rnd(13) - 2;
        uint32_t argb = (uint32_t)(40 + rnd(200)) << 24;
        old_shadow(&ca, x, y, w, h, r, dx, dy, blur, argb);
        canvas_shadow(&cb, x, y, w, h, r, dx, dy, blur, argb);
        if (memcmp(a, b, LW * LH * 4)) { fprintf(stderr, "layer case %d differs\n", k); return 1; }
    }
    printf("shadow C/Rust: 600 random shadows and 200 on offset drag layers, %ld pixels identical to the former C\n", pixels);
    /* gradients: fractional boxes and radii, CSS angles and random ones, opacity, translucent ends */
    long gpix = 0;
    for (int k = 0; k < 1500; k++) {
        for (int i = 0; i < W * H; i++) a[i] = i % 4 == 0 ? 0 : (rnd(0x7fffffff) * 2654435761u) | (i % 4 == 1 ? 0xff000000u : 0);
        for (int i = 0; i < W * H; i++) if ((a[i] >> 24) < 255) { uint32_t al = a[i] >> 24, v = a[i];
            a[i] = al << 24 | ((((v >> 16) & 255) * al / 255) << 16) | ((((v >> 8) & 255) * al / 255) << 8) | ((v & 255) * al / 255); }
        memcpy(b, a, W * H * 4);
        Canvas ca, cb;
        canvas_init(&ca, a, W, H, W);
        canvas_init(&cb, b, W, H, W);
        Rect clip = { (int)rnd(80), (int)rnd(80), W - (int)rnd(80), H - (int)rnd(80) };
        canvas_set_clip(&ca, clip);
        canvas_set_clip(&cb, clip);
        float x = (float)rnd(600) - 50 + rnd(100) / 100.f, y = (float)rnd(420) - 50 + rnd(100) / 100.f;
        float w = (float)rnd(500) + rnd(100) / 100.f, h = (float)rnd(380) + rnd(100) / 100.f;
        float r = (float)rnd(80) - 4 + rnd(4) / 4.f;
        static const float css[] = { 0, 90, 180, 270, 45, 135, -90, 360 };
        float angle = k % 3 ? css[rnd(8)] : (float)rnd(72000) / 100.f - 360;
        float opacity = k % 4 ? 1.0f : rnd(1001) / 1000.f;
        uint32_t from = (uint32_t)rnd(0x7fffffff) * 2654435761u, to = (uint32_t)rnd(0x7fffffff) * 40503u;
        if (k % 2) { from |= 0xff000000u; to |= 0xff000000u; }
        old_gradient(&ca, x, y, w, h, r, from, to, angle, opacity);
        canvas_rrect_gradient(&cb, x, y, w, h, r, from, to, angle, opacity);
        if (memcmp(a, b, W * H * 4)) {
            for (int i = 0; i < W * H; i++)
                if (a[i] != b[i]) { fprintf(stderr, "gradient case %d: pixel %d,%d %08x != %08x\n", k, i % W, i / W, a[i], b[i]); break; }
            return 1;
        }
        gpix += W * H;
    }
    printf("gradient C/Rust: 1500 random gradients, %ld pixels identical to the former C\n", gpix);

    /* a maximized window's shadow, the whole screen repainted (an app's frame under glass, a theme
     * animation): the former blend read every byte under the window */
    enum { SW = 1920, SH = 1080 };
    uint32_t *s1 = calloc((size_t)SW * SH, 4), *s2 = calloc((size_t)SW * SH, 4);
    assert(s1 && s2);
    Canvas c1, c2;
    canvas_init(&c1, s1, SW, SH, SW);
    canvas_init(&c2, s2, SW, SH, SW);
    struct timespec t0, t1, t2;
    /* the same cached mask both ways: the former blend (every byte, canvas_alpha_mask) and the ring */
    const AxiaShadowMask *m = axia_shadow_mask(1920, 1000, 12, 24, 0, 8);
    assert(m);
    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (int i = 0; i < 50; i++) canvas_alpha_mask(&c1, m->x0, 32 + m->y0, m->a, m->mw, m->mh, m->mw, 0x59000000);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    for (int i = 0; i < 50; i++) canvas_shadow(&c2, 0, 32, 1920, 1000, 12, 0, 8, 24, 0x59000000);
    clock_gettime(CLOCK_MONOTONIC, &t2);
    assert(!memcmp(s1, s2, (size_t)SW * SH * 4));
    double old_ms = ((t1.tv_sec - t0.tv_sec) * 1e3 + (t1.tv_nsec - t0.tv_nsec) / 1e6) / 50;
    double new_ms = ((t2.tv_sec - t1.tv_sec) * 1e3 + (t2.tv_nsec - t1.tv_nsec) / 1e6) / 50;
    printf("shadow of a 1920x1000 window, blended over the screen: %.3f ms (every byte) -> %.3f ms (its ring), same pixels\n", old_ms, new_ms);
    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (int i = 0; i < 20; i++) old_gradient(&c1, 100, 80, 1600, 900, 12, 0xff2a2a3a, 0xff16161e, 180, 1);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    for (int i = 0; i < 20; i++) canvas_rrect_gradient(&c2, 100, 80, 1600, 900, 12, 0xff2a2a3a, 0xff16161e, 180, 1);
    clock_gettime(CLOCK_MONOTONIC, &t2);
    assert(!memcmp(s1, s2, (size_t)SW * SH * 4));
    printf("a 1600x900 gradient: %.3f ms (the former C) -> %.3f ms, same pixels\n",
           ((t1.tv_sec - t0.tv_sec) * 1e3 + (t1.tv_nsec - t0.tv_nsec) / 1e6) / 20, ((t2.tv_sec - t1.tv_sec) * 1e3 + (t2.tv_nsec - t1.tv_nsec) / 1e6) / 20);
    free(s1);
    free(s2);
    return 0;
}
