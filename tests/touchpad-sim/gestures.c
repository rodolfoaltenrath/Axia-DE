/* gestures.c — scripted gestures for the simulated touchpad (see gestures.h). Positions are in
 * mm from the touchpad's top-left corner; each frame lasts 12.5 ms (80 Hz). */
#include "gestures.h"

#include <linux/input.h>
#include <math.h>
#include <string.h>

typedef struct { double x, y; int p; } Pt;

#define P_FINGER 60
#define P_PALM 250

static void ev(SimGen *g, int type, int code, int value) { g->emit(g->ud, g->t, type, code, value); }

void sim_init(SimGen *g, SimEmit emit, void *ud) {
    memset(g, 0, sizeof *g);
    g->emit = emit;
    g->ud = ud;
    g->next_id = 100;
    g->frame_ms = 12.5;
    for (int i = 0; i < SIM_SLOTS; i++) g->id[i] = -1;
}

void sim_wait(SimGen *g, double ms) { g->t += ms; }

static int ux(double mm) { int v = (int)lround(SIM_X_MIN + mm * SIM_RES); return v < SIM_X_MIN ? SIM_X_MIN : v > SIM_X_MAX ? SIM_X_MAX : v; }
static int uy(double mm) { int v = (int)lround(SIM_Y_MIN + mm * SIM_RES); return v < SIM_Y_MIN ? SIM_Y_MIN : v > SIM_Y_MAX ? SIM_Y_MAX : v; }

/* One frame with n contacts (in slots 0..n-1) and the clickpad button. */
static void frame(SimGen *g, int n, const Pt *pts, int button) {
    static const int tools[] = { 0, BTN_TOOL_FINGER, BTN_TOOL_DOUBLETAP, BTN_TOOL_TRIPLETAP, BTN_TOOL_QUADTAP, BTN_TOOL_QUINTTAP };
    for (int i = 0; i < SIM_SLOTS; i++) {
        int want = i < n;
        if (want) {
            int x = ux(pts[i].x), y = uy(pts[i].y), p = pts[i].p;
            if (!g->down[i]) {
                ev(g, EV_ABS, ABS_MT_SLOT, i);
                g->id[i] = g->next_id++;
                ev(g, EV_ABS, ABS_MT_TRACKING_ID, g->id[i]);
                ev(g, EV_ABS, ABS_MT_POSITION_X, x);
                ev(g, EV_ABS, ABS_MT_POSITION_Y, y);
                ev(g, EV_ABS, ABS_MT_PRESSURE, p);
            } else if (x != g->x[i] || y != g->y[i] || p != g->p[i]) {
                ev(g, EV_ABS, ABS_MT_SLOT, i);
                if (x != g->x[i]) ev(g, EV_ABS, ABS_MT_POSITION_X, x);
                if (y != g->y[i]) ev(g, EV_ABS, ABS_MT_POSITION_Y, y);
                if (p != g->p[i]) ev(g, EV_ABS, ABS_MT_PRESSURE, p);
            }
            g->down[i] = 1;
            g->x[i] = x;
            g->y[i] = y;
            g->p[i] = p;
        } else if (g->down[i]) {
            ev(g, EV_ABS, ABS_MT_SLOT, i);
            ev(g, EV_ABS, ABS_MT_TRACKING_ID, -1);
            g->down[i] = 0;
            g->id[i] = -1;
        }
    }
    /* single-finger emulation (like the kernel): the first contact */
    if (n > 0) {
        ev(g, EV_ABS, ABS_X, g->x[0]);
        ev(g, EV_ABS, ABS_Y, g->y[0]);
        ev(g, EV_ABS, ABS_PRESSURE, g->p[0]);
    }
    int tool = n > 5 ? 5 : n;
    if (tool != g->tool) {
        if (g->tool) ev(g, EV_KEY, tools[g->tool], 0);
        if (!g->tool && tool) ev(g, EV_KEY, BTN_TOUCH, 1);
        if (tool) ev(g, EV_KEY, tools[tool], 1);
        else ev(g, EV_KEY, BTN_TOUCH, 0);
        g->tool = tool;
    }
    if (button != g->button) { ev(g, EV_KEY, BTN_LEFT, button); g->button = button; }
    ev(g, EV_SYN, SYN_REPORT, 0);
    g->t += g->frame_ms;
}

static double ease(double u) { return u * u * (3 - 2 * u); }

/* n fingers from from[] to to[] in ms (with smooth acceleration and deceleration). */
static void stroke(SimGen *g, int n, const Pt *from, const Pt *to, double ms, int button) {
    int frames = (int)(ms / g->frame_ms);
    if (frames < 1) frames = 1;
    Pt p[SIM_SLOTS];
    for (int k = 0; k <= frames; k++) {
        double u = ease((double)k / frames);
        for (int i = 0; i < n; i++) {
            p[i].x = from[i].x + (to[i].x - from[i].x) * u;
            p[i].y = from[i].y + (to[i].y - from[i].y) * u;
            p[i].p = from[i].p;
        }
        frame(g, n, p, button);
    }
}

