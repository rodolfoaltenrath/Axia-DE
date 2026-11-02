/* thbrk_test.c — Thai word breaks (src/ui/thbrk.c, thbrk_text), one output line per
 * input line: "|" where a break is allowed. Compared with libthai in
 * tests/thbrk_compare.sh (only breaks between characters of the Thai block). */
#include "ui/thbrk.h"

#include <stdio.h>
#include <string.h>

static int decode(const unsigned char *s, uint32_t *out, int max) {
    int n = 0;
    while (*s && n < max) {
        uint32_t c = *s++;
        int k = c >= 0xf0 ? 3 : c >= 0xe0 ? 2 : c >= 0xc0 ? 1 : 0;
        if (k) c &= 0x3f >> k;
        while (k-- && *s) c = c << 6 | (*s++ & 0x3f);
        out[n++] = c;
    }
    return n;
}

static void put(uint32_t c) {
    if (c < 0x80) putchar((int)c);
    else if (c < 0x800) { putchar(0xc0 | (int)(c >> 6)); putchar(0x80 | (int)(c & 0x3f)); }
    else { putchar(0xe0 | (int)(c >> 12)); putchar(0x80 | (int)(c >> 6 & 0x3f)); putchar(0x80 | (int)(c & 0x3f)); }
}

int main(void) {
    static char line[65536];
    static uint32_t cp[16384];
    static uint8_t bk[16385];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\n")] = 0;
        int n = decode((const unsigned char *)line, cp, 16384);
        memset(bk, 0, (size_t)n + 1);
        thbrk_text(cp, n, bk);
        for (int i = 0; i < n; i++) {
            if (bk[i] == 1) putchar('|');
            put(cp[i]);
        }
        putchar('\n');
    }
    return thbrk_available() ? 0 : 1;
}
