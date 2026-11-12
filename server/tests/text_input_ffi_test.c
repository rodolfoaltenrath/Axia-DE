/* Real libwayland-server dispatch over socketpairs, with an independent wire client.
 * Run unchanged against the baseline C implementation as well as the Rust owner. */
#include "wl/wl_input.h"
#include <wayland-server-core.h>
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

extern const struct wl_interface wl_surface_interface, wl_seat_interface;
typedef struct { int on, x, y, w, h; } Report;
static Report reports[8192];
static unsigned nr, messages;
static unsigned long long digest = 14695981039346656037ULL;
static void hash(const void *data, size_t n) {
    const unsigned char *p = data;
    while (n--) { digest ^= *p++; digest *= 1099511628211ULL; }
}
static void report(int on, int x, int y, int w, int h) {
    assert(nr < sizeof reports / sizeof *reports);
    reports[nr++] = (Report){on, x, y, w, h};
    hash(&reports[nr - 1], sizeof *reports);
}
static void last(int on, int x, int y, int w, int h) {
    Report expected = {on, x, y, w, h};
    assert(nr && !memcmp(&reports[nr - 1], &expected, sizeof expected));
}

typedef struct {
    int fd;
    struct wl_client *client;
    struct wl_resource *surface;
    unsigned global, enters, leaves, input_events;
} Peer;

static struct wl_display *display;
static void request(Peer *p, unsigned object, unsigned op, const void *args, size_t n) {
    unsigned char bytes[1024];
    assert(n % 4 == 0 && n + 8 <= sizeof bytes);
    uint32_t head[2] = {object, ((uint32_t)n + 8) << 16 | op};
    memcpy(bytes, head, 8);
    if (n) memcpy(bytes + 8, args, n);
    assert(send(p->fd, bytes, n + 8, MSG_NOSIGNAL) == (ssize_t)(n + 8));
}
static void empty(Peer *p, unsigned object, unsigned op) { request(p, object, op, NULL, 0); }
static void drain(Peer *p) {
    unsigned char bytes[8192];
    ssize_t n;
    while ((n = recv(p->fd, bytes, sizeof bytes, MSG_DONTWAIT)) > 0) {
        size_t at = 0;
        while (at < (size_t)n) {
            uint32_t object, word;
            assert((size_t)n - at >= 8);
            memcpy(&object, bytes + at, 4); memcpy(&word, bytes + at + 4, 4);
            unsigned size = word >> 16, opcode = word & 65535;
            assert(size >= 8 && at + size <= (size_t)n);
            const unsigned char *a = bytes + at + 8;
            if (object == 2 && opcode == 0) {
                unsigned name, len, version;
                assert(size >= 20);
                memcpy(&name, a, 4); memcpy(&len, a + 4, 4);
                assert(len && 16 + ((len + 3) & ~3u) + 4 == size);
                memcpy(&version, a + 8 + ((len + 3) & ~3u), 4);
                if (!strcmp((const char *)a + 8, "zwp_text_input_manager_v3")) {
                    assert(version == 1); p->global = name;
                }
            } else if (object == 5 || object == 7) {
                assert(size == 12 && opcode <= 1); /* no text, done or other events */
                unsigned surface; memcpy(&surface, a, 4); assert(surface == 6);
                p->input_events++;
                if (opcode == 0) p->enters++; else p->leaves++;
                messages++;
                hash(bytes + at, size);
            } else assert(object != 1 || opcode != 0); /* no protocol errors */
            at += size;
        }
    }
    assert(n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK));
}
static void pump(Peer *a, Peer *b) {
    assert(wl_event_loop_dispatch(wl_display_get_event_loop(display), 0) == 0);
    wl_display_flush_clients(display);
    drain(a); if (b) drain(b);
}
static Peer peer(void) {
    int pair[2]; assert(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, pair) == 0);
    Peer p = {.fd = pair[1], .client = wl_client_create(display, pair[0])};
    assert(p.client);
    uint32_t registry = 2; request(&p, 1, 1, &registry, 4);
    pump(&p, NULL); assert(p.global);
    const char *interface = "zwp_text_input_manager_v3";
    uint32_t len = (uint32_t)strlen(interface) + 1, padded = (len + 3) & ~3u;
    unsigned char bind[128] = {0};
    memcpy(bind, &p.global, 4); memcpy(bind + 4, &len, 4); memcpy(bind + 8, interface, len);
    uint32_t version = 1, manager = 3;
    memcpy(bind + 8 + padded, &version, 4); memcpy(bind + 12 + padded, &manager, 4);
    request(&p, 2, 0, bind, 16 + padded);
    pump(&p, NULL);
    assert(wl_resource_create(p.client, &wl_seat_interface, 1, 4));
    uint32_t get[] = {5, 4}; request(&p, 3, 1, get, sizeof get);
    pump(&p, NULL);
    p.surface = wl_resource_create(p.client, &wl_surface_interface, 1, 6); assert(p.surface);
    return p;
}
static void rect(Peer *p, unsigned input, int x, int y, int w, int h) {
    int32_t r[] = {x, y, w, h}; request(p, input, 6, r, sizeof r);
}
static void commit(Peer *p, unsigned input) { empty(p, input, 7); }

