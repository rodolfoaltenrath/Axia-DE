#include "wayland_input_fixture.h"
#include "wl/wl_input.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

extern const struct wl_interface wl_surface_interface, wl_seat_interface, wl_pointer_interface;
WliPeer wli_peers[2];
WliEvent wli_events[262144];
unsigned wli_nevents;
unsigned long long wli_digest = 14695981039346656037ULL;
uint64_t wli_time_ms = 123456;
int wli_focus = -1, wli_hit = -1;
const unsigned wli_versions[6] = {1, 4, 5, 7, 8, 9};
static struct wl_display *display;
static struct wl_list pointers;
static void hash(const void *data, size_t n) {
    const unsigned char *p = data;
    while (n--) { wli_digest ^= *p++; wli_digest *= 1099511628211ULL; }
}

int __real_clock_gettime(clockid_t clock, struct timespec *t);
int __wrap_clock_gettime(clockid_t clock, struct timespec *t) {
    if (clock != CLOCK_MONOTONIC) return __real_clock_gettime(clock, t);
    *t = (struct timespec){.tv_sec = wli_time_ms / 1000, .tv_nsec = (wli_time_ms % 1000) * 1000000};
    return 0;
}
struct wl_display *wl__display(void) { return display; }
struct wl_resource *wl__pointer_focus(void) {
    return wli_focus >= 0 && wli_peers[wli_focus].alive ? wli_peers[wli_focus].surface : NULL;
}
struct wl_list *wl__pointers(void) { return &pointers; }
struct wl_resource *wl__surface_at(float x, float y, float *lx, float *ly) {
    if (wli_hit < 0 || !wli_peers[wli_hit].alive) return NULL;
    *lx = x - (wli_hit ? 600 : 100); *ly = y - 50;
    return wli_peers[wli_hit].surface;
}
static void pointer_gone(struct wl_resource *r) { wl_list_remove(wl_resource_get_link(r)); }
static void destroy(struct wl_client *c, struct wl_resource *r) { (void)c; wl_resource_destroy(r); }
static void set_cursor(struct wl_client *c, struct wl_resource *r, uint32_t serial, struct wl_resource *s, int32_t x, int32_t y) {
    (void)c; (void)r; (void)serial; (void)s; (void)x; (void)y;
}
static const struct { void (*cursor)(struct wl_client *, struct wl_resource *, uint32_t, struct wl_resource *, int32_t, int32_t);
    void (*release)(struct wl_client *, struct wl_resource *); } pointer_impl = {set_cursor, destroy};

