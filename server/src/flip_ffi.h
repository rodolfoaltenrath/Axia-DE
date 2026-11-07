#ifndef AXIA_FLIP_FFI_H
#define AXIA_FLIP_FFI_H
#include <stdint.h>
/* Whether the display driver waits for vblank (rust/axia-shell/src/flip_pacing.rs). Main loop only. */
int axia_flip_observe(uint64_t submit_ns, uint64_t flip_ns, uint64_t period_ns);   /* 1: pacing just turned on */
int axia_flip_soft(void);                                                          /* 1: pace frames ourselves */
#endif
