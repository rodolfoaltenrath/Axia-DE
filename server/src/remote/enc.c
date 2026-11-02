/* enc.c — PNG for the remote desktop (lord-sessiond's bridge), with vendored stb_image_write: the
 * lossless refresh of still areas that goes over the video's data channel, and the apps' cursor images.
 * The screen itself only travels as WebRTC video (docs/REMOTE.md, "Video (WebRTC)"). */
#define STB_IMAGE_WRITE_IMPLEMENTATION
#define STBI_WRITE_NO_STDIO
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wsign-compare"
#pragma GCC diagnostic ignored "-Wunused-parameter"
#pragma GCC diagnostic ignored "-Wmissing-field-initializers"
#include "stb_image_write.h"
#pragma GCC diagnostic pop

#include "sessiond.h"

#include <stdlib.h>

static unsigned char *to_rgb(const uint32_t *px, int stride, int w, int h) {
    unsigned char *rgb = malloc((size_t)w * h * 3);
    if (!rgb) return NULL;
    unsigned char *o = rgb;
    for (int y = 0; y < h; y++) {
        const uint32_t *row = px + (size_t)y * stride;
        for (int x = 0; x < w; x++) {
            uint32_t v = row[x];
            *o++ = (unsigned char)(v >> 16);
            *o++ = (unsigned char)(v >> 8);
            *o++ = (unsigned char)v;
        }
    }
    return rgb;
}

unsigned char *rd_encode_png(const uint32_t *px, int stride, int w, int h, int *len) {
    *len = 0;
    unsigned char *rgb = to_rgb(px, stride, w, h);
    if (!rgb) return NULL;
    stbi_write_png_compression_level = 2;
    int n = 0;
    unsigned char *png = stbi_write_png_to_mem(rgb, w * 3, w, h, 3, &n);
    free(rgb);
    *len = png ? n : 0;
    return png;
}

unsigned char *rd_encode_rgba(const uint32_t *px, int w, int h, int *len) {
    unsigned char *rgba = malloc((size_t)w * h * 4);
    if (!rgba) return NULL;
    for (int i = 0; i < w * h; i++) {
        uint32_t v = px[i];
        unsigned a = v >> 24;
        /* the cursor image comes premultiplied: convert back to the real color */
        unsigned r = (v >> 16) & 255, g = (v >> 8) & 255, bl = v & 255;
        if (a && a < 255) { r = r * 255 / a; g = g * 255 / a; bl = bl * 255 / a; }
        rgba[i * 4] = (unsigned char)(r > 255 ? 255 : r);
        rgba[i * 4 + 1] = (unsigned char)(g > 255 ? 255 : g);
        rgba[i * 4 + 2] = (unsigned char)(bl > 255 ? 255 : bl);
        rgba[i * 4 + 3] = (unsigned char)a;
    }
    unsigned char *png = stbi_write_png_to_mem(rgba, w * 4, w, h, 4, len);
    free(rgba);
    return png;
}
