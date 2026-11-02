/* keylag-client.c — a minimal Wayland client for the key latency harness (docs/TROUBLESHOOTING.md).
 *
 * A 320x200 window that repaints itself in a new colour on every key press and asks for
 * presentation feedback for that frame. It prints, in CLOCK_MONOTONIC ms:
 *
 *   client key t=81234.901          wl_keyboard.key (press) arrived
 *   client shown t=81251.300        the frame drawn for it was presented
 *
 * (no key identity is printed, like the compositor's trace)
 *
 * Build (libwayland-dev and wayland-protocols; the two protocol headers come from wayland-scanner):
 *   P=/usr/share/wayland-protocols
 *   wayland-scanner client-header $P/stable/xdg-shell/xdg-shell.xml xdg-shell-client-protocol.h
 *   wayland-scanner private-code  $P/stable/xdg-shell/xdg-shell.xml xdg-shell-protocol.c
 *   wayland-scanner client-header $P/stable/presentation-time/presentation-time.xml presentation-time-client-protocol.h
 *   wayland-scanner private-code  $P/stable/presentation-time/presentation-time.xml presentation-time-protocol.c
 *   cc -O2 -I. -o keylag-client keylag-client.c xdg-shell-protocol.c presentation-time-protocol.c -lwayland-client
 * Run inside a LordOS session: XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0 ./keylag-client */
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>

#include "xdg-shell-client-protocol.h"
#include "presentation-time-client-protocol.h"

#define W 320
#define H 200

static struct wl_compositor *comp;
static struct wl_shm *shm;
static struct xdg_wm_base *wm;
static struct wl_seat *seat;
static struct wp_presentation *pres;
static struct wl_surface *surf;
static struct wl_buffer *buf[2];
static uint32_t *px[2];
static int cur, configured;

static double mono_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

static void fb_sync(void *d, struct wp_presentation_feedback *f, struct wl_output *o) { (void)d; (void)f; (void)o; }
static void fb_presented(void *d, struct wp_presentation_feedback *f, uint32_t hi, uint32_t lo, uint32_t ns, uint32_t refresh,
                         uint32_t shi, uint32_t slo, uint32_t flags) {
    (void)refresh; (void)shi; (void)slo; (void)flags;
    double t = ((double)(((uint64_t)hi << 32) | lo)) * 1e3 + ns / 1e6;
    (void)d;
    printf("client shown t=%.3f\n", t);
    fflush(stdout);
    wp_presentation_feedback_destroy(f);
}
static void fb_discarded(void *d, struct wp_presentation_feedback *f) {
    (void)d;
    printf("client discarded t=%.3f\n", mono_ms());
    fflush(stdout);
    wp_presentation_feedback_destroy(f);
}
static const struct wp_presentation_feedback_listener fb_l = { fb_sync, fb_presented, fb_discarded };

static void draw(int code) {
    cur ^= 1;
    uint32_t c = 0xff000000u | ((uint32_t)(code * 2654435761u) & 0x00ffffffu);
    for (int i = 0; i < W * H; i++) px[cur][i] = c;
    wl_surface_attach(surf, buf[cur], 0, 0);
    wl_surface_damage(surf, 0, 0, W, H);
    if (pres) wp_presentation_feedback_add_listener(wp_presentation_feedback(pres, surf), &fb_l, (void *)(intptr_t)code);
    wl_surface_commit(surf);
}

static void kb_keymap(void *d, struct wl_keyboard *k, uint32_t f, int fd, uint32_t s) { (void)d; (void)k; (void)f; (void)s; close(fd); }
static void kb_enter(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *sf, struct wl_array *a) { (void)d; (void)k; (void)s; (void)sf; (void)a; fprintf(stderr, "keylag-client: keyboard focus\n"); }
static void kb_leave(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *sf) { (void)d; (void)k; (void)s; (void)sf; }
static void kb_key(void *d, struct wl_keyboard *k, uint32_t serial, uint32_t time, uint32_t key, uint32_t state) {
    (void)d; (void)k; (void)serial; (void)time;
    if (state != WL_KEYBOARD_KEY_STATE_PRESSED) return;
    printf("client key t=%.3f\n", mono_ms());
    fflush(stdout);
    draw((int)key);
}
static void kb_mods(void *d, struct wl_keyboard *k, uint32_t s, uint32_t a, uint32_t b, uint32_t c, uint32_t g) { (void)d; (void)k; (void)s; (void)a; (void)b; (void)c; (void)g; }
static void kb_repeat(void *d, struct wl_keyboard *k, int32_t r, int32_t dl) { (void)d; (void)k; (void)r; (void)dl; }
static const struct wl_keyboard_listener kb_l = { kb_keymap, kb_enter, kb_leave, kb_key, kb_mods, kb_repeat };

