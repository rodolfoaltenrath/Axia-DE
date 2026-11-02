/* text_test.c — renders text with the UI engine (text.c + textlayout.c) into a
 * PPM image, headless. Useful for checking bidi, Arabic, CJK and font fallback.
 *   build/text-test out.ppm "text" ["text"…]
 *   LORD_WRAP=220 build/text-test …   instead, each text in a 220 px box (line breaking)
 * Fallback fonts are looked up as in LordOS (/usr/share/fonts, ~/.fonts…). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "ui/fonts.h"
#include "ui/ui.h"

Ui ui;
void text_layout_dump(const char *s, int font, float size);

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: text-test out.ppm \"text\"…\n"); return 2; }
    const char *dir = getenv("LORD_FONTS") ? getenv("LORD_FONTS") : "../assets/fonts";
    fonts_set_builtin_dir(dir);
    if (text_init(dir)) return 1;
    if (getenv("LORD_DUMP")) { for (int i = 2; i < argc; i++) text_layout_dump(argv[i], 0, 18); return 0; }
    int wrap = getenv("LORD_WRAP") ? atoi(getenv("LORD_WRAP")) : 0;
    int W = 640, LH = 34, H = wrap ? 8 + (argc - 2) * 180 : LH * (argc - 2) * 2 + 8;
    uint32_t *px = malloc(sizeof *px * (size_t)W * (size_t)H);
    Canvas c;
    canvas_init(&c, px, W, H, W);
    canvas_clear(&c, 0x202020);
    for (int i = 2; wrap && i < argc; i++) {
        float y = 4 + (float)(i - 2) * 180;
        canvas_rrect(&c, 9, y, (float)wrap + 2, 176, 0, 0xff303848u);
        text_draw(&c, argv[i], 400, 18, 0, 10, y, (float)wrap, TEXT_LEFT, 0, 0xffffffffu);
    }
    for (int i = 2; !wrap && i < argc; i++) {
        float y = 4 + (float)(i - 2) * 2 * LH;
        text_draw(&c, argv[i], 400, 18, 0, 10, y, W - 20, TEXT_LEFT, 1, 0xffffffffu);
        text_draw(&c, argv[i], 600, 18, 0, 10, y + LH, W - 20, TEXT_RIGHT, 1, 0xff80c0ffu);
    }
    FILE *f = fopen(argv[1], "wb");
    if (!f) return 1;
    fprintf(f, "P6\n%d %d\n255\n", W, H);
    for (int k = 0; k < W * H; k++) { unsigned char rgb[3] = { px[k] >> 16 & 255, px[k] >> 8 & 255, px[k] & 255 }; fwrite(rgb, 1, 3, f); }
    fclose(f);
    return 0;
}
