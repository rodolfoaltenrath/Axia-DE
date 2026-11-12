#ifndef AXIA_SNAP_FFI_H
#define AXIA_SNAP_FFI_H
/* Window snapping in Rust (rust/axia-shell/src/snap, docs/WINDOWS.md "Snapping"). UI thread only.
 * Frames are in screen pixels. Zones cross as indices (axia_snap_zone_name); -1 is "none".
 * Callers keep ownership of every buffer they pass. */
#include <stdbool.h>
#include <stddef.h>

typedef struct { double x, y, w, h; } AxiaSnapRect;
typedef struct { AxiaSnapRect bounds, work; } AxiaSnapDisplay;          /* a display and its work area */
typedef struct { AxiaSnapRect frame; double min_w, min_h, max_w, max_h; } AxiaSnapMember;   /* max 0 = none */

enum { AXIA_SNAP_NONE = -1, AXIA_SNAP_RESTORE = 100, AXIA_SNAP_MINIMIZE = 101 };

int         axia_snap_zone_count(void);
const char *axia_snap_zone_name(int zone);                 /* static; NULL for an unknown index */
int         axia_snap_zone_index(const char *name);        /* -1 for an unknown name */
int         axia_snap_rect(int zone, AxiaSnapRect area, AxiaSnapRect *out);   /* 1 if written */
/* The zone for a window dropped at (x, y) in work area `work`; thirds: Shift is held. */
int         axia_snap_zone_at(double x, double y, AxiaSnapRect work, bool thirds);
/* Super+arrows: at a zone or -1; dir 0 left, 1 right, 2 up, 3 down → a zone, RESTORE, MINIMIZE or -1. */
int         axia_snap_next(int at, int dir);

/* Snap Assist: the free zones of `zone`'s layout (the `layout` picked, if it has `zone`; may be empty)
 * given the zones other windows hold; returns how many were written. */
size_t axia_snap_assist_zones(int zone, const int *layout, size_t nlayout, const int *occupied, size_t n, int *out, size_t max);

/* While a window is moved (Rust runtime/window_grab.rs): the zone under the pointer. update returns 1 when it changed. */
typedef struct AxiaSnapDrag AxiaSnapDrag;
AxiaSnapDrag *axia_snap_drag_new(const AxiaSnapDisplay *displays, size_t n);
void          axia_snap_drag_free(AxiaSnapDrag *);
int           axia_snap_drag_update(AxiaSnapDrag *, double x, double y, bool thirds);
int           axia_snap_drag_zone(const AxiaSnapDrag *, AxiaSnapRect *rect);    /* rect may be NULL */

/* Snapped windows that share the dragged edge of members[0] resize with it (Rust runtime/window_grab.rs). NULL when no
 * dragged edge is shared: the plain resize applies. apply writes every member's frame. */
typedef struct AxiaSnapResize AxiaSnapResize;
AxiaSnapResize *axia_snap_resize_new(int edges, const AxiaSnapMember *members, size_t n, AxiaSnapRect area);
void            axia_snap_resize_free(AxiaSnapResize *);
int             axia_snap_resize_moves(const AxiaSnapResize *, size_t i);
size_t          axia_snap_resize_apply(const AxiaSnapResize *, double dx, double dy, AxiaSnapRect *out, size_t max);

/* Moving or resizing a window's node with the pointer (rust snap/grab.rs; the Rust runtime owns the node). In the
 * node's parent coordinates; the node is the frame enlarged by `inset` per side. */
typedef struct { double x, w, top; } AxiaGrabArea;     /* a display's span [x, x + w) and its work area's top */
typedef struct {
    AxiaSnapRect node;                  /* the node at the start: left, top, width, height */
    double origin_x, origin_y;          /* the parent's origin on screen */
    double pointer_x, pointer_y;        /* where the button was pressed */
    int edges;                          /* 0 moves; xdg edges (1 top, 2 bottom, 4 left, 8 right) */
    double inset, min_w, min_h, max_w, max_h;   /* of the frame; max 0 = none */
    double min_top;                     /* on screen, unless the area under the frame's centre says otherwise */
    int nareas;
    AxiaGrabArea areas[4];
    double detach_w, detach_h;          /* a maximized or snapped window's node once it comes loose; 0 = none */
} AxiaGrabStart;
enum { AXIA_GRAB_MOVED = 1, AXIA_GRAB_SIZED = 2, AXIA_GRAB_DETACHED = 4 };
typedef struct AxiaGrab AxiaGrab;
AxiaGrab *axia_grab_new(const AxiaGrabStart *start);
void      axia_grab_free(AxiaGrab *);
/* The pointer moved: AXIA_GRAB_* flags (0: the node stays) and the node now in `out`. */
int       axia_grab_motion(AxiaGrab *, double px, double py, double screen_w, double screen_h, AxiaSnapRect *out);
int       axia_grab_waiting(const AxiaGrab *);         /* 1 until a maximized or snapped window comes loose */
int       axia_grab_end(const AxiaGrab *, AxiaSnapRect *delta);   /* the node's change; 1 if it came loose */
#endif
