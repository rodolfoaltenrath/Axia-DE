/* grab.c — window dragging in C: move and resize without JS per frame (grab.h). */
#include "grab.h"
#include "../cursor.h"
#include "../bench/lbinput.h"

#include <math.h>
#include <string.h>

#define KEEP 48             /* px of the frame that always stay visible */

static struct {
    int active, node, edges;
    float px0, py0;                 /* pointer at start */
    float x0, y0, w0, h0;           /* node's left/top/width/height at start */
    float x, y, w, h;               /* now */
    float ox, oy;                   /* parent's origin on screen */
    GrabLimits lim;
    AxiaSnapDrag *drag;             /* moving with snapping on: the zone under the pointer (Rust) */
    /* resizing a snapped window with others along the dragged edge: member 0 is this window */
    AxiaSnapResize *link;
    int nmem;
    struct { int node; float inset, ox, oy; } mem[GRAB_LINKS + 1];
    AxiaSnapRect start[GRAB_LINKS + 1], out[GRAB_LINKS + 1];   /* the members' frames then and now */
} g;
static float ptr_x, ptr_y;
static int ptr_buttons;
static int shift_held;
/* the zone last announced, kept past the drag so its end is announced too */
static int zone_now = -1, zone_pending;
static AxiaSnapRect zone_rect;

void grab_note_pointer(float x, float y, int buttons) {
    ptr_x = x;
    ptr_y = y;
    ptr_buttons = buttons;
}

int grab_active(void) { return g.active; }

int grab_state(float *x, float *y, int *edges) {
    *x = ptr_x;
    *y = ptr_y;
    *edges = g.active ? g.edges : 0;
    return g.active;
}

int grab_cursor(void) { return g.edges ? cursor_for_edges(g.edges) : CUR_MOVE; }

static float dim_or(Dim d, float fallback) { return d.unit == UNIT_PX ? d.v : fallback; }

/* LordBench's scripted move (grab_script_begin): the node follows the synthetic pointer (bench/lbinput.h), or
 * without it the same path by the clock, one step per frame */
static struct { int on; double t0; float rx, ry, period; } sc;

static void script_off(void) {
    sc.on = 0;
    lbi_stop();
}

/* ---- snapping (rust/axia-shell/src/snap, docs/WINDOWS.md "Snapping") ---- */

static void zone_check(float px, float py) {
    if (!g.drag || !axia_snap_drag_update(g.drag, px, py, shift_held)) return;
    zone_now = axia_snap_drag_zone(g.drag, &zone_rect);
    zone_pending = 1;
}

/* the drag's Rust state goes with it; a zone still shown is announced as gone */
static void snap_release(void) {
    axia_snap_drag_free(g.drag);
    g.drag = NULL;
    axia_snap_resize_free(g.link);
    g.link = NULL;
    g.nmem = 0;
    if (zone_now >= 0) {
        zone_now = -1;
        zone_pending = 1;
    }
}

/* A node's frame in screen pixels (the node is the frame enlarged by `inset` per side). */
static AxiaSnapMember member_of(Node *o, float inset, float min_w, float min_h, float max_w, float max_h, int slot) {
    float left = dim_or(o->s.pos[EDGE_L], o->x), top = dim_or(o->s.pos[EDGE_T], o->y);
    float w = dim_or(o->s.width, o->w), h = dim_or(o->s.height, o->h);
    g.mem[slot].inset = inset;
    g.mem[slot].ox = o->ax - o->x;
    g.mem[slot].oy = o->ay - o->y;
    return (AxiaSnapMember){
        .frame = { g.mem[slot].ox + left + inset, g.mem[slot].oy + top + inset, w - 2 * inset, h - 2 * inset },
        .min_w = min_w, .min_h = min_h, .max_w = max_w, .max_h = max_h,
    };
}

/* Resizing: the windows that share the dragged edge follow it. Without any, the plain resize. */
static void link_setup(Node *n, const GrabLimits *lim) {
    AxiaSnapMember m[GRAB_LINKS + 1];
    int k = 0;
    g.mem[0].node = g.node;
    m[k++] = member_of(n, lim->inset, lim->min_w, lim->min_h, lim->max_w, lim->max_h, 0);
    for (int i = 0; i < lim->nlinks && i < GRAB_LINKS; i++) {
        int id = lim->links[i].node;
        Node *o = id == g.node ? NULL : node_get(id);
        if (!o || !o->s.absolute) continue;
        g.mem[k].node = id;
        m[k] = member_of(o, lim->links[i].inset, lim->links[i].min_w, lim->links[i].min_h, lim->links[i].max_w, lim->links[i].max_h, k);
        k++;
    }
    if (k < 2) return;
    g.link = axia_snap_resize_new(g.edges, m, (size_t)k, lim->area);
    if (!g.link) return;
    g.nmem = k;
    for (int i = 0; i < k; i++) g.start[i] = g.out[i] = m[i].frame;
}

