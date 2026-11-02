/* bidi_test.c — runs the Unicode conformance test against bidi.c.
 *
 *   build/bidi-test BidiCharacterTest.txt
 *
 * (https://www.unicode.org/Public/<version>/ucd/BidiCharacterTest.txt). Each line holds the
 * characters, the paragraph direction, the resolved level, the per-character levels after
 * L1 ("x" = removed by X9) and the visual order. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../src/ui/bidi.h"

#define MAXN 512

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: bidi-test BidiCharacterTest.txt [-v]\n"); return 2; }
    FILE *f = fopen(argv[1], "r");
    if (!f) { perror(argv[1]); return 2; }
    int verbose = argc > 2;
    static char line[16384];
    long total = 0, bad = 0;
    while (fgets(line, sizeof line, f)) {
        if (line[0] == '#' || line[0] == '\n') continue;
        char *fld[5];
        char *s = line;
        int nf = 0;
        for (; nf < 5; nf++) { fld[nf] = s; char *sc = strchr(s, ';'); if (!sc) { nf++; break; } *sc = 0; s = sc + 1; }
        if (nf < 5) continue;
        uint32_t cp[MAXN];
        int n = 0;
        for (char *p = fld[0]; *p && n < MAXN;) {
            char *e;
            unsigned long v = strtoul(p, &e, 16);
            if (e == p) break;
            cp[n++] = (uint32_t)v;
            p = e;
        }
        int dir = atoi(fld[1]), want_para = atoi(fld[2]);
        int want_lv[MAXN], nl = 0;
        for (char *p = strtok(fld[3], " \n"); p && nl < MAXN; p = strtok(NULL, " \n")) want_lv[nl++] = *p == 'x' ? -1 : atoi(p);
        int want_ord[MAXN], no = 0;
        for (char *p = strtok(fld[4], " \n"); p && no < MAXN; p = strtok(NULL, " \n")) want_ord[no++] = atoi(p);

        uint8_t lv[MAXN], rm[MAXN];
        int para = bidi_levels(cp, n, dir == 2 ? -1 : dir, lv, rm);
        int order[MAXN];
        bidi_line(cp, lv, 0, n, para, order);
        int ok = para == want_para && nl == n;
        for (int i = 0; ok && i < n; i++) {
            if (want_lv[i] < 0) { if (!rm[i]) ok = 0; }
            else if (rm[i] || lv[i] != want_lv[i]) ok = 0;
        }
        int k = 0;
        for (int i = 0; ok && i < n; i++) {
            if (rm[order[i]]) continue;
            if (k >= no || order[i] != want_ord[k]) ok = 0;
            k++;
        }
        if (ok && k != no) ok = 0;
        total++;
        if (!ok) {
            bad++;
            if (verbose && bad <= 20) {
                printf("FAIL: %s; dir %d; para %d (want %d)\n  levels:", fld[0], dir, para, want_para);
                for (int i = 0; i < n; i++) printf(" %s%d", rm[i] ? "x" : "", lv[i]);
                printf("\n  quiero: ");
                for (int i = 0; i < nl; i++) { if (want_lv[i] < 0) printf(" x"); else printf(" %d", want_lv[i]); }
                printf("\n");
            }
        }
    }
    fclose(f);
    printf("BidiCharacterTest: %ld cases, %ld pass, %ld fail\n", total, total - bad, bad);
    return bad ? 1 : 0;
}
