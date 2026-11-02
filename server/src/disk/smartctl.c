/* smartctl.c — pure helpers for the smart-* actions (smartctl_admin.c), tested in
 * tests/smartctl_test.c: which device names smartctl may be pointed at, and the compaction of its
 * JSON so it travels in few LOG lines. No I/O here. */
#include "disk.h"

#include <ctype.h>
#include <string.h>

/* sd<letters> / hd<letters> (SATA, SAS, USB disks behind a bridge) and nvme<N>n<M> (an NVMe
 * namespace). Nothing else: no partitions (sda1, nvme0n1p1), loop, dm, md, zram, mmcblk (cards
 * have no SMART), sr (optical), and no path characters at all. */
int smartctl_valid_disk(const char *n) {
    if (!n) return 0;
    size_t l = strlen(n);
    if (l < 3 || l > 15) return 0;
    if ((n[0] == 's' || n[0] == 'h') && n[1] == 'd') {
        if (l > 5) return 0;                            /* sda … sdzzz */
        for (size_t i = 2; i < l; i++) if (n[i] < 'a' || n[i] > 'z') return 0;
        return 1;
    }
    if (strncmp(n, "nvme", 4)) return 0;
    size_t i = 4, d = 0;
    while (i < l && isdigit((unsigned char)n[i])) { i++; d++; }
    if (!d || d > 3 || i >= l || n[i] != 'n') return 0;
    if (d > 1 && n[4] == '0') return 0;                 /* no leading zeros */
    i++;
    size_t start = i;
    d = 0;
    while (i < l && isdigit((unsigned char)n[i])) { i++; d++; }
    if (!d || d > 3 || i != l) return 0;
    if (d > 1 && n[start] == '0') return 0;
    if (n[start] == '0') return 0;                      /* namespaces start at 1 */
    return 1;
}

/* Removes the whitespace outside JSON strings, in place. Returns the new length. Raw control
 * characters (which valid JSON never has outside strings, and smartctl escapes inside them)
 * become spaces inside strings and are dropped outside. */
size_t smartctl_compact(char *s, size_t n) {
    size_t o = 0;
    int in = 0, esc = 0;
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (!c) break;
        if (in) {
            if (c < 0x20) c = ' ';
            s[o++] = (char)c;
            if (esc) esc = 0;
            else if (c == '\\') esc = 1;
            else if (c == '"') in = 0;
            continue;
        }
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r' || c < 0x20) continue;
        if (c == '"') in = 1;
        s[o++] = (char)c;
    }
    if (o < n) s[o] = 0;
    return o;
}

/* Cut point for a LOG line of at most max bytes from s[0..n): never inside a UTF-8 sequence. */
size_t smartctl_chunk(const char *s, size_t n, size_t max) {
    if (n <= max) return n;
    size_t k = max;
    while (k > 1 && ((unsigned char)s[k] & 0xc0) == 0x80) k--;
    return k;
}
