/* grab.c — window dragging in C: move and resize without JS per frame (grab.h). */
#include "grab.h"
#include "../cursor.h"
#include "../bench/lbinput.h"

#include <math.h>
#include <string.h>

/* Where the node goes is decided in Rust (rust/axia-shell/src/snap/grab.rs); this file owns the nodes. */
typedef struct { int node; Dim l, t, w, h; } Saved;

static struct {
    int active, node, edges;
    float px0, py0;                 /* pointer at start */
    AxiaGrab *move;                 /* the node's geometry while moved or resized alone (Rust) */
    GrabLimits lim;
    /* what the theme had set on each node the drag moves, put back when it ends (restore_nodes) */
    int nsaved;
    Saved saved[GRAB_LINKS + 1];
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

/* ---- the nodes the drag moves ---- */

static void save_node(Node *o, int id) {
    if (!o || g.nsaved > GRAB_LINKS) return;
    for (int i = 0; i < g.nsaved; i++)
        if (g.saved[i].node == id) return;
    g.saved[g.nsaved++] = (Saved){ id, o->s.pos[EDGE_L], o->s.pos[EDGE_T], o->s.width, o->s.height };
}

static int same_dim(Dim a, Dim b) { return a.v == b.v && a.unit == b.unit; }

/* The drag is over: every node it moved goes back to what the theme had set. The renderer sends a
 * node only the styles that changed since its own last render, so a node left where the drag put it
 * kept that place whenever the window list gave the same style as before: a window snapped to the
 * side it came from stayed at its loose size, one maximized again stayed small where it was dropped,
 * and the next drag was measured from a node that wasn't where the window list had it (a window
 * could end up above the menu bar). main.c calls grab_restore() right before js_wm_end, whose jobs
 * run before the next frame, so the node is drawn only where the window list puts it. */
static void restore_nodes(void) {
    for (int i = 0; i < g.nsaved; i++) {
        Saved *v = &g.saved[i];
        Node *o = node_get(v->node);
        if (!o) continue;
        int sized = !same_dim(o->s.width, v->w) || !same_dim(o->s.height, v->h);
        if (!sized && same_dim(o->s.pos[EDGE_L], v->l) && same_dim(o->s.pos[EDGE_T], v->t)) continue;
        o->s.pos[EDGE_L] = v->l;
        o->s.pos[EDGE_T] = v->t;
        node_moved(o);
        if (sized) {
            o->s.width = v->w;
            o->s.height = v->h;
            node_invalidate(o);
        }
        ui.dirty = 1;
    }
    g.nsaved = 0;
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
    save_node(o, g.mem[slot].node);
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
    if (g.active || g.nsaved) grab_cancel();
    AxiaGrabStart st = {
        .node = { dim_or(n->s.pos[EDGE_L], n->x), dim_or(n->s.pos[EDGE_T], n->y), dim_or(n->s.width, n->w), dim_or(n->s.height, n->h) },
        .origin_x = n->ax - n->x, .origin_y = n->ay - n->y,
        .pointer_x = ptr_x, .pointer_y = ptr_y,
        .edges = edges & 15,
        .inset = lim->inset, .min_w = lim->min_w, .min_h = lim->min_h, .max_w = lim->max_w, .max_h = lim->max_h,
        .min_top = lim->min_top,
        .detach_w = lim->detach_w, .detach_h = lim->detach_h,
    };
    for (int i = 0; i < lim->nareas && i < 4; i++) st.areas[st.nareas++] = (AxiaGrabArea){ lim->ax[i], lim->aw[i], lim->ay[i] };
    AxiaGrab *move = axia_grab_new(&st);
    if (!move) return -1;
    memset(&g, 0, sizeof g);
    g.active = 1;
    g.node = node;
    g.edges = edges & 15;
    g.lim = *lim;
    g.px0 = ptr_x;
    g.py0 = ptr_y;
    g.move = move;
    save_node(n, node);
    if (!g.edges && lim->nsnap > 0) {
        g.drag = axia_snap_drag_new(lim->snap, (size_t)(lim->nsnap < GRAB_SNAP_DISPLAYS ? lim->nsnap : GRAB_SNAP_DISPLAYS));
        if (!axia_grab_waiting(g.move)) zone_check(ptr_x, ptr_y);   /* a click on a maximized window's bar shows no zone */
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

void grab_motion(float px, float py) {
    if (!g.active) return;
    Node *n = node_get(g.node);
    if (!n) { grab_cancel(); return; }
    if (g.link) { link_motion(roundf(px - g.px0), roundf(py - g.py0)); return; }
    AxiaSnapRect r;
    int moved = axia_grab_motion(g.move, px, py, ui.width, ui.height, &r);
    if (axia_grab_waiting(g.move)) return;  /* a maximized or snapped window still where it was */
    if (!g.edges) zone_check(px, py);       /* the pointer decides, not the clamped frame */
    /* came loose: the layer held the window at its old size, painted again at the new one */
    if (moved & AXIA_GRAB_DETACHED) { paint_layer_end(); paint_layer_begin(g.node, 0); }
    if (!(moved & AXIA_GRAB_MOVED)) return;
    n->s.pos[EDGE_L] = (Dim){ (float)r.x, UNIT_PX };
    n->s.pos[EDGE_T] = (Dim){ (float)r.y, UNIT_PX };
    node_moved(n);
    if (moved & AXIA_GRAB_SIZED) {
        n->s.width = (Dim){ (float)r.w, UNIT_PX };
        n->s.height = (Dim){ (float)r.h, UNIT_PX };
        node_invalidate(n);         /* the content gets laid out again */
    }
    ui.dirty = 1;                   /* move: only the position layout, without repainting the content */
}

/* the drag's state goes with it; its nodes go back to the theme's in grab_restore (or at once on cancel) */
static void end_drag(void) {
    g.active = 0;
    snap_release();
    axia_grab_free(g.move);
    g.move = NULL;
    paint_layer_end();
}

void grab_restore(void) { restore_nodes(); }

int grab_finish(GrabEnd *e) {
    if (!g.active || ptr_buttons) return 0;
    if (sc.on) { grab_cancel(); return 0; }         /* the person moved the pointer: the scene stops, the window back where it was */
    memset(e, 0, sizeof *e);
    e->node = g.node;
    e->edges = g.edges;
    AxiaSnapRect d = { 0 };
    if (g.link) d = (AxiaSnapRect){ g.out[0].x - g.start[0].x, g.out[0].y - g.start[0].y, g.out[0].w - g.start[0].w, g.out[0].h - g.start[0].h };
    else e->detached = axia_grab_end(g.move, &d);
    e->dx = (float)d.x;
    e->dy = (float)d.y;
    e->dw = (float)d.w;
    e->dh = (float)d.h;
    e->zone = -1;
    if (g.drag && !axia_grab_waiting(g.move)) {                      /* where the button was released */
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
    end_drag();
    return 1;
}

void grab_cancel(void) {
    script_off();
    if (g.active) end_drag();
    restore_nodes();
}
