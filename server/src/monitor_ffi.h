/* Session lifecycle ABI; implementation and traffic recording are Rust-owned. */
#ifndef AXIA_MONITOR_FFI_H
#define AXIA_MONITOR_FFI_H
void netrec_init(int allowed);
void netrec_tick(void);
double netrec_timeout(void);
void netrec_shutdown(void);
#endif
