/* xz.h — in-memory decompression of .xz (LZMA2, our own) and .gz (stb_image's inflate). */
#ifndef LORD_PKG_XZ_H
#define LORD_PKG_XZ_H

#include <stddef.h>
#include <stdint.h>

/* 0 ok; *out with malloc. max: maximum accepted output size. */
int xz_decode(const uint8_t *in, size_t len, uint8_t **out, size_t *outlen, size_t max);
int gz_decode(const uint8_t *in, size_t len, uint8_t **out, size_t *outlen, size_t max);

#endif
