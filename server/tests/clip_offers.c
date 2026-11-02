/* clip_offers.c — a Wayland client that checks that clipboard and primary-selection offers are
 * bound to the selection they were made for (docs/PRIVACY.md, "X11 apps"; wl.c clip_offer_gen and
 * prim_gen): after the selection changes, reading through an OLD offer must give nothing, and the
 * new offer must give the new text. Run by tests/xwayland/clip_offers.sh inside a LordOS session,
 * which changes both selections from an X11 app (xclip) while this window keeps the keyboard.
 *
 *   make -C server build/clip-offers
 *   build/clip-offers NEW_CLIPBOARD NEW_PRIMARY      (prints READY, then the results)
 *
 * Exit status 0 when every check passed. */
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "client/xdg-shell-client-protocol.h"
#include "client/primary-selection-unstable-v1-client-protocol.h"

static struct wl_compositor *comp;
static struct wl_shm *shm;
static struct xdg_wm_base *wm;
static struct wl_seat *seat;
static struct wl_data_device_manager *ddm;
static struct zwp_primary_selection_device_manager_v1 *pdm;
static int focused, configured;
static struct wl_data_offer *clip_cur;
static struct zwp_primary_selection_offer_v1 *prim_cur;
static int clip_changes, prim_changes;

static void reg_global(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t ver) {
    (void)d; (void)ver;
    if (!strcmp(iface, wl_compositor_interface.name)) comp = wl_registry_bind(r, name, &wl_compositor_interface, 4);
    else if (!strcmp(iface, wl_shm_interface.name)) shm = wl_registry_bind(r, name, &wl_shm_interface, 1);
    else if (!strcmp(iface, xdg_wm_base_interface.name)) wm = wl_registry_bind(r, name, &xdg_wm_base_interface, 1);
    else if (!strcmp(iface, wl_seat_interface.name)) seat = wl_registry_bind(r, name, &wl_seat_interface, 1);
    else if (!strcmp(iface, wl_data_device_manager_interface.name)) ddm = wl_registry_bind(r, name, &wl_data_device_manager_interface, 3);
    else if (!strcmp(iface, zwp_primary_selection_device_manager_v1_interface.name))
        pdm = wl_registry_bind(r, name, &zwp_primary_selection_device_manager_v1_interface, 1);
}
static void reg_remove(void *d, struct wl_registry *r, uint32_t name) { (void)d; (void)r; (void)name; }
static const struct wl_registry_listener reg = { reg_global, reg_remove };

static void wm_ping(void *d, struct xdg_wm_base *b, uint32_t serial) { (void)d; xdg_wm_base_pong(b, serial); }
static const struct xdg_wm_base_listener wm_l = { wm_ping };
static void xs_configure(void *d, struct xdg_surface *s, uint32_t serial) { (void)d; xdg_surface_ack_configure(s, serial); configured = 1; }
static const struct xdg_surface_listener xs_l = { xs_configure };
static void top_configure(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *st) { (void)d; (void)t; (void)w; (void)h; (void)st; }
static void top_close(void *d, struct xdg_toplevel *t) { (void)d; (void)t; }
static const struct xdg_toplevel_listener top_l = { .configure = top_configure, .close = top_close };

static void kb_keymap(void *d, struct wl_keyboard *k, uint32_t f, int32_t fd, uint32_t n) { (void)d; (void)k; (void)f; (void)n; close(fd); }
static void kb_enter(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *sf, struct wl_array *keys) { (void)d; (void)k; (void)s; (void)sf; (void)keys; focused = 1; }
static void kb_leave(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *sf) { (void)d; (void)k; (void)s; (void)sf; focused = 0; }
static void kb_key(void *d, struct wl_keyboard *k, uint32_t s, uint32_t t, uint32_t key, uint32_t st) { (void)d; (void)k; (void)s; (void)t; (void)key; (void)st; }
static void kb_mods(void *d, struct wl_keyboard *k, uint32_t s, uint32_t a, uint32_t b, uint32_t c, uint32_t g) { (void)d; (void)k; (void)s; (void)a; (void)b; (void)c; (void)g; }
static const struct wl_keyboard_listener kb_l = { kb_keymap, kb_enter, kb_leave, kb_key, kb_mods, NULL };

