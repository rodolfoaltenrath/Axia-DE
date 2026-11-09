/* js_snap.c — lord.snap: window snapping's geometry, in Rust (rust/axia-shell/src/snap, snap_ffi.h,
 * docs/WINDOWS.md "Snapping"). Pure functions; the session's window manager (sdk/windows.js) and
 * Snap Assist (sdk/overview.jsx) call them. Rectangles are { x, y, w, h } in screen pixels.
 *
 *   lord.snap.zones                         the zone names, in order
 *   lord.snap.rect(zone, area)              → the zone's rectangle in work area `area`, or null
 *   lord.snap.zoneAt(x, y, area, thirds)    → the zone a window dropped at (x, y) takes, or null
 *   lord.snap.next(at, dir)                 → Super+arrow `dir` ('left'…'down') from `at` (a zone, 'max'
 *                                             or null): a zone, 'restore', 'minimize' or null
 *   lord.snap.assist(zone, held, layout)    → the free zones of `zone`'s layout (`layout`, the zones
 *                                             of the layout picked, if it has `zone`), given the zones
 *                                             other windows on that display hold
 *   lord.snap.grid(cell, frames)            → [{ x, y, w, h, scale }] the candidates' miniatures in `cell`
 */
#include "js_snap.h"
#include "snap_ffi.h"

#include <math.h>
#include <string.h>

/* A rectangle from a JS object: 1 if it has four finite numbers and no negative size. */
static int get_rect(JSContext *c, JSValueConst o, AxiaSnapRect *r) {
    if (!JS_IsObject(o)) return 0;
    static const char *const keys[4] = { "x", "y", "w", "h" };
    double v[4];
    for (int i = 0; i < 4; i++) {
        JSValue p = JS_GetPropertyStr(c, o, keys[i]);
        int ok = JS_IsNumber(p) && !JS_ToFloat64(c, &v[i], p) && isfinite(v[i]) && fabs(v[i]) <= 1e6;
        JS_FreeValue(c, p);
        if (!ok) return 0;
    }
    if (v[2] < 0 || v[3] < 0) return 0;
    *r = (AxiaSnapRect){ v[0], v[1], v[2], v[3] };
    return 1;
}

static JSValue new_rect(JSContext *c, double x, double y, double w, double h) {
    JSValue o = JS_NewObject(c);
    JS_SetPropertyStr(c, o, "x", JS_NewFloat64(c, x));
    JS_SetPropertyStr(c, o, "y", JS_NewFloat64(c, y));
    JS_SetPropertyStr(c, o, "w", JS_NewFloat64(c, w));
    JS_SetPropertyStr(c, o, "h", JS_NewFloat64(c, h));
    return o;
}

/* A zone's index from a JS string, or -1 (null, undefined, unknown names). */
static int get_zone(JSContext *c, JSValueConst v) {
    if (!JS_IsString(v)) return -1;
    size_t n = 0;
    const char *s = JS_ToCStringLen(c, &n, v);
    if (!s) { JS_FreeValue(c, JS_GetException(c)); return -1; }
    int z = n < 32 && strlen(s) == n ? axia_snap_zone_index(s) : -1;
    JS_FreeCString(c, s);
    return z;
}

static JSValue zone_name(JSContext *c, int zone) {
    const char *s = zone >= 0 ? axia_snap_zone_name(zone) : NULL;
    return s ? JS_NewString(c, s) : JS_NULL;
}

static JSValue f_rect(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    AxiaSnapRect a, r;
    if (argc < 2 || !get_rect(c, argv[1], &a)) return JS_NULL;
    if (!axia_snap_rect(get_zone(c, argv[0]), a, &r)) return JS_NULL;
    return new_rect(c, r.x, r.y, r.w, r.h);
}

static JSValue f_zone_at(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    double x, y;
    AxiaSnapRect a;
    if (argc < 3 || !JS_IsNumber(argv[0]) || !JS_IsNumber(argv[1])) return JS_NULL;
    if (JS_ToFloat64(c, &x, argv[0]) || JS_ToFloat64(c, &y, argv[1]) || !get_rect(c, argv[2], &a)) return JS_NULL;
    return zone_name(c, axia_snap_zone_at(x, y, a, argc > 3 && JS_ToBool(c, argv[3]) > 0));
}

