#ifndef AXIA_GLASS_PACING_FFI_H
#define AXIA_GLASS_PACING_FFI_H
/* How often app frames refresh the glass over them (rust/axia-shell/src/glass_pacing.rs). UI thread;
 * times in monotonic milliseconds. */
int axia_glass_pacing_request(double now_ms);   /* 1: refresh now; 0: kept for later */
int axia_glass_pacing_tick(double now_ms);      /* 1: what was kept is due now */
double axia_glass_pacing_timeout(double now_ms);/* ms until due, -1 if nothing is kept */
#endif
