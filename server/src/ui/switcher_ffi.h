#ifndef AXIA_SWITCHER_FFI_H
#define AXIA_SWITCHER_FFI_H
#include <stddef.h>
#include <stdbool.h>
/* Opaque, Rust-owned state. Only the UI thread may use a handle. Text is copied
 * during each call; callers retain ownership of all input buffers. */
typedef struct AxiaSwitcher AxiaSwitcher;
AxiaSwitcher *axia_switcher_new(void);
void axia_switcher_free(AxiaSwitcher *);
void axia_switcher_focus(AxiaSwitcher *, const char *);
void axia_switcher_begin(AxiaSwitcher *, const char *const *, size_t, const char *, bool);
void axia_switcher_step(AxiaSwitcher *, int);
void axia_switcher_select(AxiaSwitcher *, size_t);
void axia_switcher_remove(AxiaSwitcher *, const char *);
int axia_switcher_index(const AxiaSwitcher *);
size_t axia_switcher_len(const AxiaSwitcher *);
int axia_switcher_source(const AxiaSwitcher *, size_t);
int axia_switcher_finish(AxiaSwitcher *, bool);
#endif
