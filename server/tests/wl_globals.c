/* wl_globals.c — prints every global the Wayland compositor offers to a client, one interface
 * name per line (tests/fwlinux/screen.sh checks that none of them lets an app copy the screen).
 *
 *   WAYLAND_DISPLAY=wayland-0 build/wl-globals */
#include <stdio.h>
#include <wayland-client.h>

static void global(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t version) {
    (void)d; (void)r; (void)name;
    printf("%s %u\n", iface, version);
}
static void global_remove(void *d, struct wl_registry *r, uint32_t name) { (void)d; (void)r; (void)name; }
static const struct wl_registry_listener listener = { global, global_remove };

int main(void) {
    struct wl_display *dpy = wl_display_connect(NULL);
    if (!dpy) { fprintf(stderr, "wl-globals: no Wayland display\n"); return 2; }
    struct wl_registry *reg = wl_display_get_registry(dpy);
    wl_registry_add_listener(reg, &listener, NULL);
    wl_display_roundtrip(dpy);
    wl_display_disconnect(dpy);
    return 0;
}
