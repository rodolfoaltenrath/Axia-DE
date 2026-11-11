/* Bounded Rust codecs. Returned buffers belong to C: free() normally,
 * sm_free() when linked to the vault's secure-memory archive. */
#ifndef AXIA_CODECS_FFI_H
#define AXIA_CODECS_FFI_H
#include <stddef.h>
#include <stdint.h>
uint8_t *axia_codec_inflate(const uint8_t *, size_t, size_t max, size_t *outlen);
uint8_t *axia_codec_gunzip(const uint8_t *, size_t, size_t max, size_t *outlen);
uint8_t *axia_image_decode(const uint8_t *, size_t, int *w, int *h, int channels, uint32_t max_side, uint64_t max_pixels);
uint8_t *axia_image_load(const char *, int *w, int *h);
int axia_image_write_png(const char *, int w, int h, const uint8_t *rgba);
typedef int (*AxiaCodecWrite)(void *, const uint8_t *, size_t);
/* quality=0: PNG; quality=1..100: JPEG */
int axia_image_encode(AxiaCodecWrite, void *, const uint8_t *, int w, int h, int channels, int quality);
uint32_t *axia_vault_decode_argb(const uint8_t *, size_t, int *w, int *h);
/* Opaque font handle; masks are allocated with the ordinary C allocator. */
typedef struct AxiaRustFont AxiaRustFont;
AxiaRustFont *axia_font_open(const uint8_t *, size_t, uint32_t index, int *ascent, int *descent, int *gap, int *upem, int *offset);
void axia_font_close(AxiaRustFont *);
uint32_t axia_font_glyph(const AxiaRustFont *, uint32_t codepoint);
int axia_font_kern(const AxiaRustFont *, uint32_t left_glyph, uint32_t right_glyph);
uint8_t *axia_font_raster(const AxiaRustFont *, uint32_t glyph, float size, int *w, int *h, int *x, int *y, float *advance);

#endif