int main(void) {
    display = wl_display_create(); assert(display);
    wl_textinput_globals(display, report);
    Peer a = peer(), b = peer();
    assert(!a.input_events && !b.input_events && !nr);
    empty(&a, 5, 1); rect(&a, 5, 10, -20, 30, 40); commit(&a, 5);
    pump(&a, &b); assert(!nr); /* no keyboard focus */
    wl_textinput_focus(a.client, a.surface); pump(&a, &b);
    assert(a.enters == 1 && !nr); /* focus alone does not notify */
    commit(&a, 5); pump(&a, &b); assert(!nr);
    rect(&a, 5, 10, -20, 50, 40); commit(&a, 5); pump(&a, &b); assert(!nr);
    rect(&a, 5, 11, -20, 50, 40); commit(&a, 5); pump(&a, &b);
    assert(nr == 1); last(1, 11, -20, 50, 40);
    empty(&a, 5, 2); pump(&a, &b); assert(nr == 1); /* disable is pending */
    commit(&a, 5); pump(&a, &b); assert(nr == 2); last(0, 11, -20, 50, 40);
    empty(&a, 5, 1); commit(&a, 5); pump(&a, &b); assert(nr == 3); last(1, 0, 0, 0, 0);

    /* Ignored field contents/types must decode correctly without producing any text events. */
    unsigned char surrounding[20] = {0};
    uint32_t len = 4; memcpy(surrounding, &len, 4); memcpy(surrounding + 4, "abc", 4);
    request(&a, 5, 3, surrounding, 16);
    uint32_t cause = 1, content[] = {511, 10};
    request(&a, 5, 4, &cause, 4); request(&a, 5, 5, content, sizeof content);
    commit(&a, 5); pump(&a, &b); assert(nr == 3);

    uint32_t get[] = {7, 4}; request(&a, 3, 1, get, sizeof get); pump(&a, &b);
    assert(a.enters == 2); /* creating a second input on a focused client enters immediately */
    empty(&a, 7, 1); rect(&a, 7, -9, 8, 7, 6); commit(&a, 7); pump(&a, &b);
    last(1, -9, 8, 7, 6);
    unsigned before = nr;
    wl_textinput_focus(b.client, b.surface); pump(&a, &b);
    assert(nr == before + 2 && a.leaves == 2 && b.enters == 1);
    Report newest = reports[before]; assert(newest.x == -9); last(0, 0, 0, 0, 0);
    empty(&b, 5, 1); rect(&b, 5, 100, 200, 300, 400); commit(&b, 5); pump(&a, &b);
    last(1, 100, 200, 300, 400);
    before = nr; wl_textinput_focus(b.client, b.surface); pump(&a, &b);
    assert(nr == before + 1 && b.leaves == 1 && b.enters == 2); last(0, 100, 200, 300, 400);
    commit(&b, 5); pump(&a, &b); assert(nr == before + 1); /* focus reset want/active */

    /* Deterministic event stream exercises pending/committed data and repeated focus changes. */
    uint32_t rng = 0x7183;
    for (unsigned k = 0; k < 3000; k++) {
        rng = rng * 1664525u + 1013904223u;
        Peer *p = rng & 1 ? &a : &b;
        switch ((rng >> 8) % 7) {
        case 0: empty(p, 5, 1); break;
        case 1: empty(p, 5, 2); break;
        case 2: rect(p, 5, (int)(rng % 1001) - 500, (int)(k % 901) - 450, k % 80, k % 101); break;
        case 3: commit(p, 5); break;
        case 4: wl_textinput_focus(p->client, p->surface); break;
        case 5: wl_textinput_gone(); break;
        case 6: wl_textinput_focus(NULL, NULL); break;
        }
        pump(&a, &b);
    }

    wl_textinput_focus(a.client, a.surface); pump(&a, &b);
    empty(&a, 5, 2); commit(&a, 5); pump(&a, &b);
    empty(&a, 5, 1); commit(&a, 5); pump(&a, &b); last(1, 0, 0, 0, 0);
    before = nr; empty(&a, 5, 0); pump(&a, &b);
    assert(nr == before + 1); last(0, 0, 0, 0, 0); /* active resource destruction */
    empty(&a, 7, 1); commit(&a, 7); pump(&a, &b);
    before = nr; wl_textinput_gone(); pump(&a, &b); assert(nr == before);
    empty(&a, 7, 0); empty(&a, 3, 0); pump(&a, &b); assert(nr == before);
    wl_client_destroy(a.client); close(a.fd);
    wl_textinput_focus(b.client, b.surface); pump(&b, NULL);
    empty(&b, 5, 1); commit(&b, 5); pump(&b, NULL);
    before = nr; wl_client_destroy(b.client); close(b.fd); assert(nr == before + 1);
    wl_display_destroy(display);
    /* Frozen from the unchanged fixture against the original C implementation. */
    assert(nr == 44 && messages == 1113 && digest == 0x46c8615e21ad4334ULL);
    printf("text input Wayland wire: %u reports, %u focus events, digest %016llx passed\n", nr, messages, digest);
    return 0;
}
