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
typedef void (*AxiaGlassKernel)(void *, int, int);
typedef void (*AxiaGlassParallel)(int, uint64_t, AxiaGlassKernel, void *);
void axia_glass_render(void *canvas, float x, float y, float w, float h, float r, int radius, AxiaGlassParallel parallel);
/* UI-thread callback: NULL pixels queries a generation key; otherwise fills the
 * downsampled background. Zero means no GPU background or an unavailable read. */
typedef uint64_t (*AxiaGlassBackdrop)(int, int, int, int, int, int, int, uint32_t *);
void axia_glass_set_backdrop(AxiaGlassBackdrop reader);
/* What a commit changed in a window (rust/axia-shell/src/surface_damage.rs). */
typedef struct { int32_t surface[4], buffer[4]; } AxiaSurfaceDamage;
typedef struct {
    float x, y;
    int32_t width, height, buffer_width, buffer_height;
    bool cropped, attached, has_children;
} AxiaSurfaceShape;
void axia_surface_damage_add(AxiaSurfaceDamage *damage, bool in_buffer, int32_t x, int32_t y, int32_t width, int32_t height);
int axia_surface_damage_box(const AxiaSurfaceDamage *damage, const AxiaSurfaceShape *shape, float *out);
uint32_t axia_glass_tint(bool dark, bool effects);
uint32_t axia_glass_outline(bool dark);
#endif
