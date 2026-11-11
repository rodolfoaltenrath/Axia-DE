/* The C/Rust contract of window snapping (src/ui/snap_ffi.h, rust/axia-shell/src/snap). The rules
 * themselves are tested in Rust; this checks the ABI: struct layout, indices, names and buffers. */
#include "../src/ui/snap_ffi.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    const AxiaSnapRect area = { 0, 0, 1280, 752 };
    assert(axia_snap_zone_count() == 14);
    for (int i = 0; i < axia_snap_zone_count(); i++) assert(axia_snap_zone_index(axia_snap_zone_name(i)) == i);
    assert(!axia_snap_zone_name(-1) && !axia_snap_zone_name(14));
    assert(axia_snap_zone_index("sideways") == AXIA_SNAP_NONE && axia_snap_zone_index(NULL) == AXIA_SNAP_NONE);

    int left = axia_snap_zone_index("left"), right = axia_snap_zone_index("right"), max = axia_snap_zone_index("max");
    AxiaSnapRect r;
    assert(axia_snap_rect(right, area, &r) == 1 && r.x == 640 && r.y == 0 && r.w == 640 && r.h == 752);
    assert(axia_snap_rect(99, area, &r) == 0);
    assert(axia_snap_zone_at(0, 400, area, false) == left);
    assert(axia_snap_zone_at(0, 400, area, true) == axia_snap_zone_index("left-third"));
    assert(axia_snap_zone_at(640, 400, area, false) == AXIA_SNAP_NONE);
    assert(axia_snap_next(AXIA_SNAP_NONE, 2) == max);
    assert(axia_snap_next(max, 3) == AXIA_SNAP_RESTORE);
    assert(axia_snap_next(AXIA_SNAP_NONE, 3) == AXIA_SNAP_MINIMIZE);

    int free_zones[4], held[1] = { right };
    assert(axia_snap_assist_zones(left, NULL, 0, NULL, 0, free_zones, 4) == 1 && free_zones[0] == right);
    assert(axia_snap_assist_zones(left, NULL, 0, held, 1, free_zones, 4) == 0);
    int layout[2] = { axia_snap_zone_index("left-two-thirds"), axia_snap_zone_index("right-third") };
    assert(axia_snap_assist_zones(layout[1], layout, 2, NULL, 0, free_zones, 4) == 1 && free_zones[0] == layout[0]);

    AxiaSnapDisplay displays[1] = { { { 0, 0, 1280, 800 }, area } };
    AxiaSnapDrag *d = axia_snap_drag_new(displays, 1);
    assert(d && axia_snap_drag_zone(d, NULL) == AXIA_SNAP_NONE);
    assert(axia_snap_drag_update(d, 1279, 400, false) == 1 && axia_snap_drag_update(d, 1278, 410, false) == 0);
    assert(axia_snap_drag_zone(d, &r) == right && r.x == 640);
    axia_snap_drag_free(d);
    axia_snap_drag_free(NULL);

    AxiaSnapMember m[2] = {
        { { 0, 0, 640, 752 }, 160, 100, 0, 0 },
        { { 640, 0, 640, 752 }, 160, 100, 0, 0 },
    };
    AxiaSnapResize *l = axia_snap_resize_new(8, m, 2, area);
    assert(l && axia_snap_resize_moves(l, 1) == 1 && axia_snap_resize_moves(l, 2) == 0);
    AxiaSnapRect out[2];
    memset(out, 0, sizeof out);
    assert(axia_snap_resize_apply(l, 60, 0, out, 2) == 2);
    assert(out[0].w == 700 && out[1].x == 700 && out[1].w == 580);
    axia_snap_resize_free(l);
    axia_snap_resize_free(NULL);
    assert(!axia_snap_resize_new(4, m, 2, area));

    /* a maximized window (no borders) grabbed in the middle of its bar: the fields after areas[]
     * reach Rust only with the same layout (detach_*), and so does the second display's top */
    AxiaGrabStart gs = {
        .node = { 0, 28, 1280, 772 }, .pointer_x = 640, .pointer_y = 40, .edges = 0,
        .min_w = 160, .min_h = 100, .min_top = 28, .nareas = 2,
        .areas = { { 0, 1280, 28 }, { 1280, 1920, 0 } },
        .detach_w = 600, .detach_h = 400,
    };
    AxiaGrab *g = axia_grab_new(&gs);
    assert(g && axia_grab_waiting(g) == 1);
    assert(axia_grab_motion(g, 642, 42, 3200, 1080, &r) == 0);
    assert(axia_grab_motion(g, 644, 44, 3200, 1080, &r) == (AXIA_GRAB_DETACHED | AXIA_GRAB_MOVED | AXIA_GRAB_SIZED));
    assert(r.x == 344 && r.y == 32 && r.w == 600 && r.h == 400 && !axia_grab_waiting(g));
    assert(axia_grab_motion(g, 644 + 1600, -400, 3200, 1080, &r) == AXIA_GRAB_MOVED && r.y == 0);
    AxiaSnapRect delta;
    assert(axia_grab_end(g, &delta) == 1 && delta.x == 1944 && delta.y == -28 && delta.w == -680 && delta.h == -372);
    axia_grab_free(g);
    axia_grab_free(NULL);
    assert(!axia_grab_new(NULL));
    puts("snap C/Rust ABI: ok");
    return 0;
}
