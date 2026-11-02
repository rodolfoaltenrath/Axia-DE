/* gestures.h — test gestures for the simulated touchpad (tpsim.c) and for the recognizer
 * test (server/tests/tp_test.c). Each gesture generates the evdev sequence (protocol B) that
 * a real clickpad would send, frame by frame at 80 Hz.
 *
 * The device imitates a laptop Synaptics clickpad (SynPS/2, 105 × 87 mm, 5 contacts,
 * resolution 42 units/mm, pressure 0..255, INPUT_PROP_BUTTONPAD). */
#ifndef TPSIM_GESTURES_H
#define TPSIM_GESTURES_H

#define SIM_NAME   "LordOS SynPS/2 Synaptics TouchPad (simulado)"
#define SIM_X_MIN  1266
#define SIM_X_MAX  5676
#define SIM_Y_MIN  1096
#define SIM_Y_MAX  4758
#define SIM_RES    42
#define SIM_SLOTS  5
#define SIM_PMAX   255
#define SIM_W_MM   ((SIM_X_MAX - SIM_X_MIN) / (double)SIM_RES)
#define SIM_H_MM   ((SIM_Y_MAX - SIM_Y_MIN) / (double)SIM_RES)

/* type, code, value at time t (ms since the start) */
typedef void (*SimEmit)(void *ud, double t, int type, int code, int value);

typedef struct {
    SimEmit emit;
    void *ud;
    double t;                       /* current time (ms) */
    int down[SIM_SLOTS], id[SIM_SLOTS], x[SIM_SLOTS], y[SIM_SLOTS], p[SIM_SLOTS];
    int next_id, tool, button;
    double frame_ms;
} SimGen;

typedef struct {
    const char *name;
    const char *what;
    void (*play)(SimGen *g);
} SimGesture;

void sim_init(SimGen *g, SimEmit emit, void *ud);
const SimGesture *sim_gestures(int *n);
const SimGesture *sim_find(const char *name);
void sim_wait(SimGen *g, double ms);

#endif
