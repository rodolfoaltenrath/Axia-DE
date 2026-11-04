/* js_wm.c — lord.wm: window dragging in C (grab.c, docs/WINDOWS.md).
 *
 *   lord.wm.grab(node, edges, { inset, minW, minH, maxW, maxH, minTop }) → bool
 *       edges 0 = move; 1 top, 2 bottom, 4 left, 8 right (like xdg_toplevel.resize).
 *       Only with a button held down; axia-server tracks it until it is released.
 *   lord.wm.cancel()                 also stops a scripted move
 *   lord.wm.script(node, { rx, ry, periodMs, inset, minTop, areas }) → bool
 *       LordBench's desktop scene (docs/BENCHMARK.md): the node moves along a figure of eight, rx × ry px,
 *       one turn every periodMs (500–60000), one step per frame through the drag's own path (grab.c),
 *       with no button held; pointer input or cancel() stops it. No end notice: the scene's window closes.
 *   lord.wm.pointer() → { x, y, active, edges }: the pointer now and the drag in progress (no
 *       arguments, read only). The theme polls it while a window moves, for the snapping preview.
 * On release: __lordWm('end', { node, edges, dx, dy, dw, dh }) with the node's change.
 *
 *   lord.portal.set({ dark, accent: '#rrggbb', accentName, buttons })
 *       what GTK, GNOME and Qt apps read through org.freedesktop.portal.Settings (portal.c). */
#include "js_wm.h"
#include "grab.h"
#include "portal.h"
#include "ui.h"
#include <time.h>

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static JSContext *wctx;

static double num(JSContext *c, JSValueConst o, const char *k, double def, double lo, double hi) {
    if (!JS_IsObject(o)) return def;
    JSValue v = JS_GetPropertyStr(c, o, k);
    double d = def;
    if (JS_IsNumber(v)) JS_ToFloat64(c, &d, v);
    JS_FreeValue(c, v);
    if (!isfinite(d)) d = def;
    return d < lo ? lo : d > hi ? hi : d;
}

static GrabLimits limits(JSContext *c, JSValueConst o) {
    GrabLimits lim = {
        .inset = (float)num(c, o, "inset", 0, 0, 64),
        .min_w = (float)num(c, o, "minW", 120, 32, 8192),
        .min_h = (float)num(c, o, "minH", 80, 32, 8192),
        .max_w = (float)num(c, o, "maxW", 0, 0, 16384),
        .max_h = (float)num(c, o, "maxH", 0, 0, 16384),
        .min_top = (float)num(c, o, "minTop", 0, -100000, 100000),
    };
    JSValue areas = JS_IsObject(o) ? JS_GetPropertyStr(c, o, "areas") : JS_UNDEFINED;
    for (int i = 0; i < 4 && JS_IsArray(c, areas); i++) {
        JSValue a = JS_GetPropertyUint32(c, areas, (uint32_t)i);
        if (JS_IsObject(a)) {
            lim.ax[lim.nareas] = (float)num(c, a, "x", 0, -100000, 100000);
            lim.ay[lim.nareas] = (float)num(c, a, "y", 0, -100000, 100000);
            lim.aw[lim.nareas] = (float)num(c, a, "w", 0, 0, 100000);
            lim.nareas++;
        }
        JS_FreeValue(c, a);
    }
    JS_FreeValue(c, areas);
    return lim;
}

static JSValue f_grab(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    int32_t node = 0, edges = 0;
    if (argc < 2 || JS_ToInt32(c, &node, argv[0]) || JS_ToInt32(c, &edges, argv[1])) return JS_FALSE;
    if (edges < 0 || edges > 15 || (edges & 3) == 3 || (edges & 12) == 12) return JS_FALSE;
    GrabLimits lim = limits(c, argc > 2 ? argv[2] : JS_UNDEFINED);
    return JS_NewBool(c, grab_begin(node, edges, &lim) == 0);
}

