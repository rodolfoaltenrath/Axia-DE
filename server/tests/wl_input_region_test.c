/* Exercise the actual region handlers and surface hit testing, without a GPU or
 * a running desktop. Unused compositor code is discarded by --gc-sections. */
#include "../src/wl/wl.c"
#include <sys/socket.h>

static int checks;
#define CHECK(c) do { checks++; if (!(c)) { fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #c); return 1; } } while (0)

int main(void) {
    int fd[2];
    CHECK(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, fd) == 0);
    struct wl_display *display = wl_display_create();
    CHECK(display != NULL);
    struct wl_client *client = wl_client_create(display, fd[0]);
    CHECK(client != NULL);
    InputRegion *region = calloc(1, sizeof *region);
    CHECK(region != NULL);
    struct wl_resource *res = wl_resource_create(client, &wl_region_interface, 1, 0);
    CHECK(res != NULL);
    wl_resource_set_implementation(res, &region_impl, region, rgn_gone);
    CHECK(!input_region_contains(region, 0, 0));
    CHECK(input_region_contains(NULL, -100, 100));

    rgn_add(client, res, -5, -5, 20, 20);
    rgn_subtract(client, res, 0, 0, 10, 10);
    for (int y = -7; y < 17; y++) for (int x = -7; x < 17; x++) {
        int expected = x >= -5 && x < 15 && y >= -5 && y < 15 && !(x >= 0 && x < 10 && y >= 0 && y < 10);
        CHECK(input_region_contains(region, x, y) == expected);
    }
    rgn_add(client, res, 2, 2, 2, 2);       /* add back inside a subtracted hole */
    CHECK(input_region_contains(region, 2, 2));
    CHECK(!input_region_contains(region, 4, 2));
    rgn_add(client, res, INT32_MAX - 1, 0, 10, 10); /* endpoint must not overflow */
    CHECK(input_region_contains(region, 2147483648.0f, 1));
    size_t count = region->len;
    rgn_add(client, res, 0, 0, 0, 5);
    rgn_subtract(client, res, 0, 0, 5, -1);
    CHECK(region->len == count);

    Surf parent = { .content = 1, .lw = 100, .lh = 100 };
    Surf child = { .content = 1, .lw = 100, .lh = 100, .role = R_SUB, .parent = &parent };
    parent.kids[0] = &child; parent.nkids = 1;
    struct wl_resource *surface = wl_resource_create(client, &wl_surface_interface, 6, 0);
    CHECK(surface != NULL);
    wl_resource_set_implementation(surface, NULL, &child, NULL);
    float x, y;
    CHECK(surface_at(&parent, 50, 50, &x, &y) == &child);

    struct wl_resource *empty = wl_resource_create(client, &wl_region_interface, 1, 0);
    CHECK(empty != NULL);
    InputRegion *empty_region = calloc(1, sizeof *empty_region);
    CHECK(empty_region != NULL);
    wl_resource_set_implementation(empty, &region_impl, empty_region, rgn_gone);
    srf_input_region(client, surface, empty);
    CHECK(surface_at(&parent, 50, 50, &x, &y) == &child); /* pending until commit */
    rgn_add(client, empty, 0, 0, 100, 100); /* must not mutate the pending copy */
    wl_resource_destroy(empty);             /* caller may destroy it immediately */
    commit_input_region(&child);
    CHECK(surface_at(&parent, 50, 50, &x, &y) == &parent); /* Firefox drawing layer */
    commit_input_region(&child);
    CHECK(surface_at(&parent, 50, 50, &x, &y) == &parent); /* persists across commits */

    srf_input_region(client, surface, res);
    wl_resource_destroy(res);
    commit_input_region(&child);
    CHECK(surface_at(&parent, 2, 2, &x, &y) == &child);
    CHECK(surface_at(&parent, 5, 5, &x, &y) == &parent);
    parent.input = calloc(1, sizeof *parent.input);
    CHECK(parent.input != NULL);
    CHECK(surface_at(&parent, 2, 2, &x, &y) == &child); /* parent region does not clip children */
    CHECK(surface_at(&parent, 50, 50, &x, &y) == NULL);
    srf_input_region(client, surface, NULL);
    CHECK(surface_at(&parent, 50, 50, &x, &y) == NULL);
    commit_input_region(&child);
    CHECK(surface_at(&parent, 50, 50, &x, &y) == &child); /* NULL restores full input */
    CHECK(surface_at(&parent, 100, 50, &x, &y) == NULL);
    input_region_free(parent.input);
    input_region_free(child.input);
    input_region_free(child.p_input);
    wl_display_destroy_clients(display);
    wl_display_destroy(display);
    close(fd[1]);
    printf("wl-input-region: %d checks passed\n", checks);
    return 0;
}