/* the same but at constant speed (for a scroll released while moving) */
static void stroke_linear(SimGen *g, int n, const Pt *from, const Pt *to, double ms) {
    int frames = (int)(ms / g->frame_ms);
    Pt p[SIM_SLOTS];
    for (int k = 0; k <= frames; k++) {
        double u = (double)k / frames;
        for (int i = 0; i < n; i++) {
            p[i].x = from[i].x + (to[i].x - from[i].x) * u;
            p[i].y = from[i].y + (to[i].y - from[i].y) * u;
            p[i].p = from[i].p;
        }
        frame(g, n, p, 0);
    }
}

static void hold(SimGen *g, int n, const Pt *at, double ms, int button) { stroke(g, n, at, at, ms, button); }
static void lift(SimGen *g) { frame(g, 0, NULL, g->button); }

/* ---- gestures ---- */

static void g_move(SimGen *g) {
    Pt a[] = { { 30, 45, P_FINGER } }, b[] = { { 75, 52, P_FINGER } };
    stroke(g, 1, a, b, 400, 0);
    lift(g);
}
static void g_move_back(SimGen *g) {
    Pt a[] = { { 75, 52, P_FINGER } }, b[] = { { 30, 45, P_FINGER } };
    stroke(g, 1, a, b, 400, 0);
    lift(g);
}
static void tapn(SimGen *g, int n) {
    Pt a[] = { { 42, 40, P_FINGER }, { 56, 41, P_FINGER }, { 70, 42, P_FINGER } };
    hold(g, n, a, 50, 0);
    lift(g);
}
static void g_tap(SimGen *g) { tapn(g, 1); sim_wait(g, 300); }
static void g_tap2(SimGen *g) { tapn(g, 2); sim_wait(g, 300); }
static void g_tap3(SimGen *g) { tapn(g, 3); sim_wait(g, 300); }
static void g_doubletap(SimGen *g) { tapn(g, 1); sim_wait(g, 70); tapn(g, 1); sim_wait(g, 300); }
static void g_tapdrag(SimGen *g) {
    tapn(g, 1);
    sim_wait(g, 70);
    Pt a[] = { { 42, 40, P_FINGER } }, b[] = { { 80, 50, P_FINGER } };
    stroke(g, 1, a, b, 450, 0);
    lift(g);
    sim_wait(g, 300);
}
static void clickn(SimGen *g, int n) {
    Pt a[] = { { 42, 40, P_FINGER }, { 56, 41, P_FINGER }, { 70, 42, P_FINGER } };
    hold(g, n, a, 40, 0);
    hold(g, n, a, 80, 1);
    hold(g, n, a, 30, 0);
    lift(g);
}
static void g_click(SimGen *g) { clickn(g, 1); }
static void g_click2(SimGen *g) { clickn(g, 2); }
static void g_click3(SimGen *g) { clickn(g, 3); }
static void g_thumbclick(SimGen *g) {          /* thumb resting at the bottom + index: left click */
    Pt a[] = { { 50, 82, P_FINGER }, { 45, 40, P_FINGER } };
    hold(g, 2, a, 60, 0);
    hold(g, 2, a, 80, 1);
    hold(g, 2, a, 30, 0);
    lift(g);
}
static void g_scroll(SimGen *g) {             /* two fingers up, fast, released while moving */
    Pt a[] = { { 45, 65, P_FINGER }, { 60, 66, P_FINGER } }, b[] = { { 45, 30, P_FINGER }, { 60, 31, P_FINGER } };
    stroke_linear(g, 2, a, b, 220);
    lift(g);
    sim_wait(g, 1500);
}
static void g_scroll_down(SimGen *g) {
    Pt a[] = { { 45, 30, P_FINGER }, { 60, 31, P_FINGER } }, b[] = { { 45, 65, P_FINGER }, { 60, 66, P_FINGER } };
    stroke_linear(g, 2, a, b, 220);
    lift(g);
    sim_wait(g, 1500);
}
static void g_scroll_slow(SimGen *g) {        /* slow, stopping before release: no momentum */
    Pt a[] = { { 45, 60, P_FINGER }, { 60, 61, P_FINGER } }, b[] = { { 45, 40, P_FINGER }, { 60, 41, P_FINGER } };
    stroke(g, 2, a, b, 800, 0);
    hold(g, 2, b, 200, 0);
    lift(g);
    sim_wait(g, 300);
}
static void g_pinch_out(SimGen *g) {
    Pt a[] = { { 46, 45, P_FINGER }, { 56, 40, P_FINGER } }, b[] = { { 22, 60, P_FINGER }, { 80, 26, P_FINGER } };
    stroke(g, 2, a, b, 500, 0);
    lift(g);
    sim_wait(g, 200);
}
static void g_pinch_in(SimGen *g) {
    Pt a[] = { { 22, 60, P_FINGER }, { 80, 26, P_FINGER } }, b[] = { { 46, 45, P_FINGER }, { 56, 40, P_FINGER } };
    stroke(g, 2, a, b, 500, 0);
    lift(g);
    sim_wait(g, 200);
}
static void g_rotate(SimGen *g) {             /* two fingers rotating 60° around the center */
    int frames = 40;
    for (int k = 0; k <= frames; k++) {
        double ang = (-20 + 60.0 * ease((double)k / frames)) * M_PI / 180;
        Pt p[2] = { { 52 - 15 * cos(ang), 44 - 15 * sin(ang), P_FINGER }, { 52 + 15 * cos(ang), 44 + 15 * sin(ang), P_FINGER } };
        frame(g, 2, p, 0);
    }
    lift(g);
    sim_wait(g, 200);
}
static void swipe(SimGen *g, int n, double dx, double dy) {
    Pt a[4] = { { 32, 55, P_FINGER }, { 46, 52, P_FINGER }, { 60, 52, P_FINGER }, { 74, 55, P_FINGER } }, b[4];
    for (int i = 0; i < n; i++) { b[i] = a[i]; b[i].x += dx; b[i].y += dy; }
    stroke(g, n, a, b, 300, 0);
    lift(g);
    sim_wait(g, 300);
}
static void g_swipe3_up(SimGen *g) { swipe(g, 3, 0, -35); }
static void g_swipe3_down(SimGen *g) { swipe(g, 3, 0, 30); }
static void g_swipe3_left(SimGen *g) { swipe(g, 3, -28, 0); }
static void g_swipe3_right(SimGen *g) { swipe(g, 3, 28, 0); }
static void g_swipe4_up(SimGen *g) { swipe(g, 4, 0, -35); }
static void g_palm(SimGen *g) {               /* palm: high-pressure contact on the edge */
    Pt a[] = { { 101, 40, P_PALM } }, b[] = { { 100, 70, P_PALM } };
    stroke(g, 1, a, b, 600, 0);
    lift(g);
    sim_wait(g, 300);
}
static void g_edge(SimGen *g) {               /* a touch that stays on the left edge */
    Pt a[] = { { 3, 25, P_FINGER } }, b[] = { { 3, 65, P_FINGER } };
    hold(g, 1, a, 320, 0);
    stroke(g, 1, a, b, 500, 0);
    lift(g);
    sim_wait(g, 300);
}
static void g_threedrag(SimGen *g) {          /* with "three-finger drag" */
    Pt a[] = { { 40, 50, P_FINGER }, { 54, 47, P_FINGER }, { 68, 50, P_FINGER } }, b[3];
    for (int i = 0; i < 3; i++) { b[i] = a[i]; b[i].x += 30; b[i].y -= 10; }
    stroke(g, 3, a, b, 500, 0);
    lift(g);
    sim_wait(g, 700);
}