/* LordBench's scene: lord.wm.script(node, { rx, ry, periodMs, inset, minTop, areas }) */
static JSValue f_script(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    int32_t node = 0;
    if (argc < 2 || JS_ToInt32(c, &node, argv[0]) || node <= 0 || !JS_IsObject(argv[1])) return JS_FALSE;
    GrabLimits lim = limits(c, argv[1]);
    float rx = (float)num(c, argv[1], "rx", 0, 0, 4000), ry = (float)num(c, argv[1], "ry", 0, 0, 4000);
    float period = (float)num(c, argv[1], "periodMs", 6000, 500, 60000);
    return JS_NewBool(c, grab_script_begin(node, &lim, rx, ry, period) == 0);
}

static JSValue f_cancel(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)c; (void)t; (void)argc; (void)argv;
    grab_cancel();
    return JS_UNDEFINED;
}

static JSValue f_pointer(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    float x = 0, y = 0;
    int edges = 0, active = grab_state(&x, &y, &edges);
    JSValue o = JS_NewObject(c);
    JS_SetPropertyStr(c, o, "x", JS_NewFloat64(c, x));
    JS_SetPropertyStr(c, o, "y", JS_NewFloat64(c, y));
    JS_SetPropertyStr(c, o, "active", JS_NewBool(c, active));
    JS_SetPropertyStr(c, o, "edges", JS_NewInt32(c, edges));
    return o;
}

/* Short text without odd characters (accent name, GTK button order). */
static int get_word(JSContext *c, JSValueConst o, const char *k, char *out, size_t n, const char *allowed) {
    JSValue v = JS_GetPropertyStr(c, o, k);
    const char *s = JS_IsString(v) ? JS_ToCString(c, v) : NULL;
    int ok = s && strlen(s) < n && strspn(s, allowed) == strlen(s);
    if (ok) snprintf(out, n, "%s", s);
    if (s) JS_FreeCString(c, s);
    JS_FreeValue(c, v);
    return ok;
}

static JSValue f_portal_set(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc < 1 || !JS_IsObject(argv[0])) return JS_FALSE;
    JSValue dv = JS_GetPropertyStr(c, argv[0], "dark");
    int dark = JS_ToBool(c, dv);
    JS_FreeValue(c, dv);
    char hex[8] = "", name[16] = "", buttons[64] = "";
    double rgb[3] = { 0.04, 0.52, 1.0 };
    if (get_word(c, argv[0], "accent", hex, sizeof hex, "#0123456789abcdefABCDEF") && hex[0] == '#' && strlen(hex) == 7) {
        unsigned long v = strtoul(hex + 1, NULL, 16);
        rgb[0] = ((v >> 16) & 255) / 255.0;
        rgb[1] = ((v >> 8) & 255) / 255.0;
        rgb[2] = (v & 255) / 255.0;
    }
    int has_name = get_word(c, argv[0], "accentName", name, sizeof name, "abcdefghijklmnopqrstuvwxyz");
    int has_buttons = get_word(c, argv[0], "buttons", buttons, sizeof buttons, "abcdefghijklmnopqrstuvwxyz,:");
    portal_set(dark, rgb, has_name ? name : NULL, has_buttons ? buttons : NULL);
    return JS_TRUE;
}

static JSValue f_portal_pick_result(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    uint32_t id=0, count=0;
    if (argc<2 || JS_ToUint32(c,&id,argv[0]) || !JS_IsArray(c,argv[1])) return JS_FALSE;
    JSValue length=JS_GetPropertyStr(c,argv[1],"length");
    JS_ToUint32(c,&count,length); JS_FreeValue(c,length);
    if (count>1024) return JS_FALSE;
    const char **uris=calloc(count?count:1,sizeof *uris);
    if (!uris) return JS_ThrowOutOfMemory(c);
    unsigned used=0;
    for (;used<count;used++) {
        JSValue value=JS_GetPropertyUint32(c,argv[1],used);
        uris[used]=JS_ToCString(c,value); JS_FreeValue(c,value);
        if (!uris[used] || strncmp(uris[used],"file:///",8) || strlen(uris[used])>32768) break;
    }
    if (used==count) portal_pick_result(id,uris,count);
    for (unsigned i=0;i<=used && i<count;i++) if (uris[i]) JS_FreeCString(c,uris[i]);
    free(uris); return JS_NewBool(c,used==count);
}

