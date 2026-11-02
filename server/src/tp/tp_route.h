/* tp_route.h — splits what the driver (tp.c) produces between the LordOS UI (JS) and the
 * Wayland apps (wl/wl_input.c). main.c gives it the pointer; the remote desktop injects the browser's scroll and
 * pinch through here. */
#ifndef LORD_TP_ROUTE_H
#define LORD_TP_ROUTE_H

typedef struct {
    void (*move)(double dx, double dy, double raw_dx, double raw_dy);   /* relative, px */
    void (*warp)(double x, double y);                                   /* absolute, px */
    void (*button)(int button, int down);
    void (*position)(double *x, double *y, int *w, int *h);
    void (*flush)(void);            /* dispatch pending buttons and motion (like a SYN) */
} TpHost;

void tp_route_init(const TpHost *h);

/* sources for tp_route_scroll */
enum { TPR_FINGER = 0, TPR_WHEEL = 1, TPR_CONTINUOUS = 2 };
/* Scroll in pixels (positive = move the content down) with phase TP_*; also from the browser. */
void tp_route_scroll(int phase, double dx, double dy, int source);
/* Pinch: scale relative to the start and accumulated rotation (degrees). */
void tp_route_pinch(int phase, int fingers, double dx, double dy, double scale, double rotation);

/* Mouse wheels (main.c): REL_WHEEL_HI_RES / REL_HWHEEL[_HI_RES] in units of 1/120 of a notch
 * (+ up / + right). They're accumulated until the SYN (tp_route_wheel_flush) and go as fine scroll:
 * to the Wayland app with axis_value120, to the UI with deltaX/deltaY (120 = 48 px). */
void tp_route_wheel120(int v120, int h120);
void tp_route_wheel_flush(void);

#endif
