/* transform.c — 2D affine transforms for `transform: scale(...) rotate(...)` (paint and hit testing
 * only: layout never sees them, as in CSS). */
#include "ui.h"

#include <math.h>

Affine affine_identity(void) { return (Affine){ 1, 0, 0, 1, 0, 0 }; }

Affine affine_mul(Affine m, Affine n) {
    return (Affine){
        m.a * n.a + m.c * n.b, m.b * n.a + m.d * n.b,
        m.a * n.c + m.c * n.d, m.b * n.c + m.d * n.d,
        m.a * n.e + m.c * n.f + m.e, m.b * n.e + m.d * n.f + m.f,
    };
}

int affine_invert(Affine m, Affine *out) {
    float det = m.a * m.d - m.b * m.c;
    if (fabsf(det) < 1e-8f) return 0;
    float i = 1 / det;
    *out = (Affine){ m.d * i, -m.b * i, -m.c * i, m.a * i,
                     (m.c * m.f - m.d * m.e) * i, (m.b * m.e - m.a * m.f) * i };
    return 1;
}

void affine_apply(Affine m, float x, float y, float *ox, float *oy) {
    *ox = m.a * x + m.c * y + m.e;
    *oy = m.b * x + m.d * y + m.f;
}

Box affine_box(Affine m, Box b) {
    float xs[4], ys[4];
    affine_apply(m, b.x0, b.y0, &xs[0], &ys[0]);
    affine_apply(m, b.x1, b.y0, &xs[1], &ys[1]);
    affine_apply(m, b.x1, b.y1, &xs[2], &ys[2]);
    affine_apply(m, b.x0, b.y1, &xs[3], &ys[3]);
    Box r = { xs[0], ys[0], xs[0], ys[0] };
    for (int i = 1; i < 4; i++) {
        r.x0 = fminf(r.x0, xs[i]); r.y0 = fminf(r.y0, ys[i]);
        r.x1 = fmaxf(r.x1, xs[i]); r.y1 = fmaxf(r.y1, ys[i]);
    }
    return r;
}

int style_rotates(float deg) { return fmodf(deg, 360) != 0; }

static float origin(Dim d, float size) {
    return d.unit == UNIT_PCT ? size * d.v / 100 : d.unit == UNIT_PX ? d.v : size / 2;
}

Affine style_transform(const Style *st, float x, float y, float w, float h) {
    const StyleExt *s = st->ext;
    if (!s) return affine_identity();
    float ox = x + origin(s->origin_x, w), oy = y + origin(s->origin_y, h);
    float r = s->rotate * 3.14159265358979f / 180, co = cosf(r), si = sinf(r);
    /* translate(o) · rotate(r) · scale(sx, sy) · translate(-o); y grows downwards, so a positive
     * angle turns clockwise on screen, like CSS */
    Affine m = { co * s->scale_x, si * s->scale_x, -si * s->scale_y, co * s->scale_y, 0, 0 };
    m.e = ox - (m.a * ox + m.c * oy);
    m.f = oy - (m.b * ox + m.d * oy);
    return m;
}