int js_portal_pick(unsigned id, const char *mode, const char *title, const char *folder,
                   const char *name, int multiple, const char *filters) {
    JSContext *c=wctx; if (!c) return 0;
    JSValue g=JS_GetGlobalObject(c), fn=JS_GetPropertyStr(c,g,"__axiaPortalPick");
    int ok=JS_IsFunction(c,fn);
    if (ok) {
        JSValue v=JS_NewObject(c);
        JS_SetPropertyStr(c,v,"id",JS_NewUint32(c,id));
        JS_SetPropertyStr(c,v,"mode",JS_NewString(c,mode));
        JS_SetPropertyStr(c,v,"title",JS_NewString(c,title));
        JS_SetPropertyStr(c,v,"start",JS_NewString(c,folder));
        JS_SetPropertyStr(c,v,"name",JS_NewString(c,name));
        JS_SetPropertyStr(c,v,"multiple",JS_NewBool(c,multiple));
        JS_SetPropertyStr(c,v,"patterns",JS_NewString(c,filters));
        JSValue result=JS_Call(c,fn,g,1,&v);
        if (JS_IsException(result)) { JSValue error=JS_GetException(c);JS_FreeValue(c,error);ok=0; }
        JS_FreeValue(c,result);JS_FreeValue(c,v);
    }
    JS_FreeValue(c,fn);JS_FreeValue(c,g);return ok;
}
void js_portal_close(unsigned id) {
    JSContext *c=wctx; if (!c) return;
    JSValue g=JS_GetGlobalObject(c), fn=JS_GetPropertyStr(c,g,"__axiaPortalClose");
    if (JS_IsFunction(c,fn)) {
        JSValue value=JS_NewUint32(c,id),result=JS_Call(c,fn,g,1,&value);
        if (JS_IsException(result)) { JSValue e=JS_GetException(c);JS_FreeValue(c,e); }
        JS_FreeValue(c,result);JS_FreeValue(c,value);
    }
    JS_FreeValue(c,fn);JS_FreeValue(c,g);
}

/* Rust owns hover timing, paging and the aggregate capture budget. This bridge
 * only validates native handles and paints into a node-owned, bounded image. */
extern char *axia_dock_preview_request(const char *, double);
extern void axia_dock_preview_reset(void);
extern void axia_dock_preview_free(char *);
extern int axia_dock_preview_due(uint32_t, int, uint64_t, double, int);
extern void axia_dock_preview_done(uint32_t, double, double);
static double preview_now(void) { struct timespec ts;clock_gettime(CLOCK_MONOTONIC,&ts);return ts.tv_sec*1000.+ts.tv_nsec/1e6; }
static JSValue f_preview(JSContext *c,JSValueConst self,int argc,JSValueConst *argv) {
    (void)self;if (!argc) return JS_NULL;
    size_t len=0;const char *input=JS_ToCStringLen(c,&len,argv[0]);if (!input) return JS_EXCEPTION;
    if (len>131072 || memchr(input,0,len)) {JS_FreeCString(c,input);return JS_ThrowRangeError(c,"invalid preview request");}
    char *result=axia_dock_preview_request(input,preview_now());JS_FreeCString(c,input);
    if (!result) return JS_ThrowOutOfMemory(c);
    JSValue out=JS_ParseJSON(c,result,strlen(result),"<dock-preview>");axia_dock_preview_free(result);return out;
}
static JSValue f_thumbnail(JSContext *c,JSValueConst self,int argc,JSValueConst *argv) {
    (void)self;uint32_t handle=0;int32_t source=0,target=0,w=0,h=0;
    if (argc<5 || JS_ToUint32(c,&handle,argv[0]) || JS_ToInt32(c,&source,argv[1]) ||
        JS_ToInt32(c,&target,argv[2]) || JS_ToInt32(c,&w,argv[3]) || JS_ToInt32(c,&h,argv[4])) return JS_FALSE;
    Node *n=node_get(source), *dst=node_get(target);
    if (!n || !dst || dst->type!=NODE_IMAGE || w<1 || h<1 || w>384 || h>256 || n->s.display_none) return JS_FALSE;
    for (Node *p=dst;p;p=p->parent) if (p==n) return JS_FALSE; /* never capture our own preview */
    for (Node *p=n;p;p=p->parent) if (p->protect) {
        if (dst->img) { image_release(dst->img);dst->img=NULL;dst->paint_dirty=1;dst->gen++;ui.dirty=1; } return JS_FALSE;
    }
    double now=preview_now();
    uint64_t sig=paint_preview_signature(n) ^ ((uint64_t)(unsigned)w<<48) ^ ((uint64_t)(unsigned)h<<32);
    if (!axia_dock_preview_due(handle,source,sig,now,!dst->img)) return JS_FALSE;
    uint32_t *px=calloc((size_t)w*h,sizeof *px);
    int ok=0;
    if (px) {
        Canvas canvas;canvas_init(&canvas,px,w,h,w);
        if (paint_preview(&canvas,n)) {
            dst->img=image_argb(dst->img,w,h,px);
            if (dst->img) {dst->paint_dirty=1;dst->gen++;ui.dirty=1;ok=1;}
        }
        free(px);
    }
    double end=preview_now();axia_dock_preview_done(handle,end-now,end);
    return JS_NewBool(c,ok);
}

