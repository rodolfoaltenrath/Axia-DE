/* thbrk_libthai.c — the same as thbrk_test.c but with libthai (only for comparison; not
 * part of LordOS). cc -o thbrk-libthai thbrk_libthai.c -lthai */
#include <stdio.h>
#include <string.h>
#include <wchar.h>
#include <locale.h>
#include <stdlib.h>
#include <thai/thbrk.h>
#include <thai/thwbrk.h>

int main(void) {
    setlocale(LC_ALL, "C.UTF-8");
    static char line[65536];
    static wchar_t w[16384];
    static int pos[16384];
    ThBrk *b = th_brk_new(NULL);
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\n")] = 0;
        int n = (int)mbstowcs(w, line, 16384);
        if (n < 0) continue;
        int k = th_brk_wc_find_breaks(b, w, pos, 16384);
        for (int i = 0, j = 0; i < n; i++) {
            if (j < k && pos[j] == i) { putwchar(L'|'); j++; }
            putwchar(w[i]);
        }
        putwchar(L'\n');
    }
    return 0;
}