static void send_bytes(WliPeer *p, unsigned object, unsigned opcode, const void *args, unsigned size) {
    uint32_t data[256] = {object, ((size + 8) << 16) | opcode};
    assert(size % 4 == 0 && size + 8 <= sizeof data);
    if (size) memcpy(data + 2, args, size);
    assert(send(p->fd, data, size + 8, MSG_NOSIGNAL) == (ssize_t)(size + 8));
}
void wli_request(unsigned peer, unsigned object, unsigned opcode, const uint32_t *args, unsigned n) {
    send_bytes(&wli_peers[peer], object, opcode, args, n * 4);
}
static void drain(unsigned index) {
    WliPeer *p = &wli_peers[index];
    ssize_t n;
    while ((n = recv(p->fd, p->pending + p->used, sizeof p->pending - p->used, MSG_DONTWAIT)) > 0) {
        p->used += n;
        size_t at = 0;
        while (p->used - at >= 8) {
            uint32_t head[2]; memcpy(head, p->pending + at, 8);
            unsigned object = head[0], opcode = head[1] & 65535, size = head[1] >> 16;
            assert(size >= 8 && size <= sizeof p->pending && size % 4 == 0);
            if (size > p->used - at) break;
            const unsigned char *a = p->pending + at + 8;
            assert(object != 1 || opcode != 0); /* protocol error */
            if (object == 2) {
                assert(opcode == 0 && size >= 20);
                uint32_t name, len, version;
                memcpy(&name, a, 4); memcpy(&len, a + 4, 4);
                assert(len && size == 20 + ((len + 3) & ~3u));
                memcpy(&version, a + 8 + ((len + 3) & ~3u), 4);
                if (!strcmp((const char *)a + 8, "zwp_pointer_gestures_v1")) { assert(version == 3); p->global = name; }
            } else if (object != 1) {
                assert((size - 8) / 4 <= 8 && wli_nevents < sizeof wli_events / sizeof *wli_events);
                WliEvent *e = &wli_events[wli_nevents++];
                *e = (WliEvent){.peer=index, .object=object, .opcode=opcode, .n=(size - 8) / 4};
                memcpy(e->args, a, size - 8);
                hash(e, sizeof *e);
            }
            at += size;
        }
        memmove(p->pending, p->pending + at, p->used - at); p->used -= at;
        assert(p->used < sizeof p->pending);
    }
    assert(n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK));
}
void wli_pump(void) {
    assert(wl_event_loop_dispatch(wl_display_get_event_loop(display), 0) == 0);
    wl_display_flush_clients(display);
    for (unsigned i = 0; i < 2; i++) if (wli_peers[i].alive) drain(i);
}
void wli_bind_manager(unsigned index, unsigned id, unsigned version) {
    WliPeer *p = &wli_peers[index];
    const char *name = "zwp_pointer_gestures_v1";
    uint32_t len = strlen(name) + 1, padded = (len + 3) & ~3u;
    unsigned char bind[128] = {0};
    memcpy(bind, &p->global, 4); memcpy(bind + 4, &len, 4); memcpy(bind + 8, name, len);
    memcpy(bind + 8 + padded, &version, 4); memcpy(bind + 12 + padded, &id, 4);
    send_bytes(p, 2, 0, bind, 16 + padded); wli_pump();
}
static void peer(unsigned index) {
    WliPeer *p = &wli_peers[index];
    int pair[2]; assert(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, pair) == 0);
    p->fd = pair[1]; p->client = wl_client_create(display, pair[0]); p->alive = 1; assert(p->client);
    uint32_t registry = 2; wli_request(index, 1, 1, &registry, 1); wli_pump(); assert(p->global);
    wli_bind_manager(index, 3, 3);
    p->seat = wl_resource_create(p->client, &wl_seat_interface, 9, 4); assert(p->seat);
    wl_input_seat_caps(p->seat);
    for (unsigned k = 0; k < 6; k++) {
        struct wl_resource *r = wl_resource_create(p->client, &wl_pointer_interface, wli_versions[k], 5 + k); assert(r);
        wl_resource_set_implementation(r, &pointer_impl, NULL, pointer_gone);
        wl_list_insert(&pointers, wl_resource_get_link(r)); p->pointers[k] = r;
    }
    p->surface = wl_resource_create(p->client, &wl_surface_interface, 1, 11); assert(p->surface);
    for (unsigned k = 0; k < 3; k++) {
        uint32_t args[] = {12 + k, 10}; wli_request(index, 3, k == 2 ? 3 : k, args, 2);
    }
    wli_pump();
    wl_input_get_touch(p->client, p->seat, 15);
    uint32_t args[] = {16, 10}; wli_request(index, 3, 0, args, 2); wli_pump();
    wl_input_get_touch(p->client, p->seat, 17); wli_pump();
}
void wli_init(void) {
    display = wl_display_create(); assert(display); wl_list_init(&pointers);
    assert(!wl_input_has_focus());
    wl_input_devices_changed();
    assert(!wl_input_touch(1, 0, 200, 100));
    wl_input_globals(display); peer(0); peer(1);
}
const WliEvent *wli_last(unsigned peer, unsigned object, unsigned opcode) {
    for (unsigned k = wli_nevents; k; k--) {
        const WliEvent *e = &wli_events[k - 1];
        if (e->peer == peer && e->object == object && e->opcode == opcode) return e;
    }
    return NULL;
}
void wli_finish_peer(unsigned index) {
    WliPeer *p = &wli_peers[index]; if (!p->alive) return;
    wl_client_destroy(p->client); close(p->fd); p->alive = 0;
}
void wli_finish(void) {
    wli_finish_peer(0); wli_finish_peer(1);
    assert(wl_list_empty(&pointers)); wl_display_destroy(display);
}