static const JSCFunctionListEntry portal_funcs[] = {
    JS_CFUNC_DEF("set", 1, f_portal_set),
    JS_CFUNC_DEF("pickResult", 2, f_portal_pick_result),
};

static const JSCFunctionListEntry wm_funcs[] = {
    JS_CFUNC_DEF("preview", 1, f_preview),
    JS_CFUNC_DEF("thumbnail", 5, f_thumbnail),
    JS_CFUNC_DEF("grab", 3, f_grab),
    JS_CFUNC_DEF("cancel", 0, f_cancel),
    JS_CFUNC_DEF("pointer", 0, f_pointer),
    JS_CFUNC_DEF("script", 2, f_script),
};

void js_wm_register(JSContext *c, JSValue lord) {
    axia_dock_preview_reset();
    wctx = c;
    JSValue o = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, o, wm_funcs, sizeof wm_funcs / sizeof wm_funcs[0]);
    JS_SetPropertyStr(c, lord, "wm", o);
    JSValue p = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, p, portal_funcs, sizeof portal_funcs / sizeof portal_funcs[0]);
    JS_SetPropertyStr(c, lord, "portal", p);
}

void js_wm_end(int node, int edges, float dx, float dy, float dw, float dh) {
    JSContext *c = wctx;
    if (!c) return;
    JSValue g = JS_GetGlobalObject(c), f = JS_GetPropertyStr(c, g, "__lordWm");
    if (JS_IsFunction(c, f)) {
        JSValue d = JS_NewObject(c);
        JS_SetPropertyStr(c, d, "node", JS_NewInt32(c, node));
        JS_SetPropertyStr(c, d, "edges", JS_NewInt32(c, edges));
        JS_SetPropertyStr(c, d, "dx", JS_NewFloat64(c, dx));
        JS_SetPropertyStr(c, d, "dy", JS_NewFloat64(c, dy));
        JS_SetPropertyStr(c, d, "dw", JS_NewFloat64(c, dw));
        JS_SetPropertyStr(c, d, "dh", JS_NewFloat64(c, dh));
        JSValue args[2] = { JS_NewString(c, "end"), d };
        JSValue r = JS_Call(c, f, g, 2, args);
        if (JS_IsException(r)) {
            JSValue e = JS_GetException(c);
            const char *m = JS_ToCString(c, e);
            fprintf(stderr, "js: __lordWm: %s\n", m ? m : "(exception)");
            if (m) JS_FreeCString(c, m);
            JS_FreeValue(c, e);
        }
        JS_FreeValue(c, r);
        JS_FreeValue(c, args[0]);
        JS_FreeValue(c, args[1]);
        JSContext *jc;
        while (JS_ExecutePendingJob(JS_GetRuntime(c), &jc) > 0) {}
    }
    JS_FreeValue(c, f);
    JS_FreeValue(c, g);
}
