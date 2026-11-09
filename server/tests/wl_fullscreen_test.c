/* Real fullscreen/minimize state transitions with an XWM transport probe. No GPU, active session
 * or real application is touched. In particular, Alt+Tab must not request a window-sized buffer. */
#include "../src/wl/wl.c"
#include <assert.h>

Ui ui;
static int width, height, size_changes, full, iconic;
Node *node_get(int id) { (void)id; return NULL; }
void damage_add(Box box) { (void)box; }
void xwm_place(XWin *win, int x, int y) { (void)win; (void)x; (void)y; }
void xwm_resize(XWin *win, int w, int h) {
    (void)win;
    if (width != w || height != h) size_changes++;
    width = w; height = h;
}
void xwm_set_state(XWin *win, int maximized, int fullscreen, int minimized) {
    (void)win; (void)maximized;
    full = fullscreen; iconic = minimized;
}

int main(void) {
    ui.width = 1920; ui.height = 1080;
    Surf game = { .id = 777, .role = R_TOPLEVEL, .xw = (XWin *)1,
        .conf_w = 640, .conf_h = 480, .content = 1 };
    surfs[0] = &game;
    enter_fullscreen(&game);
    assert(wl_fullscreen_window() == 777 && game.fs_saved_w == 640 && game.fs_saved_h == 480);
    int initial_changes = size_changes;
    for (int i = 0; i < 100; i++) {
        wl_set_minimized(777, 1);
        assert(!wl_fullscreen_active() && game.fullscreen && game.fs_resume && full && iconic);
        assert(game.conf_w == 1920 && game.conf_h == 1080);
        wl_set_minimized(777, 1); /* repeated shell synchronization is harmless */
        wl_set_minimized(777, 0);
        assert(wl_fullscreen_window() == 777 && !game.fs_resume && full && !iconic);
        assert(game.fs_saved_w == 640 && game.fs_saved_h == 480);
        assert(size_changes == initial_changes);
    }
    wl_set_minimized(777, 1);
    ui.width = 2560; ui.height = 1440;
    wl_set_minimized(777, 0);
    assert(width == 2560 && height == 1440 && size_changes == initial_changes + 1);
    assert(game.fs_saved_w == 640 && game.fs_saved_h == 480);
    wl_set_minimized(777, 1);
    leave_fullscreen(&game); /* an app may explicitly leave fullscreen while minimized */
    assert(!game.fullscreen && !game.fs_resume && game.conf_w == 640 && game.conf_h == 480);
    wl_set_minimized(777, 0);
    assert(!wl_fullscreen_active() && width == 640 && height == 480 && !full && !iconic);
    puts("fullscreen: 100 switches without buffer resize, output change and explicit exit passed");
    return 0;
}