static void set_node_frame(int i, AxiaSnapRect f) {
    Node *o = node_get(g.mem[i].node);
    if (!o) return;
    float in = g.mem[i].inset;
    float x = roundf((float)f.x - in - g.mem[i].ox), y = roundf((float)f.y - in - g.mem[i].oy);
    float w = roundf((float)f.w + 2 * in), h = roundf((float)f.h + 2 * in);
    float cx = dim_or(o->s.pos[EDGE_L], o->x), cy = dim_or(o->s.pos[EDGE_T], o->y);
    float cw = dim_or(o->s.width, o->w), ch = dim_or(o->s.height, o->h);
    if (x == cx && y == cy && w == cw && h == ch) return;
    o->s.pos[EDGE_L] = (Dim){ x, UNIT_PX };
    o->s.pos[EDGE_T] = (Dim){ y, UNIT_PX };
    node_moved(o);
    if (w != cw || h != ch) {
        o->s.width = (Dim){ w, UNIT_PX };
        o->s.height = (Dim){ h, UNIT_PX };
        node_invalidate(o);
    }
    if (i == 0) { g.x = x; g.y = y; g.w = w; g.h = h; }
    ui.dirty = 1;
}

static void link_motion(float dx, float dy) {
    size_t k = axia_snap_resize_apply(g.link, dx, dy, g.out, (size_t)g.nmem);
    for (size_t i = 0; i < k; i++) set_node_frame((int)i, g.out[i]);
}

void grab_note_shift(int held) {
    held = held != 0;
    if (held == shift_held) return;
    shift_held = held;
    if (g.active) zone_check(ptr_x, ptr_y);
}

int grab_zone_take(int *zone, AxiaSnapRect *rect) {
    if (!zone_pending) return 0;
    zone_pending = 0;
    *zone = zone_now;
    *rect = zone_rect;
    return 1;
}

static int setup(int node, int edges, const GrabLimits *lim) {
    Node *n = node_get(node);
    if (!n || !n->s.absolute) return -1;
    if (g.active) grab_cancel();
    memset(&g, 0, sizeof g);
    g.active = 1;
    g.node = node;
    g.edges = edges & 15;
    g.lim = *lim;
    g.px0 = ptr_x;
    g.py0 = ptr_y;
    g.x = g.x0 = dim_or(n->s.pos[EDGE_L], n->x);
    g.y = g.y0 = dim_or(n->s.pos[EDGE_T], n->y);
    g.w = g.w0 = dim_or(n->s.width, n->w);
    g.h = g.h0 = dim_or(n->s.height, n->h);
    g.ox = n->ax - n->x;
    g.oy = n->ay - n->y;
    if (!g.edges && lim->nsnap > 0) {
        g.drag = axia_snap_drag_new(lim->snap, (size_t)(lim->nsnap < GRAB_SNAP_DISPLAYS ? lim->nsnap : GRAB_SNAP_DISPLAYS));
        zone_check(ptr_x, ptr_y);
    }
    if (g.edges && lim->nlinks > 0) link_setup(n, lim);
    /* the layer keeps what is below one window: windows resizing together repaint as usual */
    if (!g.link) paint_layer_begin(node, g.edges != 0);
    return 0;
}

int grab_begin(int node, int edges, const GrabLimits *lim) {
    if (!ptr_buttons) return -1;
    script_off();
    return setup(node, edges, lim);
}

int grab_script_begin(int node, const GrabLimits *lim, float rx, float ry, float period_ms) {
    if (rx < 0 || ry < 0 || period_ms < 500) return -1;
    if (setup(node, 0, lim)) return -1;
    sc.on = 1;
    sc.t0 = -1;
    sc.rx = rx;
    sc.ry = ry;
    sc.period = period_ms;
    lbi_start(rx, ry, period_ms);           /* if it fails, the clock drives it (no input latency) */
    ui.dirty = 1;
    return 0;
}

int grab_scripted(void) { return sc.on && g.active; }

/* a figure of eight that starts and ends where the window was (lbi_path) */
void grab_script_tick(double now_ms) {
    if (!sc.on) return;
    if (!g.active) { script_off(); return; }
    float dx, dy;
    if (lbi_fd() >= 0) {                    /* the synthetic pointer's newest position, if one came */
        if (lbi_take(&dx, &dy)) grab_motion(g.px0 + dx, g.py0 + dy);
        return;
    }
    if (sc.t0 < 0) sc.t0 = now_ms;
    lbi_path(sc.rx, sc.ry, sc.period, now_ms - sc.t0, &dx, &dy);
    grab_motion(g.px0 + dx, g.py0 + dy);
}

