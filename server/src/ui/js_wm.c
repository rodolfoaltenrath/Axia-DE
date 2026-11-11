/* js_wm.c — lord.wm: window dragging in C (grab.c, docs/WINDOWS.md).
 *
 *   lord.wm.grab(node, edges, { inset, minW, minH, maxW, maxH, minTop, areas, snap, links, area }) → bool
 *       edges 0 = move; 1 top, 2 bottom, 4 left, 8 right (like xdg_toplevel.resize).
 *       Only with a button held down; axia-server tracks it until it is released.
 *       snap: [{ x, y, w, h, work: { x, y, w, h } }] per display turns on the snapping zones while
 *       moving: __lordWm('zone', { zone, x, y, w, h }) each time the zone under the pointer changes
 *       (zone null: none). links: [{ node, inset, minW, minH, maxW, maxH }] the other windows snapped
 *       in work area `area`; those sharing the dragged edge resize with it (docs/WINDOWS.md).
 *       detach: { w, h } moving a maximized or snapped window: once the pointer really moves, the node
 *       takes that size, under the pointer where it was grabbed (as Windows does).
 *   lord.wm.cancel()                 also stops a scripted move
 *   lord.wm.script(node, { rx, ry, periodMs, inset, minTop, areas }) → bool
 *       LordBench's desktop scene (docs/BENCHMARK.md): the node moves along a figure of eight, rx × ry px,
 *       one turn every periodMs (500–60000), one step per frame through the drag's own path (grab.c),
 *       with no button held; pointer input or cancel() stops it. No end notice: the scene's window closes.
 *   lord.wm.policy(json) → object: where a Linux app's new window opens and which windows the session
 *       remembers (rust/axia-shell/src/window_policy.rs; dialogs never take or leave a saved frame).
 *   lord.wm.pointer() → { x, y, active, edges }: the pointer now and the drag in progress (no
 *       arguments, read only).
 * On release: __lordWm('end', { node, edges, dx, dy, dw, dh, zone, detached, links: [{ node, dx, dy, dw, dh }] })
 * with the node's change, the zone it was dropped on, whether it took its detach size and the change
 * of each linked window.
 *
 *   lord.portal.set({ dark, accent: '#rrggbb', accentName, buttons })
 *       what GTK, GNOME and Qt apps read through org.freedesktop.portal.Settings (portal.c). */
#include "js_wm.h"
#include "grab.h"
#include "portal.h"
#include "appearance.h"
#include "ui.h"
#include "../wl/wl.h"
#include <time.h>

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern char *axia_window_policy(const char *input);
extern void axia_window_policy_free(char *reply);

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

static AxiaSnapRect rect(JSContext *c, JSValueConst o) {
    return (AxiaSnapRect){ num(c, o, "x", 0, -100000, 100000), num(c, o, "y", 0, -100000, 100000),
                           num(c, o, "w", 0, 0, 100000), num(c, o, "h", 0, 0, 100000) };
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
    /* snapping while moving: snap: [{ x, y, w, h, work: { x, y, w, h } }] */
    JSValue snap = JS_IsObject(o) ? JS_GetPropertyStr(c, o, "snap") : JS_UNDEFINED;
    for (uint32_t i = 0; i < GRAB_SNAP_DISPLAYS && JS_IsArray(c, snap); i++) {
        JSValue d = JS_GetPropertyUint32(c, snap, i);
        JSValue w = JS_IsObject(d) ? JS_GetPropertyStr(c, d, "work") : JS_UNDEFINED;
        if (JS_IsObject(d) && JS_IsObject(w)) {
            AxiaSnapDisplay *s = &lim.snap[lim.nsnap++];
            s->bounds = rect(c, d);
            s->work = rect(c, w);
        }
        JS_FreeValue(c, w);
        JS_FreeValue(c, d);
    }
    JS_FreeValue(c, snap);
    /* resizing a snapped window: links: [{ node, inset, minW, minH, maxW, maxH }] and their area */
    JSValue links = JS_IsObject(o) ? JS_GetPropertyStr(c, o, "links") : JS_UNDEFINED;
    for (uint32_t i = 0; i < GRAB_LINKS && JS_IsArray(c, links); i++) {
        JSValue l = JS_GetPropertyUint32(c, links, i);
        double node = num(c, l, "node", 0, 0, 2147483647);
        if (JS_IsObject(l) && node > 0) {
            lim.links[lim.nlinks].node = (int)node;
            lim.links[lim.nlinks].inset = (float)num(c, l, "inset", 0, 0, 64);
            lim.links[lim.nlinks].min_w = (float)num(c, l, "minW", 120, 32, 8192);
            lim.links[lim.nlinks].min_h = (float)num(c, l, "minH", 80, 32, 8192);
            lim.links[lim.nlinks].max_w = (float)num(c, l, "maxW", 0, 0, 16384);
            lim.links[lim.nlinks].max_h = (float)num(c, l, "maxH", 0, 0, 16384);
            lim.nlinks++;
        }
        JS_FreeValue(c, l);
    }
    JS_FreeValue(c, links);
    JSValue area = JS_IsObject(o) ? JS_GetPropertyStr(c, o, "area") : JS_UNDEFINED;
    lim.area = rect(c, area);
    JS_FreeValue(c, area);
    if (lim.area.w <= 0 || lim.area.h <= 0) lim.nlinks = 0;
    /* a maximized or snapped window being moved: detach: { w, h } its node's own size */
    JSValue detach = JS_IsObject(o) ? JS_GetPropertyStr(c, o, "detach") : JS_UNDEFINED;
    if (JS_IsObject(detach)) {
        lim.detach_w = (float)num(c, detach, "w", 0, 0, 16384);
        lim.detach_h = (float)num(c, detach, "h", 0, 0, 16384);
        if (lim.detach_w < 32 || lim.detach_h < 32) lim.detach_w = lim.detach_h = 0;
    }
    JS_FreeValue(c, detach);
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
    JSValue ev = JS_GetPropertyStr(c, argv[0], "externalStyle");
    int external = !JS_IsUndefined(ev) && JS_ToBool(c, ev);
    JS_FreeValue(c, ev);
    portal_set(dark, rgb, has_name ? name : NULL, has_buttons ? buttons : NULL, external);
    static int applied, last_external, last_dark;
    if (!applied || external != last_external || dark != last_dark) {
        axia_external_style(external, dark);
        applied = 1; last_external = external; last_dark = dark;
    }
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
extern void axia_dock_preview_finish(uint32_t, int, double, double, int);
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
            Image *img=image_argb(dst->img,w,h,px);
            if (img) {dst->img=img;dst->paint_dirty=1;dst->gen++;ui.dirty=1;ok=1;}
        }
        free(px);
    }
    double end=preview_now();axia_dock_preview_finish(handle,source,end-now,end,ok);
    return JS_NewBool(c,ok);
}

