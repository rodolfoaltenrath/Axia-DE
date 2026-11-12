/* Real Wayland protocol events, run unchanged against baseline C and Rust. */
#include "wayland_input_fixture.h"
#include "wl/wl_input.h"
#include "tp/tp.h"
#include <assert.h>
#include <stdio.h>
extern const struct wl_interface wl_seat_interface;

static const WliEvent *last(unsigned peer, unsigned object, unsigned opcode) {
    const WliEvent *e = wli_last(peer, object, opcode); assert(e); return e;
}
static unsigned count(unsigned start, unsigned peer, unsigned object, unsigned opcode) {
    unsigned n = 0;
    for (unsigned i = start; i < wli_nevents; i++) {
        const WliEvent *e = &wli_events[i];
        if (e->peer == peer && (!object || e->object == object) && e->opcode == opcode) n++;
    }
    return n;
}
static void scroll(void) {
    unsigned before = wli_nevents;
    wl_input_axis(0, 1, 2, 0, 0); wl_input_wheel120(120, 120); wli_pump(); assert(wli_nevents == before);
    wli_focus = 0; assert(wl_input_has_focus());
    wl_input_axis(0, 1.5, -2.25, 0, 1); wli_pump();
    for (unsigned i = 0; i < 6; i++) {
        unsigned p = 5 + i, v = wli_versions[i];
        assert(count(before, 0, p, 4) == 2);
        const WliEvent *e = last(0, p, 4); assert(e->args[0] == 123456 && e->args[1] == 1 && e->args[2] == 384);
        assert(count(before, 0, p, 6) == (v >= 5));
        assert(count(before, 0, p, 10) == (v >= 9 ? 2 : 0));
        assert(count(before, 0, p, 5) == (v >= 5));
        if (v >= 5) assert(last(0, p, 6)->args[0] == 1);
        if (v >= 9) assert(last(0, p, 10)->args[1] == 1);
    }
    assert(!count(before, 1, 0, 4));
    wli_time_ms = (1ULL << 32) + 7;
    wl_input_axis(1, 0.5 / 256, -0.5 / 256, 0, 0); wli_pump();
    assert(last(0, 10, 4)->args[0] == 7 && last(0, 10, 4)->args[2] == 1);
    assert(last(0, 10, 10)->args[1] == 0);
    wli_time_ms = 123456;
    before = wli_nevents; wl_input_axis(99, 30, 40, 1, 0); wli_pump();
    for (unsigned i = 0; i < 6; i++) {
        assert(count(before, 0, 5 + i, 7) == (wli_versions[i] >= 5 ? 2 : 0));
        assert(!count(before, 0, 5 + i, 4));
        if (wli_versions[i] >= 5) assert(last(0, 5 + i, 6)->args[0] == 2);
    }
    for (unsigned i = 0; i < 6; i++) {
        before = wli_nevents; wl_input_wheel(wli_peers[0].pointers[i], 890, 2); wli_pump();
        assert(!count(before, 0, 5 + i, 5)); /* caller owns the mouse-wheel frame */
        const WliEvent *e = last(0, 5 + i, 4); assert(e->args[0] == 890 && e->args[2] == -5120);
        if (wli_versions[i] >= 8) assert(last(0, 5 + i, 9)->args[1] == -240);
        else if (wli_versions[i] >= 5) assert(last(0, 5 + i, 8)->args[1] == -2);
    }
    before = wli_nevents; wl_input_wheel(wli_peers[0].pointers[5], 0, 0); wli_pump(); assert(before == wli_nevents);
    before = wli_nevents; wl_input_wheel120(60, 60); wli_pump();
    assert(!count(before, 0, 7, 8)); assert(last(0, 10, 9)->args[1] == 60);
    wli_focus = 1; wl_input_wheel120(60, 60); wli_pump();
    assert(last(1, 7, 8)->args[1] == 1); /* shared remainder across focus changes */
    wl_input_wheel120(-240, -120); wli_pump(); assert(last(1, 7, 8)->args[1] == -1);
}
static void gestures(void) {
    wli_focus = 0;
    unsigned before = wli_nevents;
    wl_input_swipe(TP_BEGIN, 4, 0, 0); wli_pump();
    assert(count(before, 0, 12, 0) == 1 && count(before, 0, 16, 0) == 1);
    assert(wli_events[before].object == 16 && wli_events[before + 1].object == 12);
    assert(last(0, 12, 0)->args[2] == 11 && last(0, 12, 0)->args[3] == 4);
    wli_focus = 1; before = wli_nevents;
    wl_input_swipe(TP_UPDATE, 4, 1.25, -3.5); wli_pump();
    assert(count(before, 0, 12, 1) == 1 && !count(before, 1, 12, 1));
    assert(last(0, 12, 1)->args[1] == 320 && last(0, 12, 1)->args[2] == -896);
    wli_focus = -1; wl_input_swipe(TP_CANCEL, 4, 0, 0); wli_pump(); assert(last(0, 12, 2)->args[2] == 1);
    before = wli_nevents; wl_input_swipe(TP_UPDATE, 4, 8, 9); wli_pump(); assert(before == wli_nevents);
    wli_focus = 1; wl_input_pinch(TP_BEGIN, 2, 0, 0, 1, 0); wli_pump();
    wli_focus = 0; wl_input_pinch(TP_UPDATE, 2, -0.5, 3.75, 1.25, -15.5); wli_pump();
    const WliEvent *e = last(1, 13, 1);
    assert(e->n == 5 && e->args[1] == -128 && e->args[2] == 960 && e->args[3] == 320 && e->args[4] == -3968);
    wl_input_pinch(TP_END, 2, 0, 0, 1, 0); wli_pump(); assert(last(1, 13, 2)->args[2] == 0);
    wl_input_hold(TP_BEGIN, 2); wli_pump(); assert(last(0, 14, 0)->args[3] == 2);
    wl_input_hold(TP_CANCEL, 2); wli_pump(); assert(last(0, 14, 1)->args[2] == 1);
    before = wli_nevents; wl_input_hold(TP_END, 2); wli_pump(); assert(before == wli_nevents);
}
static void touch(void) {
    wli_hit = 0; wli_focus = 1;
    assert(!wl_input_touch(TP_BEGIN, -1, 200, 100) && !wl_input_touch(TP_BEGIN, 10, 200, 100));
    assert(wl_input_touch(TP_BEGIN, 0, 200.25, 150.5));
    assert(wl_input_touch(TP_BEGIN, 1, 250, 170)); wli_pump();
    const WliEvent *e = last(0, 15, 0); assert(e->args[3] == 1 && e->args[4] == 38400 && e->args[5] == 30720);
    unsigned before = wli_nevents; wl_input_touch_frame(); wli_pump();
    assert(count(before, 0, 15, 3) == 1 && count(before, 0, 17, 3) == 1);
    before = wli_nevents; wl_input_touch_frame(); wli_pump(); assert(before == wli_nevents);
    wli_hit = 1; assert(wl_input_touch(TP_UPDATE, 0, 650.25, 180.5)); wli_pump();
    e = last(0, 15, 2); assert(e->args[1] == 0 && e->args[2] == 140864 && e->args[3] == 33408);
    assert(wl_input_touch(TP_CANCEL, 0, 0, 0)); wli_pump(); assert(last(0, 15, 4)->n == 0);
    assert(!wl_input_touch(TP_UPDATE, 0, 0, 0));
    assert(wl_input_touch(TP_UPDATE, 1, 270, 190));
    assert(wl_input_touch(TP_END, 1, 0, 0));
    assert(wl_input_touch(TP_BEGIN, 2, 700, 200));
    before = wli_nevents; wl_input_touch_frame(); wli_pump();
    assert(count(before, 0, 15, 3) == 1 && count(before, 1, 15, 3) == 1);
    assert(wl_input_touch(TP_END, 2, 0, 0));
    wli_hit = -1; assert(!wl_input_touch(TP_BEGIN, 3, 200, 100));
    wli_pump();
}
static void capabilities(void) {
    assert(last(0, 4, 0)->args[0] == 3);
    TpCaps caps = {.kind=TP_KIND_TOUCHSCREEN, .nslots=5, .max_fingers=5};
    caps.x.maximum=10000; caps.x.resolution=100; caps.y.maximum=6000; caps.y.resolution=100;
    TpDevice *device = tp_new(&caps); assert(device);
    wl_input_devices_changed(); wli_pump(); assert(last(0, 4, 0)->args[0] == 7 && last(1, 4, 0)->args[0] == 7);
    tp_free(device); wl_input_devices_changed(); wli_pump(); assert(last(0, 4, 0)->args[0] == 3);
}
static void older_clients(void) {
    wli_bind_manager(0, 18, 1); wli_bind_manager(0, 19, 2);
    uint32_t swipe[] = {20, 5}, pinch[] = {21, 6};
    wli_request(0, 18, 0, swipe, 2); wli_request(0, 19, 1, pinch, 2); wli_pump();
    assert(wl_resource_get_version(wl_client_get_object(wli_peers[0].client, 20)) == 1);
    assert(wl_resource_get_version(wl_client_get_object(wli_peers[0].client, 21)) == 2);
    wli_request(0, 19, 2, NULL, 0); wli_pump();
    wli_focus=0; wl_input_swipe(TP_BEGIN, 3, 0, 0); wl_input_swipe(TP_END, 3, 0, 0);
    wl_input_pinch(TP_BEGIN, 2, 0, 0, 1, 0); wl_input_pinch(TP_UPDATE, 2, 1, 2, 1.5, 30);
    wl_input_pinch(TP_END, 2, 0, 0, 1, 0); wli_pump();
    assert(last(0, 20, 0)->args[3] == 3 && last(0, 20, 2)->args[2] == 0);
    assert(last(0, 21, 1)->args[3] == 384 && last(0, 21, 2)->args[2] == 0);
    wli_request(0, 20, 0, NULL, 0); wli_request(0, 21, 0, NULL, 0); wli_pump();
    struct wl_resource *seat = wl_resource_create(wli_peers[1].client, &wl_seat_interface, 1, 18); assert(seat);
    wl_input_seat_caps(seat); wl_input_get_touch(wli_peers[1].client, seat, 19);
    wli_hit=1; assert(wl_input_touch(TP_BEGIN, 9, 650, 100)); wl_input_touch_frame();
    assert(wl_input_touch(TP_END, 9, 0, 0)); wl_input_touch_frame(); wli_pump();
    assert(last(1, 19, 0)->args[4] == 12800 && last(1, 19, 1)->args[2] == 9 && last(1, 19, 3)->n == 0);
    wl_resource_destroy(seat); wl_input_devices_changed(); wli_pump();
}
int main(int argc, char **argv) {
    wli_init(); scroll(); gestures(); touch(); capabilities();
    uint32_t rng = 0x1621;
    for (unsigned k = 0; k < 5000; k++) {
        rng = rng * 1664525u + 1013904223u;
        wli_focus = (int)((rng >> 16) % 3) - 1;
        wli_hit = (int)((rng >> 20) % 3) - 1;
        int phase = (rng >> 4) % 4 + 1;
        double dx = ((int)(rng % 901) - 450) / 256., dy = (int)(k % 701) / 8.;
        switch ((rng >> 8) % 8) {
        case 0: wl_input_axis(rng % 3, dx, dy, (rng >> 10) & 1, rng & 1); break;
        case 1: wl_input_wheel120((int)(rng % 241) - 120, (int)(k % 241) - 120); break;
        case 2: wl_input_swipe(phase, 4, dx, dy); break;
        case 3: wl_input_pinch(phase, 2, dx, dy, 1 + k / 1000., dx * 3); break;
        case 4: wl_input_hold(phase, 2); break;
        case 5: wl_input_touch(phase, rng % 10, 200 + dx, 120 + dy); break;
        case 6: wl_input_touch_frame(); break;
        case 7: wl_input_devices_changed(); break;
        }
        wli_pump();
    }
    older_clients();
    /* Releasing the manager does not destroy its children. */
    wli_request(0, 3, 2, NULL, 0); wli_pump();
    wli_focus = 0; wl_input_swipe(TP_BEGIN, 3, 0, 0); wli_pump();
    wli_request(0, 16, 0, NULL, 0); wli_pump();
    unsigned before = wli_nevents; wl_input_swipe(TP_UPDATE, 3, 1, 2); wli_pump();
    assert(count(before, 0, 12, 1) == 1 && !count(before, 0, 16, 1));
    wli_request(0, 17, 0, NULL, 0); wli_request(0, 10, 1, NULL, 0); wli_pump();
    wli_finish_peer(0); wl_input_swipe(TP_UPDATE, 3, 3, 4); wl_input_swipe(TP_END, 3, 0, 0);
    wl_input_devices_changed(); wl_input_touch_frame(); wli_pump();
    wli_focus = 1; wl_input_hold(TP_BEGIN, 2); wli_pump();
    wli_request(1, 14, 0, NULL, 0); wli_request(1, 13, 0, NULL, 0); wli_request(1, 12, 0, NULL, 0); wli_pump();
    wl_input_hold(TP_END, 2); wli_pump();
    wli_finish();
    /* Frozen from the same fixture linked to the original C implementation. */
    assert(wli_nevents == 25220 && wli_digest == 0xb972f4087f659b2aULL);
    if (argc == 2) {
        FILE *trace = fopen(argv[1], "wb"); assert(trace);
        assert(fwrite(wli_events, sizeof *wli_events, wli_nevents, trace) == wli_nevents);
        assert(!fclose(trace));
    }
    printf("Wayland input wire: %u ordered events, digest %016llx passed\n", wli_nevents, wli_digest);
}
