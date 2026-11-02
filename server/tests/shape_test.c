/* shape_test.c — headless test of OpenType shaping (otf.c, shape.c). Prints the
 * resulting glyphs for comparison with HarfBuzz. Usage:
 *   build/shape-test font.ttf liga|ccmp "text"        terminal: GSUB only, glyph ids
 *   build/shape-test font.ttf ui <script> <rtl> "text" [.ttc index]
 *        UI: GSUB + GPOS; "glyph@x,y|…" in visual order and font units */
#include "ui/otf.h"
#include "ui/shape.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

static uint32_t next_cp(const unsigned char **p) {
    const unsigned char *s = *p;
    uint32_t c = *s++;
    int n = c >= 0xf0 ? 3 : c >= 0xe0 ? 2 : c >= 0xc0 ? 1 : 0;
    if (n) c &= 0x3f >> n;
    while (n-- && *s) c = c << 6 | (*s++ & 0x3f);
    *p = s;
    return c;
}

int main(int argc, char **argv) {
    if (argc < 4) return 2;
    int fd = open(argv[1], O_RDONLY);
    struct stat st;
    if (fd < 0 || fstat(fd, &st)) return 1;
    uint8_t *d = mmap(NULL, (size_t)st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    Otf o;
    if (!strcmp(argv[2], "ui") && argc >= 6) {
        int index = argc > 6 ? atoi(argv[6]) : 0, off = 0;
        if (!memcmp(d, "ttcf", 4)) off = (int)((uint32_t)d[12 + 4 * index] << 24 | (uint32_t)d[13 + 4 * index] << 16 | (uint32_t)d[14 + 4 * index] << 8 | d[15 + 4 * index]);
        otf_init(&o, d, (size_t)st.st_size, off);
        uint32_t cp[1024];
        int n = 0;
        for (const unsigned char *p = (const unsigned char *)argv[5]; *p && n < 1024;) cp[n++] = next_cp(&p);
        Shaper sh = { 0 };
        int rtl = atoi(argv[4]);
        char script[5] = "    ";
        memcpy(script, argv[3], strlen(argv[3]) < 4 ? strlen(argv[3]) : 4);
        int len = shape_run(&sh, &o, cp, n, script, rtl);
        int x[1024], y[1024], pen = 0;
        for (int k = 0; k < len; k++) {             /* first the ones that are not anchored */
            int i = rtl ? len - 1 - k : k;
            if (sh.pos[i].att >= 0) continue;
            x[i] = pen + sh.pos[i].dx; y[i] = sh.pos[i].dy;
            pen += sh.pos[i].adv;
        }
        for (int i = 0; i < len; i++) if (sh.pos[i].att >= 0) { int b = sh.pos[i].att; x[i] = x[b] + sh.pos[i].dx; y[i] = y[b] + sh.pos[i].dy; }
        for (int k = 0; k < len; k++) { int i = rtl ? len - 1 - k : k; printf("%s%u@%d,%d", k ? "|" : "", sh.run.g[i], x[i], y[i]); }
        printf("\n");
        return 0;
    }
    otf_init(&o, d, (size_t)st.st_size, 0);
    GlyphRun r = { malloc(4096 * 2), malloc(4096 * sizeof(int)), 0, 4096, NULL, NULL, NULL };
    for (const unsigned char *p = (const unsigned char *)argv[3]; *p;) {
        uint32_t cp = next_cp(&p);
        r.cl[r.len] = r.len;
        r.g[r.len++] = otf_cmap(&o, cp);
    }
    otf_shape(&o, strcmp(argv[2], "ccmp") ? OTF_LIGA : OTF_CCMP, &r);
    for (int i = 0; i < r.len; i++) printf("%s%u", i ? "|" : "", r.g[i]);
    printf("\n");
    return 0;
}