/* Capture the client's retained surface even when its frame is minimized/unmounted.
 * This never maps a window or sends a configure/frame callback to the client. */
static JSValue f_thumbnail_surface(JSContext *c,JSValueConst self,int argc,JSValueConst *argv) {
    (void)self; uint32_t handle=0; int32_t sid=0,target=0,w=0,h=0; int sw=0,sh=0;
    if (argc<5 || JS_ToUint32(c,&handle,argv[0]) || JS_ToInt32(c,&sid,argv[1]) ||
        JS_ToInt32(c,&target,argv[2]) || JS_ToInt32(c,&w,argv[3]) || JS_ToInt32(c,&h,argv[4])) return JS_FALSE;
    Node *dst=node_get(target);
    if (!dst || dst->type!=NODE_IMAGE || sid<=0 || sid>=0x40000000 || w<1 || h<1 || w>384 || h>256) return JS_FALSE;
    if (!wl_preview_size(sid,&sw,&sh)) {
        if (dst->img) {image_release(dst->img);dst->img=NULL;dst->paint_dirty=1;dst->gen++;ui.dirty=1;}
        return JS_FALSE;
    }
    double now=preview_now();
    uint64_t sig=wl_preview_generation(sid)^((uint64_t)(unsigned)w<<48)^((uint64_t)(unsigned)h<<32);
    if (!axia_dock_preview_due(handle,sid|0x40000000,sig,now,!dst->img)) return JS_FALSE;
    uint32_t *px=calloc((size_t)w*h,sizeof *px); int ok=0;
    if (px) {
        Canvas canvas;canvas_init(&canvas,px,w,h,w);canvas_clear(&canvas,0xff17171cu);
        PaintView keep=paint_view;
        float k=fminf((float)w/sw,(float)h/sh);
        paint_view=(PaintView){0,0,k,(w-sw*k)*0.5f,(h-sh*k)*0.5f,keep.index};
        wl_preview_paint(&canvas,sid,0,0,sw,sh,1);
        paint_view=keep;
        Image *img=image_argb(dst->img,w,h,px);
        if (img) {dst->img=img;dst->paint_dirty=1;dst->gen++;ui.dirty=1;ok=1;}
        free(px);
    }
    double end=preview_now();axia_dock_preview_finish(handle,sid|0x40000000,end-now,end,ok);
    return JS_NewBool(c,ok);
}

static const JSCFunctionListEntry portal_funcs[] = {
    JS_CFUNC_DEF("set", 1, f_portal_set),
    JS_CFUNC_DEF("pickResult", 2, f_portal_pick_result),
};

