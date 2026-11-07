/* Independent Wayland peer: exercise the real C protocol/Rust storage boundary.
 * Reuse upstream's registry, window and pipe helpers, not compositor internals. */
#define main clipboard_offer_reference_main
#include "clip_offers.c"
#undef main
#include <signal.h>

static int stall_next, stalled_fd = -1;
static int just_focused(void) { return focused; }
static void settle(void) { for (int i = 0; i < 12; i++) { wl_display_roundtrip(dpy); usleep(20000); } }
typedef struct { const char *mime; const unsigned char *bytes; size_t size; } Payload;
static void peer_send(void *data, struct wl_data_source *source, const char *mime, int32_t fd) {
    (void)source;
    Payload *p = data;
    if (stall_next) { stall_next = 0; stalled_fd = fd; return; }
    const unsigned char *bytes = !strcmp(mime, "application/x-axia-test") ? (const unsigned char *)"rich content" : p->bytes;
    size_t size = !strcmp(mime, "application/x-axia-test") ? 12 : p->size;
    int flags = fcntl(fd, F_GETFL); if (flags >= 0) fcntl(fd, F_SETFL, flags & ~O_NONBLOCK);
    size_t sent = 0;
    while (sent < size) { ssize_t n = write(fd, bytes + sent, size - sent); if (n > 0) sent += (size_t)n; else if (n < 0 && errno == EINTR) continue; else break; }
    close(fd);
}
static void peer_cancelled(void *data, struct wl_data_source *source) { (void)data; (void)source; }
static const struct wl_data_source_listener peer_source = { .send = peer_send, .cancelled = peer_cancelled };
static void primary_send(void *data, struct zwp_primary_selection_source_v1 *source, const char *mime, int32_t fd) { (void)source; peer_send(data, NULL, mime, fd); }
static void primary_cancelled(void *data, struct zwp_primary_selection_source_v1 *source) { (void)data; (void)source; }
static const struct zwp_primary_selection_source_v1_listener primary_source = { .send = primary_send, .cancelled = primary_cancelled };
static size_t receive_offer(struct wl_data_offer *offer, const char *mime, char *out, size_t cap) {
    if (!offer) return 0;
    int p[2]; if (pipe(p)) return 0;
    wl_data_offer_receive(offer, mime, p[1]); close(p[1]); wl_display_flush(dpy); wl_display_roundtrip(dpy);
    return read_pipe(p[0], out, cap);
}
static void require(int condition, const char *message) { if (!condition) {fprintf(stderr, "clipboard native protocol: %s\n", message); exit(1);} }
int main(void) {
    signal(SIGPIPE, SIG_IGN);
    setvbuf(stdout, NULL, _IOLBF, 0);
    dpy = wl_display_connect(NULL); require(dpy != NULL, "connect");
    struct wl_registry *registry = wl_display_get_registry(dpy);
    wl_registry_add_listener(registry, &reg, NULL); wl_display_roundtrip(dpy);
    require(comp && shm && wm && seat && ddm && pdm, "globals");
    xdg_wm_base_add_listener(wm, &wm_l, NULL);
    struct wl_keyboard *kb = wl_seat_get_keyboard(seat); wl_keyboard_add_listener(kb, &kb_l, NULL);
    struct wl_data_device *dev = wl_data_device_manager_get_data_device(ddm, seat); wl_data_device_add_listener(dev, &dev_l, NULL);
    struct zwp_primary_selection_device_v1 *pdev = zwp_primary_selection_device_manager_v1_get_device(pdm, seat); zwp_primary_selection_device_v1_add_listener(pdev, &pdev_l, NULL);
    struct wl_surface *surface = wl_compositor_create_surface(comp);
    struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm, surface); xdg_surface_add_listener(xs, &xs_l, NULL);
    struct xdg_toplevel *top = xdg_surface_get_toplevel(xs); xdg_toplevel_add_listener(top, &top_l, NULL);
    xdg_toplevel_set_title(top, "Native clipboard protocol test"); xdg_toplevel_set_app_id(top, "axia.clipboard.test"); wl_surface_commit(surface);
    while (!configured && wl_display_dispatch(dpy) >= 0) {}
    int fd = memfd_create("clipboard-native-test", MFD_CLOEXEC); require(fd >= 0 && ftruncate(fd, 240 * 120 * 4) == 0, "shm file");
    unsigned *pixels = mmap(NULL, 240 * 120 * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0); require(pixels != MAP_FAILED, "map");
    for (int i = 0; i < 240 * 120; i++) pixels[i] = 0xff3260a0;
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, 240 * 120 * 4);
    struct wl_buffer *buffer = wl_shm_pool_create_buffer(pool, 0, 240, 120, 240 * 4, WL_SHM_FORMAT_XRGB8888);
    wl_surface_attach(surface, buffer, 0, 0); wl_surface_commit(surface); require(wait_for(just_focused, 10), "window keyboard focus");
    size_t size = 1024 * 1024;
    unsigned char *large = malloc(size); char *out = malloc(size + 64); require(large && out, "test buffers");
    for (size_t i = 0; i < size; i++) large[i] = (unsigned char)('a' + i % 26);
    Payload text = {"text/plain;charset=utf-8", large, size};
    struct wl_data_source *source = wl_data_device_manager_create_data_source(ddm); wl_data_source_add_listener(source, &peer_source, &text);
    wl_data_source_offer(source, text.mime); wl_data_source_offer(source, "application/x-axia-test"); wl_data_device_set_selection(dev, source, 0); settle();
    struct wl_data_offer *old = clip_cur;
    require(receive_offer(clip_cur, "application/x-axia-test", out, size + 64) == 12 && !memcmp(out, "rich content", 12), "rich MIME forwarded to owner");
    wl_data_source_destroy(source); settle();
    require(clip_cur && clip_cur != old, "retained text offered after source destruction");
    require(receive_offer(old, text.mime, out, size + 64) == 0, "old offer cannot read newer generation");
    require(receive_offer(clip_cur, text.mime, out, size + 64) == size && !memcmp(out, large, size), "native large text writer survives owner exit");
    /* A source can retain its pipe without writing. Replacing it must close
     * the stale reader even though that reader never becomes readable. */
    stall_next = 1;
    source = wl_data_device_manager_create_data_source(ddm); wl_data_source_add_listener(source, &peer_source, &text);
    wl_data_source_offer(source, text.mime); wl_data_device_set_selection(dev, source, 0); settle();
    require(stalled_fd >= 0, "stalled source received its descriptor");
    struct wl_data_source *replacement = wl_data_device_manager_create_data_source(ddm); wl_data_source_add_listener(replacement, &peer_source, &text);
    wl_data_source_offer(replacement, text.mime); wl_data_device_set_selection(dev, replacement, 0); settle();
    errno = 0; require(write(stalled_fd, "x", 1) < 0 && errno == EPIPE, "replaced idle transfer was retired");
    close(stalled_fd); stalled_fd = -1; wl_data_source_destroy(source); wl_data_source_destroy(replacement); settle();
    Payload primary = {"text/plain;charset=utf-8", (const unsigned char *)"primary selection", 17};
    struct zwp_primary_selection_source_v1 *ps = zwp_primary_selection_device_manager_v1_create_source(pdm); zwp_primary_selection_source_v1_add_listener(ps, &primary_source, &primary);
    zwp_primary_selection_source_v1_offer(ps, primary.mime); zwp_primary_selection_device_v1_set_selection(pdev, ps, 0); settle();
    require(prim_cur && read_prim(prim_cur, out, size + 64) == primary.size && !memcmp(out, primary.bytes, primary.size), "primary selection uses independent storage");
    zwp_primary_selection_source_v1_destroy(ps); settle();
    Payload secret = {"text/plain;charset=utf-8", (const unsigned char *)"password test", 13};
    source = wl_data_device_manager_create_data_source(ddm); wl_data_source_add_listener(source, &peer_source, &secret); wl_data_source_offer(source, secret.mime); wl_data_source_offer(source, "x-kde-passwordManagerHint"); wl_data_device_set_selection(dev, source, 0); settle();
    wl_data_source_destroy(source); settle();
    require(receive_offer(clip_cur, secret.mime, out, size + 64) == secret.size && !memcmp(out, secret.bytes, secret.size), "sensitive text can still paste");
    require(receive_offer(clip_cur, "x-kde-passwordManagerHint", out, size + 64) == 6 && !memcmp(out, "secret", 6), "sensitive hint survives owner exit");
    const unsigned char png[] = {137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1,8,6,0,0,0,31,21,196,137,0,0,0,13,73,68,65,84,120,156,99,80,72,88,240,31,0,3,228,2,32,123,134,14,220,0,0,0,0,73,69,78,68,174,66,96,130};
    Payload image = {"image/png", png, sizeof png};
    old = clip_cur;
    source = wl_data_device_manager_create_data_source(ddm); wl_data_source_add_listener(source, &peer_source, &image); wl_data_source_offer(source, image.mime); wl_data_device_set_selection(dev, source, 0); settle();
    wl_data_source_destroy(source); settle();
    require(receive_offer(old, secret.mime, out, size + 64) == 0, "stale secret offer invalidated by image copy");
    require(receive_offer(clip_cur, image.mime, out, size + 64) == image.size && !memcmp(out, image.bytes, image.size), "PNG retained and delivered by native engine");
    require(read_prim(prim_cur, out, size + 64) == primary.size && !memcmp(out, primary.bytes, primary.size), "clipboard updates preserve primary selection");
    free(out); free(large); wl_display_disconnect(dpy);
    puts("clipboard native protocol: passed"); return 0;
}