static void off_offer(void *d, struct wl_data_offer *o, const char *m) { (void)d; (void)o; (void)m; }
static const struct wl_data_offer_listener off_l = { .offer = off_offer };
static void dev_offer(void *d, struct wl_data_device *dv, struct wl_data_offer *o) { (void)d; (void)dv; wl_data_offer_add_listener(o, &off_l, NULL); }
static void dev_enter(void *d, struct wl_data_device *dv, uint32_t s, struct wl_surface *sf, wl_fixed_t x, wl_fixed_t y, struct wl_data_offer *o) { (void)d; (void)dv; (void)s; (void)sf; (void)x; (void)y; (void)o; }
static void dev_leave(void *d, struct wl_data_device *dv) { (void)d; (void)dv; }
static void dev_motion(void *d, struct wl_data_device *dv, uint32_t t, wl_fixed_t x, wl_fixed_t y) { (void)d; (void)dv; (void)t; (void)x; (void)y; }
static void dev_drop(void *d, struct wl_data_device *dv) { (void)d; (void)dv; }
/* old offers are kept (not destroyed) on purpose: the test reads through them later */
static struct wl_data_offer *clip_old;
static void dev_selection(void *d, struct wl_data_device *dv, struct wl_data_offer *o) {
    (void)d; (void)dv;
    if (!o) return;
    clip_cur = o;
    clip_changes++;
}
static const struct wl_data_device_listener dev_l = { dev_offer, dev_enter, dev_leave, dev_motion, dev_drop, dev_selection };

static void poff_offer(void *d, struct zwp_primary_selection_offer_v1 *o, const char *m) { (void)d; (void)o; (void)m; }
static const struct zwp_primary_selection_offer_v1_listener poff_l = { poff_offer };
static void pdev_offer(void *d, struct zwp_primary_selection_device_v1 *dv, struct zwp_primary_selection_offer_v1 *o) { (void)d; (void)dv; zwp_primary_selection_offer_v1_add_listener(o, &poff_l, NULL); }
static struct zwp_primary_selection_offer_v1 *prim_old;
static void pdev_selection(void *d, struct zwp_primary_selection_device_v1 *dv, struct zwp_primary_selection_offer_v1 *o) {
    (void)d; (void)dv;
    if (!o) return;
    prim_cur = o;
    prim_changes++;
}
static const struct zwp_primary_selection_device_v1_listener pdev_l = { pdev_offer, pdev_selection };

static struct wl_display *dpy;

static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return (double)t.tv_sec + t.tv_nsec / 1e9; }

/* Dispatches until cond() or the timeout. */
static int wait_for(int (*cond)(void), double secs) {
    double end = now() + secs;
    while (!cond() && now() < end) {
        wl_display_flush(dpy);
        struct pollfd p = { wl_display_get_fd(dpy), POLLIN, 0 };
        if (poll(&p, 1, 100) > 0) { if (wl_display_dispatch(dpy) < 0) return 0; }
        else wl_display_dispatch_pending(dpy);
    }
    return cond();
}

static int ready_cond(void) { return focused && clip_cur && prim_cur; }
static int base_clip, base_prim;
static int changed_cond(void) { return clip_changes > base_clip && prim_changes > base_prim; }

