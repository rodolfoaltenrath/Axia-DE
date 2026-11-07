#ifndef AXIA_WALLPAPER_FFI_H
#define AXIA_WALLPAPER_FFI_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
/* Session-only API. UTF-8 paths are copied during the call; output is caller-owned.
 * Preview input is exactly width*height*4 RGBA bytes, borrowed only during the call.
 * The encoder callback must not retain any pointer. Rust owns paths and resampling. */
bool axia_wallpaper_import(const char *source, const char *home, const char *data_home, char *out, size_t capacity);
bool axia_wallpaper_preview(const char *path, const unsigned char *rgba, size_t width, size_t height,
    int (*encode_png)(const char *, int, int, const unsigned char *), char *out, size_t capacity);
#endif