static JSValue f_next(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    static const char *const dirs[4] = { "left", "right", "up", "down" };
    if (argc < 2 || !JS_IsString(argv[1])) return JS_NULL;
    int at = get_zone(c, argv[0]);
    if (at < 0 && !JS_IsNull(argv[0]) && !JS_IsUndefined(argv[0])) return JS_NULL;   /* an unknown place */
    const char *d = JS_ToCString(c, argv[1]);
    if (!d) return JS_EXCEPTION;
    int dir = -1;
    for (int i = 0; i < 4; i++) if (!strcmp(d, dirs[i])) dir = i;
    JS_FreeCString(c, d);
    int r = dir < 0 ? -1 : axia_snap_next(at, dir);
    if (r == AXIA_SNAP_RESTORE) return JS_NewString(c, "restore");
    if (r == AXIA_SNAP_MINIMIZE) return JS_NewString(c, "minimize");
    return zone_name(c, r);
}

static uint32_t array_len(JSContext *c, JSValueConst a, uint32_t max) {
    if (!JS_IsArray(c, a)) return 0;
    JSValue l = JS_GetPropertyStr(c, a, "length");
    uint32_t n = 0;
    if (JS_ToUint32(c, &n, l)) n = 0;
    JS_FreeValue(c, l);
    return n < max ? n : max;
}

/* An array of zone names into indices (unknown names become -1, which Rust ignores). */
static uint32_t get_zones(JSContext *c, JSValueConst a, int *out, uint32_t max) {
    uint32_t n = array_len(c, a, max);
    for (uint32_t i = 0; i < n; i++) {
        JSValue v = JS_GetPropertyUint32(c, a, i);
        out[i] = get_zone(c, v);
        JS_FreeValue(c, v);
    }
    return n;
}

static JSValue f_assist(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    int zone = argc > 0 ? get_zone(c, argv[0]) : -1, held[64], layout[16], out[16];
    uint32_t n = argc > 1 ? get_zones(c, argv[1], held, 64) : 0;
    uint32_t nl = argc > 2 ? get_zones(c, argv[2], layout, 16) : 0;
    JSValue list = JS_NewArray(c);
    size_t k = zone < 0 ? 0 : axia_snap_assist_zones(zone, layout, nl, held, n, out, 16);
    for (size_t i = 0; i < k; i++) JS_SetPropertyUint32(c, list, (uint32_t)i, zone_name(c, out[i]));
    return list;
}

static JSValue f_grid(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    AxiaSnapRect cell, frames[AXIA_SNAP_ASSIST_MAX];
    AxiaSnapPlace places[AXIA_SNAP_ASSIST_MAX];
    JSValue list = JS_NewArray(c);
    if (argc < 2 || !get_rect(c, argv[0], &cell)) return list;
    uint32_t n = array_len(c, argv[1], AXIA_SNAP_ASSIST_MAX);
    for (uint32_t i = 0; i < n; i++) {
        JSValue v = JS_GetPropertyUint32(c, argv[1], i);
        int ok = get_rect(c, v, &frames[i]);
        JS_FreeValue(c, v);
        if (!ok) return list;
    }
    size_t k = axia_snap_assist_grid(cell, frames, n, places, AXIA_SNAP_ASSIST_MAX);
    for (size_t i = 0; i < k; i++) {
        JSValue p = new_rect(c, places[i].x, places[i].y, places[i].w, places[i].h);
        JS_SetPropertyStr(c, p, "scale", JS_NewFloat64(c, places[i].scale));
        JS_SetPropertyUint32(c, list, (uint32_t)i, p);
    }
    return list;
}

static const JSCFunctionListEntry snap_funcs[] = {
    JS_CFUNC_DEF("rect", 2, f_rect),
    JS_CFUNC_DEF("zoneAt", 4, f_zone_at),
    JS_CFUNC_DEF("next", 2, f_next),
    JS_CFUNC_DEF("assist", 3, f_assist),
    JS_CFUNC_DEF("grid", 2, f_grid),
};

void js_snap_register(JSContext *c, JSValue lord) {
    JSValue o = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, o, snap_funcs, sizeof snap_funcs / sizeof snap_funcs[0]);
    JSValue zones = JS_NewArray(c);
    for (int i = 0; i < axia_snap_zone_count(); i++) JS_SetPropertyUint32(c, zones, (uint32_t)i, zone_name(c, i));
    JS_SetPropertyStr(c, o, "zones", zones);
    JS_SetPropertyStr(c, lord, "snap", o);
}