static const SimGesture list[] = {
    { "move", "1 finger: move the pointer right", g_move },
    { "move-back", "1 finger: move the pointer left", g_move_back },
    { "tap", "tap with 1 finger (click)", g_tap },
    { "tap2", "tap with 2 fingers (secondary click)", g_tap2 },
    { "tap3", "tap with 3 fingers (middle click)", g_tap3 },
    { "doubletap", "double tap", g_doubletap },
    { "tapdrag", "tap and drag", g_tapdrag },
    { "click", "physical click with 1 finger", g_click },
    { "click2", "physical click with 2 fingers (secondary)", g_click2 },
    { "click3", "physical click with 3 fingers (middle)", g_click3 },
    { "thumbclick", "thumb resting at the bottom + index: left click", g_thumbclick },
    { "scroll", "2 fingers up, fast (momentum)", g_scroll },
    { "scroll-down", "2 fingers down, fast", g_scroll_down },
    { "scroll-slow", "2 fingers slowly, no momentum", g_scroll_slow },
    { "pinch-out", "spread 2 fingers (zoom in)", g_pinch_out },
    { "pinch-in", "pinch 2 fingers (zoom out)", g_pinch_in },
    { "rotate", "rotate 2 fingers 60°", g_rotate },
    { "swipe3-up", "3 fingers up", g_swipe3_up },
    { "swipe3-down", "3 fingers down", g_swipe3_down },
    { "swipe3-left", "3 fingers left", g_swipe3_left },
    { "swipe3-right", "3 fingers right", g_swipe3_right },
    { "swipe4-up", "4 fingers up", g_swipe4_up },
    { "palm", "palm on the right edge (ignored)", g_palm },
    { "edge", "still touch on the left edge (ignored)", g_edge },
    { "threedrag", "3-finger drag", g_threedrag },
};

const SimGesture *sim_gestures(int *n) { *n = (int)(sizeof list / sizeof list[0]); return list; }

const SimGesture *sim_find(const char *name) {
    for (size_t i = 0; i < sizeof list / sizeof list[0]; i++) if (!strcmp(list[i].name, name)) return &list[i];
    return NULL;
}
