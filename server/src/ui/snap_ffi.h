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
typedef struct { double x, y, w, h, scale; } AxiaSnapPlace;

enum { AXIA_SNAP_NONE = -1, AXIA_SNAP_RESTORE = 100, AXIA_SNAP_MINIMIZE = 101 };
#define AXIA_SNAP_ASSIST_MAX 12

int         axia_snap_zone_count(void);
const char *axia_snap_zone_name(int zone);                 /* static; NULL for an unknown index */
int         axia_snap_zone_index(const char *name);        /* -1 for an unknown name */
int         axia_snap_rect(int zone, AxiaSnapRect area, AxiaSnapRect *out);   /* 1 if written */
/* The zone for a window dropped at (x, y) in work area `work`; thirds: Shift is held. */
int         axia_snap_zone_at(double x, double y, AxiaSnapRect work, bool thirds);
/* Super+arrows: at a zone or -1; dir 0 left, 1 right, 2 up, 3 down → a zone, RESTORE, MINIMIZE or -1. */
int         axia_snap_next(int at, int dir);

/* Snap Assist: the free zones of `zone`'s layout (the `layout` picked, if it has `zone`; may be empty)
 * given the zones other windows hold, and where the candidates' miniatures go inside a zone. Both
 * return how many entries were written. */
size_t axia_snap_assist_zones(int zone, const int *layout, size_t nlayout, const int *occupied, size_t n, int *out, size_t max);
size_t axia_snap_assist_grid(AxiaSnapRect cell, const AxiaSnapRect *frames, size_t n, AxiaSnapPlace *out, size_t max);

/* While a window is moved (grab.c): the zone under the pointer. update returns 1 when it changed. */
typedef struct AxiaSnapDrag AxiaSnapDrag;
AxiaSnapDrag *axia_snap_drag_new(const AxiaSnapDisplay *displays, size_t n);
void          axia_snap_drag_free(AxiaSnapDrag *);
int           axia_snap_drag_update(AxiaSnapDrag *, double x, double y, bool thirds);
int           axia_snap_drag_zone(const AxiaSnapDrag *, AxiaSnapRect *rect);    /* rect may be NULL */

/* Snapped windows that share the dragged edge of members[0] resize with it (grab.c). NULL when no
 * dragged edge is shared: the plain resize applies. apply writes every member's frame. */
typedef struct AxiaSnapResize AxiaSnapResize;
AxiaSnapResize *axia_snap_resize_new(int edges, const AxiaSnapMember *members, size_t n, AxiaSnapRect area);
void            axia_snap_resize_free(AxiaSnapResize *);
int             axia_snap_resize_moves(const AxiaSnapResize *, size_t i);
size_t          axia_snap_resize_apply(const AxiaSnapResize *, double dx, double dy, AxiaSnapRect *out, size_t max);
#endif
