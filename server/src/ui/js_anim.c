/* js_anim.c — lord.anim, the glue of the native animator (rust/axia-shell/src/anim.rs).
 *
 * The shell asks once (lord.anim.minimize); Rust computes each frame's values and this file only
 * writes them into the node's style through the same path as lord.setStyle (node_restyled), so
 * the animation never goes through a React render. When one ends, __lordAnimDone(id) tells the
 * shell; lord.anim.clear(node) puts back what the node showed before. */
#include "js_anim.h"
#include "js.h"
#include "ui.h"
#include "../anim_ffi.h"

#include <string.h>
#include <time.h>

static double now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000.0 + t.tv_nsec / 1e6;
}

static float origin_px(Dim d, float size) {
    return d.unit == UNIT_PX ? d.v : d.unit == UNIT_PCT ? size * d.v / 100 : size / 2;
}

/* What the node shows now. */
static AnimProps current(const Node *n) {
    AnimProps p = { 1, 1, n->s.opacity, 0, 0, 0 };
    const StyleExt *x = n->s.ext;
    if (x) {
        p.scale_x = x->scale_x;
        p.scale_y = x->scale_y;
        int centre = x->origin_x.unit == UNIT_PCT && x->origin_x.v == 50 && x->origin_y.unit == UNIT_PCT && x->origin_y.v == 50;
        if (!centre) {
            p.origin_set = 1;
            p.origin_x = origin_px(x->origin_x, n->w);
            p.origin_y = origin_px(x->origin_y, n->h);
        }
    }
    return p;
}

static void apply(Node *n, const AnimProps *p) {
    Style before = n->s;
    StyleExt ext_before;
    if (n->s.ext) memcpy(&ext_before, n->s.ext, STYLE_EXT_VALUES);
    style_set_number(&n->s, "scaleX", p->scale_x);
    style_set_number(&n->s, "scaleY", p->scale_y);
    style_set_number(&n->s, "opacity", p->opacity);
    if (p->origin_set) {
        style_set_number(&n->s, "transformOriginX", p->origin_x);
        style_set_number(&n->s, "transformOriginY", p->origin_y);
    } else {
        style_reset(&n->s, "transformOriginX");
        style_reset(&n->s, "transformOriginY");
    }
    node_restyled(n, &before, before.ext ? &ext_before : NULL);
}

int js_anim_active(void) { return axia_anim_active(); }

void js_anim_step(double now) {
    (void)now;                          /* Rust and the shell share CLOCK_MONOTONIC (now_ms) */
    AnimFrame f[64];
    size_t n = axia_anim_step(now_ms(), f, 64);
    for (size_t i = 0; i < n; i++) {
        Node *node = node_get(f[i].node);
        if (!node) { axia_anim_forget(f[i].node); continue; }
        apply(node, &f[i].props);
    }
    for (size_t i = 0; i < n; i++) if (f[i].done) js_anim_done(f[i].id);
}

/* lord.anim.minimize(node, dockX, dockY) → id (0: not animated). The node's box is read here. */
static JSValue f_minimize(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    int32_t id = 0;
    double x = 0, y = 0;
    if (argc < 3 || JS_ToInt32(c, &id, argv[0]) || JS_ToFloat64(c, &x, argv[1]) || JS_ToFloat64(c, &y, argv[2])) return JS_NewInt32(c, 0);
    Node *n = node_get(id);
    if (!n || n->w <= 0 || n->h <= 0) return JS_NewInt32(c, 0);
    AnimProps base = current(n);
    uint32_t a = axia_anim_minimize(id, now_ms(), n->ax, n->ay, n->w, n->h, (float)x, (float)y, &base);
    return JS_NewUint32(c, a);
}

/* lord.anim.clear(node): stops its animation and puts back what it showed before. */
static JSValue f_clear(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    int32_t id = 0;
    if (argc < 1 || JS_ToInt32(c, &id, argv[0])) return JS_FALSE;
    AnimProps base;
    if (!axia_anim_clear(id, &base)) return JS_FALSE;
    Node *n = node_get(id);
    if (n) apply(n, &base);
    return JS_TRUE;
}

static const JSCFunctionListEntry anim_funcs[] = {
    JS_CFUNC_DEF("minimize", 3, f_minimize),
    JS_CFUNC_DEF("clear", 1, f_clear),
};

void js_anim_register(JSContext *c, JSValue lord) {
    node_destroy_hook = axia_anim_forget;
    JSValue o = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, o, anim_funcs, sizeof anim_funcs / sizeof anim_funcs[0]);
    JS_SetPropertyStr(c, lord, "anim", o);
}