/* lord.wm.policy(json): the decision is Rust's; this only carries the JSON. */
static JSValue f_policy(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    const char *in = argc > 0 && JS_IsString(argv[0]) ? JS_ToCString(c, argv[0]) : NULL;
    if (!in) return JS_ThrowTypeError(c, "window policy requires JSON");
    char *reply = axia_window_policy(in);
    JS_FreeCString(c, in);
    if (!reply) return JS_ThrowOutOfMemory(c);
    JSValue v = JS_ParseJSON(c, reply, strlen(reply), "<window-policy>");
    axia_window_policy_free(reply);
    return v;
}

static const JSCFunctionListEntry wm_funcs[] = {
    JS_CFUNC_DEF("preview", 1, f_preview),
    JS_CFUNC_DEF("thumbnail", 5, f_thumbnail),
    JS_CFUNC_DEF("thumbnailSurface", 5, f_thumbnail_surface),
    JS_CFUNC_DEF("grab", 3, f_grab),
    JS_CFUNC_DEF("cancel", 0, f_cancel),
    JS_CFUNC_DEF("pointer", 0, f_pointer),
    JS_CFUNC_DEF("script", 2, f_script),
    JS_CFUNC_DEF("policy", 1, f_policy),
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

static JSValue zone_value(JSContext *c, int zone) {
    const char *name = zone >= 0 ? axia_snap_zone_name(zone) : NULL;
    return name ? JS_NewString(c, name) : JS_NULL;
}

static void set_rect(JSContext *c, JSValue o, double x, double y, double w, double h) {
    JS_SetPropertyStr(c, o, "x", JS_NewFloat64(c, x));
    JS_SetPropertyStr(c, o, "y", JS_NewFloat64(c, y));
    JS_SetPropertyStr(c, o, "w", JS_NewFloat64(c, w));
    JS_SetPropertyStr(c, o, "h", JS_NewFloat64(c, h));
}

static void call_wm(const char *type, JSValue (*build)(JSContext *, const void *), const void *arg);

static JSValue end_data(JSContext *c, const void *arg) {
    const GrabEnd *e = arg;
    JSValue d = JS_NewObject(c);
    JS_SetPropertyStr(c, d, "node", JS_NewInt32(c, e->node));
    JS_SetPropertyStr(c, d, "edges", JS_NewInt32(c, e->edges));
    JS_SetPropertyStr(c, d, "dx", JS_NewFloat64(c, e->dx));
    JS_SetPropertyStr(c, d, "dy", JS_NewFloat64(c, e->dy));
    JS_SetPropertyStr(c, d, "dw", JS_NewFloat64(c, e->dw));
    JS_SetPropertyStr(c, d, "dh", JS_NewFloat64(c, e->dh));
    JS_SetPropertyStr(c, d, "zone", zone_value(c, e->zone));
    JS_SetPropertyStr(c, d, "detached", JS_NewBool(c, e->detached));
    JSValue links = JS_NewArray(c);
    for (int i = 0; i < e->nlinks; i++) {
        JSValue l = JS_NewObject(c);
        JS_SetPropertyStr(c, l, "node", JS_NewInt32(c, e->links[i].node));
        JS_SetPropertyStr(c, l, "dx", JS_NewFloat64(c, e->links[i].dx));
        JS_SetPropertyStr(c, l, "dy", JS_NewFloat64(c, e->links[i].dy));
        JS_SetPropertyStr(c, l, "dw", JS_NewFloat64(c, e->links[i].dw));
        JS_SetPropertyStr(c, l, "dh", JS_NewFloat64(c, e->links[i].dh));
        JS_SetPropertyUint32(c, links, (uint32_t)i, l);
    }
    JS_SetPropertyStr(c, d, "links", links);
    return d;
}

void js_wm_end(const GrabEnd *e) { call_wm("end", end_data, e); }

typedef struct { int zone; AxiaSnapRect r; } ZoneNotice;
static JSValue zone_data(JSContext *c, const void *arg) {
    const ZoneNotice *z = arg;
    JSValue d = JS_NewObject(c);
    JS_SetPropertyStr(c, d, "zone", zone_value(c, z->zone));
    set_rect(c, d, z->r.x, z->r.y, z->r.w, z->r.h);
    return d;
}

void js_wm_zone_flush(void) {
    ZoneNotice z;
    if (!grab_zone_take(&z.zone, &z.r)) return;
    call_wm("zone", zone_data, &z);
}

static void call_wm(const char *type, JSValue (*build)(JSContext *, const void *), const void *arg) {
    JSContext *c = wctx;
    if (!c) return;
    JSValue g = JS_GetGlobalObject(c), f = JS_GetPropertyStr(c, g, "__lordWm");
    if (JS_IsFunction(c, f)) {
        JSValue args[2] = { JS_NewString(c, type), build(c, arg) };
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
