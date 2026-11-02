/* cursor.h — LordOS cursor shapes (own vector drawing, at 1x and 2x). See docs/WINDOWS.md. */
#ifndef LORD_CURSOR_H
#define LORD_CURSOR_H

#include <stdint.h>

/* The order matches CURSOR_CSS_NAMES (the nodes' `cursor` style, ui/style.c). */
enum {
    CUR_AUTO = 0, CUR_DEFAULT, CUR_POINTER, CUR_TEXT, CUR_MOVE, CUR_GRAB, CUR_GRABBING, CUR_NOT_ALLOWED,
    CUR_WAIT, CUR_PROGRESS, CUR_CROSSHAIR,
    CUR_N, CUR_S, CUR_E, CUR_W, CUR_NE, CUR_NW, CUR_SE, CUR_SW, CUR_EW, CUR_NS, CUR_NESW, CUR_NWSE,
    CUR_COUNT
};
#define CURSOR_CSS_NAMES "auto", "default", "pointer", "text", "move", "grab", "grabbing", "not-allowed", "wait", \
    "progress", "crosshair", "n-resize", "s-resize", "e-resize", "w-resize", "ne-resize", "nw-resize", "se-resize", \
    "sw-resize", "ew-resize", "ns-resize", "nesw-resize", "nwse-resize"

typedef struct {
    const uint32_t *px;     /* premultiplied ARGB */
    int w, h, hx, hy;       /* size and hotspot */
} CursorImage;

/* Image of a shape (drawn the first time and kept). scale: 1 or 2. */
const CursorImage *cursor_image(int shape, int scale);
/* Cursor scale for a W×H screen (2 on 4K/HiDPI). */
int cursor_scale_for(int W, int H);
/* The same at a display's scale in percent (100–200). */
const CursorImage *cursor_image_pct(int shape, int pct);
/* xdg_toplevel.resize edges (1 top, 2 bottom, 4 left, 8 right) → shape. */
int cursor_for_edges(int edges);
/* cursor-shape-v1 shape (wp_cursor_shape_device_v1.shape) → own shape; 0 if none. */
int cursor_from_wayland_shape(unsigned shape);

#endif