/* Reads what the offer gives (at most size-1 bytes; EOF or 2 s). */
static size_t read_pipe(int fd, char *buf, size_t size) {
    size_t n = 0;
    double end = now() + 2;
    while (n + 1 < size && now() < end) {
        wl_display_flush(dpy);
        wl_display_dispatch_pending(dpy);
        struct pollfd p = { fd, POLLIN, 0 };
        if (poll(&p, 1, 50) <= 0) continue;
        ssize_t r = read(fd, buf + n, size - 1 - n);
        if (r <= 0) break;
        n += (size_t)r;
    }
    buf[n] = 0;
    close(fd);
    return n;
}
static size_t read_clip(struct wl_data_offer *o, char *buf, size_t size) {
    int p[2];
    if (pipe(p)) return 0;
    wl_data_offer_receive(o, "text/plain;charset=utf-8", p[1]);
    close(p[1]);
    wl_display_flush(dpy);
    return read_pipe(p[0], buf, size);
}
static size_t read_prim(struct zwp_primary_selection_offer_v1 *o, char *buf, size_t size) {
    int p[2];
    if (pipe(p)) return 0;
    zwp_primary_selection_offer_v1_receive(o, "text/plain;charset=utf-8", p[1]);
    close(p[1]);
    wl_display_flush(dpy);
    return read_pipe(p[0], buf, size);
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: clip-offers NEW_CLIPBOARD NEW_PRIMARY\n"); return 2; }
    setvbuf(stdout, NULL, _IOLBF, 0);
    dpy = wl_display_connect(NULL);
    if (!dpy) { fprintf(stderr, "no Wayland\n"); return 2; }
    struct wl_registry *r = wl_display_get_registry(dpy);
    wl_registry_add_listener(r, &reg, NULL);
    wl_display_roundtrip(dpy);
    if (!comp || !shm || !wm || !seat || !ddm || !pdm) { fprintf(stderr, "a global is missing\n"); return 2; }
    xdg_wm_base_add_listener(wm, &wm_l, NULL);
    struct wl_keyboard *kb = wl_seat_get_keyboard(seat);
    wl_keyboard_add_listener(kb, &kb_l, NULL);
    struct wl_data_device *dev = wl_data_device_manager_get_data_device(ddm, seat);
    wl_data_device_add_listener(dev, &dev_l, NULL);
    struct zwp_primary_selection_device_v1 *pdev = zwp_primary_selection_device_manager_v1_get_device(pdm, seat);
    zwp_primary_selection_device_v1_add_listener(pdev, &pdev_l, NULL);
    /* a small window, so the compositor gives it the keyboard */
    struct wl_surface *s = wl_compositor_create_surface(comp);
    struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm, s);
    xdg_surface_add_listener(xs, &xs_l, NULL);
    struct xdg_toplevel *top = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(top, &top_l, NULL);
    xdg_toplevel_set_title(top, "Clipboard offers test");
    xdg_toplevel_set_app_id(top, "clip-offers");
    wl_surface_commit(s);
    while (!configured && wl_display_dispatch(dpy) >= 0) {}
    int w = 240, h = 120, stride = w * 4;
    int fd = memfd_create("clip-offers", MFD_CLOEXEC);
    if (fd < 0 || ftruncate(fd, stride * h)) return 2;
    uint32_t *px = mmap(NULL, (size_t)stride * h, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (px == MAP_FAILED) return 2;
    for (int i = 0; i < w * h; i++) px[i] = 0xff3060a0;
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, stride * h);
    struct wl_buffer *buf = wl_shm_pool_create_buffer(pool, 0, w, h, stride, WL_SHM_FORMAT_XRGB8888);
    wl_surface_attach(s, buf, 0, 0);
    wl_surface_commit(s);
    if (!wait_for(ready_cond, 10)) { printf("FAIL: no keyboard focus or no initial offers (focused %d)\n", focused); return 1; }
    clip_old = clip_cur;
    prim_old = prim_cur;
    base_clip = clip_changes;
    base_prim = prim_changes;
    printf("READY\n");
    if (!wait_for(changed_cond, 15)) { printf("FAIL: the selections didn't change\n"); return 1; }
    int fails = 0;
    char got[4096];
    struct wl_data_offer *oldc = clip_old;
    struct zwp_primary_selection_offer_v1 *oldp = prim_old;
    size_t n = oldc ? read_clip(oldc, got, sizeof got) : 0;
    printf("%s old clipboard offer gives nothing (%zu bytes)\n", n ? "FAIL:" : "ok:", n);
    fails += n != 0 || !oldc;
    n = read_clip(clip_cur, got, sizeof got);
    printf("%s new clipboard offer gives \"%s\"\n", strcmp(got, argv[1]) ? "FAIL:" : "ok:", got);
    fails += strcmp(got, argv[1]) != 0;
    n = oldp ? read_prim(oldp, got, sizeof got) : 0;
    printf("%s old primary offer gives nothing (%zu bytes)\n", n ? "FAIL:" : "ok:", n);
    fails += n != 0 || !oldp;
    n = read_prim(prim_cur, got, sizeof got);
    printf("%s new primary offer gives \"%s\"\n", strcmp(got, argv[2]) ? "FAIL:" : "ok:", got);
    fails += strcmp(got, argv[2]) != 0;
    printf("%s\n", fails ? "FAILED" : "PASSED");
    return fails ? 1 : 0;
}