static float clampf(float v, float lo, float hi) { return v < lo ? lo : v > hi ? hi : v; }

void grab_motion(float px, float py) {
    if (!g.active) return;
    Node *n = node_get(g.node);
    if (!n) { grab_cancel(); return; }
    float dx = roundf(px - g.px0), dy = roundf(py - g.py0), in = g.lim.inset;
    if (g.link) { link_motion(dx, dy); return; }
    if (!g.edges) zone_check(px, py);       /* before the frame's clamping: the pointer decides */
    float x = g.x0, y = g.y0, w = g.w0, h = g.h0;
    /* frame bounds in node coordinates (the parent may not be at 0,0) */
    float top = g.lim.min_top - g.oy - in, sw = ui.width - g.ox, sh = ui.height - g.oy;
    if (!g.edges) {
        x = clampf(g.x0 + dx, KEEP - (g.w0 - in), sw - KEEP - in);
        /* the work area of the display under the frame's center (its own menu bar, or none) */
        float cx = g.ox + x + g.w0 / 2;
        for (int i = 0; i < g.lim.nareas; i++)
            if (cx >= g.lim.ax[i] && cx < g.lim.ax[i] + g.lim.aw[i]) { top = g.lim.ay[i] - g.oy - in; break; }
        y = clampf(g.y0 + dy, top, sh - KEEP - in);
    } else {
        float minw = g.lim.min_w + 2 * in, minh = g.lim.min_h + 2 * in;
        float maxw = g.lim.max_w > 0 ? g.lim.max_w + 2 * in : 1e9f, maxh = g.lim.max_h > 0 ? g.lim.max_h + 2 * in : 1e9f;
        if (g.edges & 8) w = clampf(g.w0 + dx, minw, fminf(maxw, sw + in - g.x0));
        if (g.edges & 4) {
            w = clampf(g.w0 - dx, minw, fminf(maxw, g.x0 + g.w0 + in));
            x = g.x0 + g.w0 - w;
        }
        if (g.edges & 2) h = clampf(g.h0 + dy, minh, fminf(maxh, sh + in - g.y0));
        if (g.edges & 1) {
            h = clampf(g.h0 - dy, minh, fminf(maxh, g.y0 + g.h0 - top));
            y = g.y0 + g.h0 - h;
        }
    }
    if (x == g.x && y == g.y && w == g.w && h == g.h) return;
    int sized = w != g.w || h != g.h;
    g.x = x; g.y = y; g.w = w; g.h = h;
    n->s.pos[EDGE_L] = (Dim){ x, UNIT_PX };
    n->s.pos[EDGE_T] = (Dim){ y, UNIT_PX };
    node_moved(n);
    if (sized) {
        n->s.width = (Dim){ w, UNIT_PX };
        n->s.height = (Dim){ h, UNIT_PX };
        node_invalidate(n);         /* the content gets laid out again */
    }
    ui.dirty = 1;                   /* move: only the position layout, without repainting the content */
}

int grab_finish(GrabEnd *e) {
    if (!g.active || ptr_buttons) return 0;
    if (sc.on) { grab_cancel(); return 0; }         /* the person moved the pointer: the scene stops where it is */
    memset(e, 0, sizeof *e);
    e->node = g.node;
    e->edges = g.edges;
    e->dx = g.x - g.x0;
    e->dy = g.y - g.y0;
    e->dw = g.w - g.w0;
    e->dh = g.h - g.h0;
    e->zone = -1;
    if (g.drag) {                                   /* where the button was released */
        zone_check(ptr_x, ptr_y);
        e->zone = axia_snap_drag_zone(g.drag, NULL);
    }
    for (int i = 1; g.link && i < g.nmem && e->nlinks < GRAB_LINKS; i++) {
        if (!axia_snap_resize_moves(g.link, (size_t)i)) continue;
        e->links[e->nlinks].node = g.mem[i].node;
        e->links[e->nlinks].dx = (float)(g.out[i].x - g.start[i].x);
        e->links[e->nlinks].dy = (float)(g.out[i].y - g.start[i].y);
        e->links[e->nlinks].dw = (float)(g.out[i].w - g.start[i].w);
        e->links[e->nlinks].dh = (float)(g.out[i].h - g.start[i].h);
        e->nlinks++;
    }
    g.active = 0;
    snap_release();
    paint_layer_end();
    return 1;
}

void grab_cancel(void) {
    script_off();
    if (!g.active) return;
    g.active = 0;
    snap_release();
    paint_layer_end();
}
