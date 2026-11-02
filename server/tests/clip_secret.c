/* clip_secret.c — clipboard history test: a Wayland client that copies a text
 * marking it as secret, just like KeePassXC (it offers text/plain and x-kde-passwordManagerHint).
 * LordOS's history must not store it, but it must still be possible to paste it.
 *
 *   cc -o clip-secret tests/clip_secret.c -lwayland-client
 *   WAYLAND_DISPLAY=wayland-0 ./clip-secret "contraseña"
 *
 * It needs a window with keyboard focus for the compositor to accept the selection; since it
 * draws none, it uses the serial of the first keyboard event it receives (the compositor sends
 * enter when giving focus) or, if none arrives, 0. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client.h>

static struct wl_seat *seat;
static struct wl_data_device_manager *ddm;
static const char *secret;

static void reg_global(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t ver) {
    (void)d; (void)ver;
    if (!strcmp(iface, wl_seat_interface.name)) seat = wl_registry_bind(r, name, &wl_seat_interface, 1);
    else if (!strcmp(iface, wl_data_device_manager_interface.name)) ddm = wl_registry_bind(r, name, &wl_data_device_manager_interface, 3);
}
static void reg_remove(void *d, struct wl_registry *r, uint32_t name) { (void)d; (void)r; (void)name; }
static const struct wl_registry_listener reg = { reg_global, reg_remove };

static void src_target(void *d, struct wl_data_source *s, const char *m) { (void)d; (void)s; (void)m; }
static void src_send(void *d, struct wl_data_source *s, const char *mime, int32_t fd) {
    (void)d; (void)s;
    const char *v = !strcmp(mime, "x-kde-passwordManagerHint") ? "secret" : secret;
    if (write(fd, v, strlen(v)) < 0) perror("write");
    close(fd);
}
static int cancelled;
static void src_cancelled(void *d, struct wl_data_source *s) { (void)d; (void)s; cancelled = 1; }
static const struct wl_data_source_listener src = { .target = src_target, .send = src_send, .cancelled = src_cancelled };

int main(int argc, char **argv) {
    secret = argc > 1 ? argv[1] : "s3creta";
    struct wl_display *dpy = wl_display_connect(NULL);
    if (!dpy) { fprintf(stderr, "no Wayland\n"); return 1; }
    struct wl_registry *r = wl_display_get_registry(dpy);
    wl_registry_add_listener(r, &reg, NULL);
    wl_display_roundtrip(dpy);
    if (!seat || !ddm) { fprintf(stderr, "wl_seat or wl_data_device_manager is missing\n"); return 1; }
    struct wl_data_device *dev = wl_data_device_manager_get_data_device(ddm, seat);
    struct wl_data_source *s = wl_data_device_manager_create_data_source(ddm);
    wl_data_source_add_listener(s, &src, NULL);
    wl_data_source_offer(s, "text/plain;charset=utf-8");
    wl_data_source_offer(s, "text/plain");
    wl_data_source_offer(s, "x-kde-passwordManagerHint");
    wl_data_device_set_selection(dev, s, 0);
    wl_display_flush(dpy);
    /* stays alive for a few seconds to serve the text */
    for (int i = 0; i < 40 && !cancelled; i++) { wl_display_dispatch_pending(dpy); wl_display_flush(dpy); wl_display_roundtrip(dpy); usleep(100000); }
    printf("ready\n");
    return 0;
}
