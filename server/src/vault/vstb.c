/* vstb.c — stb_image using smem memory (see vstb.h). */
#include "vstb.h"
#include "smem.h"

#include <string.h>

#define STBI_MALLOC(sz) sm_alloc(sz)
#define STBI_REALLOC(p, sz) sm_realloc(p, sz)
#define STBI_FREE(p) sm_free(p)
#define STB_IMAGE_STATIC
#define STB_IMAGE_IMPLEMENTATION
#define STBI_ONLY_PNG
#define STBI_ONLY_JPEG
#define STBI_NO_STDIO
#define STBI_NO_FAILURE_STRINGS
#define STBI_NO_GIF
#define STBI_NO_HDR
#define STBI_NO_LINEAR
#define STBI_MAX_DIMENSIONS 8192
#if defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-function"
#pragma GCC diagnostic ignored "-Wsign-compare"
#pragma GCC diagnostic ignored "-Wunused-parameter"
#pragma GCC diagnostic ignored "-Wimplicit-fallthrough"
#pragma GCC diagnostic ignored "-Wmissing-field-initializers"
#pragma GCC diagnostic ignored "-Wshift-negative-value"
#endif
#include "stb_image.h"
#if defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

uint8_t *vstb_inflate_raw(const uint8_t *in, size_t len, size_t max, size_t *outlen) {
    if (len > 0x7fffffff) return NULL;
    int n = 0;
    char *out = stbi_zlib_decode_malloc_guesssize_headerflag((const char *)in, (int)len, (int)(len * 4 + 64 < 0x7fffffff ? len * 4 + 64 : 0x7fffffff), &n, 0);
    if (!out) return NULL;
    if ((size_t)n > max) { sm_free(out); return NULL; }
    *outlen = (size_t)n;
    return (uint8_t *)out;
}

uint8_t *vstb_gunzip(const uint8_t *in, size_t len, size_t max, size_t *outlen) {
    /* 10-byte header (1f 8b 08 flg mtime xfl os) + optional fields + deflate + crc32 + isize */
    if (len < 18 || in[0] != 0x1f || in[1] != 0x8b || in[2] != 8) return NULL;
    uint8_t flg = in[3];
    size_t p = 10;
    if (flg & 4) { if (p + 2 > len) return NULL; size_t x = in[p] | (size_t)in[p + 1] << 8; p += 2 + x; }
    if (flg & 8) { while (p < len && in[p]) p++; p++; }
    if (flg & 16) { while (p < len && in[p]) p++; p++; }
    if (flg & 2) p += 2;
    if (p + 8 > len) return NULL;
    return vstb_inflate_raw(in + p, len - p - 8, max, outlen);
}

uint32_t *vstb_decode_argb(const uint8_t *in, size_t len, int *w, int *h) {
    if (len > 0x7fffffff) return NULL;
    int comp = 0;
    unsigned char *px = stbi_load_from_memory(in, (int)len, w, h, &comp, 4);
    if (!px) return NULL;
    size_t n = (size_t)*w * (size_t)*h;
    uint32_t *out = sm_alloc(n * 4);
    if (out) for (size_t i = 0; i < n; i++)
        out[i] = (uint32_t)px[4 * i + 3] << 24 | (uint32_t)px[4 * i] << 16 | (uint32_t)px[4 * i + 1] << 8 | px[4 * i + 2];
    sm_free(px);
    return out;
}