static void wm_ping(void *d, struct xdg_wm_base *w, uint32_t s) { (void)d; xdg_wm_base_pong(w, s); }
static const struct xdg_wm_base_listener wm_l = { wm_ping };
static void xs_configure(void *d, struct xdg_surface *x, uint32_t s) {
    (void)d;
    xdg_surface_ack_configure(x, s);
    if (!configured) { configured = 1; draw(0); }
}
static const struct xdg_surface_listener xs_l = { xs_configure };
static void top_configure(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *s) { (void)d; (void)t; (void)w; (void)h; (void)s; }
static void top_close(void *d, struct xdg_toplevel *t) { (void)d; (void)t; exit(0); }
static const struct xdg_toplevel_listener top_l = { top_configure, top_close, NULL, NULL };

static void reg_global(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t ver) {
    (void)d;
    if (!strcmp(iface, wl_compositor_interface.name)) comp = wl_registry_bind(r, name, &wl_compositor_interface, 4);
    else if (!strcmp(iface, wl_shm_interface.name)) shm = wl_registry_bind(r, name, &wl_shm_interface, 1);
    else if (!strcmp(iface, xdg_wm_base_interface.name)) { wm = wl_registry_bind(r, name, &xdg_wm_base_interface, 1); xdg_wm_base_add_listener(wm, &wm_l, NULL); }
    else if (!strcmp(iface, wl_seat_interface.name)) seat = wl_registry_bind(r, name, &wl_seat_interface, ver < 5 ? ver : 5);
    else if (!strcmp(iface, wp_presentation_interface.name)) pres = wl_registry_bind(r, name, &wp_presentation_interface, 1);
}
static void reg_remove(void *d, struct wl_registry *r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener reg_l = { reg_global, reg_remove };

int main(void) {
    struct wl_display *dpy = wl_display_connect(NULL);
    if (!dpy) { fprintf(stderr, "keylag-client: no Wayland display\n"); return 1; }
    wl_registry_add_listener(wl_display_get_registry(dpy), &reg_l, NULL);
    wl_display_roundtrip(dpy);
    if (!comp || !shm || !wm || !seat) { fprintf(stderr, "keylag-client: missing globals\n"); return 1; }
    if (!pres) fprintf(stderr, "keylag-client: no wp_presentation (no 'shown' lines)\n");
    int fd = memfd_create("keylag", MFD_CLOEXEC);
    size_t one = (size_t)W * H * 4;
    if (fd < 0 || ftruncate(fd, (off_t)(one * 2))) return 1;
    uint32_t *map = mmap(NULL, one * 2, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (map == MAP_FAILED) return 1;
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, (int32_t)(one * 2));
    for (int i = 0; i < 2; i++) {
        px[i] = map + (size_t)i * W * H;
        buf[i] = wl_shm_pool_create_buffer(pool, (int32_t)(i * one), W, H, W * 4, WL_SHM_FORMAT_XRGB8888);
    }
    wl_keyboard_add_listener(wl_seat_get_keyboard(seat), &kb_l, NULL);
    surf = wl_compositor_create_surface(comp);
    struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm, surf);
    xdg_surface_add_listener(xs, &xs_l, NULL);
    struct xdg_toplevel *top = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(top, &top_l, NULL);
    xdg_toplevel_set_title(top, "keylag");
    wl_surface_commit(surf);
    while (wl_display_dispatch(dpy) >= 0) {}
    return 0;
}
