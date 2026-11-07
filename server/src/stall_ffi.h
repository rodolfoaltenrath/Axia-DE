#ifndef AXIA_STALL_FFI_H
#define AXIA_STALL_FFI_H
/* Main-loop stall watch (rust/axia-shell/src/stall.rs): how long a phase of the loop held the
 * screen. Phases: 0 theme timers, 1 rendering, 2 dispatching poll()'s fds, 3 event queues. */
void axia_stall_note(unsigned phase, double ms);
#endif
