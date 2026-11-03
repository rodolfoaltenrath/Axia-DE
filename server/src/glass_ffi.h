#ifndef AXIA_GLASS_FFI_H
#define AXIA_GLASS_FFI_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
/* Thread-owned Rust memory. See rust/axia-shell/src/glass.rs for lifetime contract. */
typedef struct { uint32_t *pixels, *source; int32_t *sums, *columns; uint8_t *division; } AxiaGlassBuffers;
bool axia_glass_prepare(size_t w, size_t h, size_t radius, size_t columns, AxiaGlassBuffers *out);
bool axia_glass_reuse(void);
void axia_glass_commit(void);
uint32_t *axia_glass_scratch(size_t words);
uint32_t axia_glass_tint(bool dark, bool effects);
uint32_t axia_glass_outline(bool dark);
#endif
