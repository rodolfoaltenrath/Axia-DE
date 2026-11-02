/* vgen.c — password generator and strength (see vgen.h). */
#include "vgen.h"
#include "vcrypto.h"
#include "smem.h"

#include <ctype.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static const char **words;
static size_t nwords;
static const char *common_buf;
static size_t common_len;
/* index of the common list: start of each line (the list is in popularity order) */
static const char **common;
static size_t ncommon;

static void common_index(void);
static size_t split_lines(const char *buf, size_t len, const char ***out) {
    size_t n = 0;
    for (size_t i = 0; i < len; i++) if (buf[i] == '\n') n++;
    const char **v = calloc(n + 1, sizeof *v);
    if (!v) return 0;
    size_t k = 0, start = 0;
    for (size_t i = 0; i <= len && k <= n; i++) {
        if (i == len || buf[i] == '\n') { if (i > start) v[k++] = buf + start; start = i + 1; }
    }
    *out = v;
    return k;
}

static size_t line_len(const char *p, const char *end) { const char *q = p; while (q < end && *q != '\n' && *q != '\r') q++; return (size_t)(q - p); }

void vg_set_lists(const char *w, size_t wlen, const char *c, size_t clen) {
    free(words); free(common);
    nwords = split_lines(w, wlen, &words);
    common_buf = c;
    common_len = clen;
    ncommon = split_lines(c, clen, &common);
    common_index();
    /* the EFF words: they end with '\n'; we copy them without it into our own block */
    for (size_t i = 0; i < nwords; i++) {
        size_t l = line_len(words[i], w + wlen);
        char *s = malloc(l + 1);
        if (!s) { nwords = i; break; }
        memcpy(s, words[i], l);
        s[l] = 0;
        words[i] = s;
    }
}

size_t vg_word_count(void) { return nwords; }

static uint32_t rnd_below(uint32_t n) {
    if (n <= 1) return 0;
    uint32_t lim = UINT32_MAX - UINT32_MAX % n, r;
    do { vc_random(&r, sizeof r); } while (r >= lim);
    return r % n;
}

int vg_generate(const VGenOpts *o, char *out, size_t cap, double *bits) {
    if (o->words > 0) {
        if (!nwords) return -1;
        int nw = o->words < 3 ? 3 : o->words > 12 ? 12 : o->words;
        int dig_at = o->add_digit ? (int)rnd_below((uint32_t)nw) : -1;
        size_t k = 0;
        for (int i = 0; i < nw; i++) {
            const char *w = words[rnd_below((uint32_t)nwords)];
            size_t l = strlen(w), sl = i ? strlen(o->sep) : 0;
            if (k + sl + l + 2 >= cap) return -1;
            if (sl) { memcpy(out + k, o->sep, sl); k += sl; }
            memcpy(out + k, w, l);
            if (o->capitalize) out[k] = (char)toupper((unsigned char)out[k]);
            k += l;
            if (i == dig_at) out[k++] = (char)('0' + rnd_below(10));
        }
        out[k] = 0;
        if (bits) *bits = nw * log2((double)nwords) + (o->add_digit ? log2(10.0) + log2((double)nw) : 0);
        return 0;
    }
    static const char U[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZ", L[] = "abcdefghijklmnopqrstuvwxyz", D[] = "0123456789",
                      S[] = "!#$%&*+-=?@^_~.:,;()[]{}<>/|";
    char pool[160];
    size_t pn = 0;
    const char *sets[4] = { o->upper ? U : NULL, o->lower ? L : NULL, o->digits ? D : NULL, o->symbols ? S : NULL };
    char cls[4][40];
    int ncls = 0;
    for (int s = 0; s < 4; s++) {
        if (!sets[s]) continue;
        size_t cn = 0;
        for (const char *p = sets[s]; *p; p++) {
            if (o->no_ambiguous && strchr("Il1O0o|", *p)) continue;
            pool[pn++] = *p;
            cls[ncls][cn++] = *p;
        }
        cls[ncls][cn] = 0;
        ncls++;
    }
    if (!pn) return -1;
    int len = o->length < 4 ? 4 : o->length > 128 ? 128 : o->length;
    if ((size_t)len + 1 > cap) return -1;
    for (int tries = 0; tries < 1000; tries++) {
        for (int i = 0; i < len; i++) out[i] = pool[rnd_below((uint32_t)pn)];
        out[len] = 0;
        int ok = 1;                              /* at least one of each requested class */
        for (int c = 0; c < ncls && ok; c++) if (!strpbrk(out, cls[c])) ok = 0;
        if (ok || len < ncls) break;
    }
    if (bits) *bits = len * log2((double)pn);
    sm_wipe(pool, sizeof pool);
    return 0;
}

/* ---- common ---- */

/* hash table (open addressing) of the common list: O(1) lookup */
#define CHASH 32768
static int32_t ctab[CHASH];        /* index + 1, 0 = free */

static uint32_t fnv(const char *s, size_t n) {
    uint32_t h = 2166136261u;
    for (size_t i = 0; i < n; i++) { h ^= (uint8_t)s[i]; h *= 16777619u; }
    return h;
}

static void common_index(void) {
    memset(ctab, 0, sizeof ctab);
    for (size_t i = 0; i < ncommon && i < CHASH / 2; i++) {
        size_t l = line_len(common[i], common_buf + common_len);
        uint32_t h = fnv(common[i], l) & (CHASH - 1);
        while (ctab[h]) h = (h + 1) & (CHASH - 1);
        ctab[h] = (int32_t)i + 1;
    }
}

static int common_exact(const char *s, size_t n) {
    if (!ncommon) return 0;
    uint32_t h = fnv(s, n) & (CHASH - 1);
    while (ctab[h]) {
        const char *c = common[ctab[h] - 1];
        size_t l = line_len(c, common_buf + common_len);
        if (l == n && !memcmp(c, s, n)) return ctab[h];
        h = (h + 1) & (CHASH - 1);
    }
    return 0;
}

int vg_common_rank(const char *pw) {
    size_t n = strlen(pw);
    if (!n || n > 64 || !ncommon) return 0;
    char low[72], unleet[72];
    for (size_t i = 0; i < n; i++) low[i] = (char)tolower((unsigned char)pw[i]);
    low[n] = 0;
    int r = common_exact(pw, n);
    if (!r) r = common_exact(low, n);
    /* without the trailing part people usually add: password1!, qwerty2024 */
    size_t m = n;
    while (m > 3 && (isdigit((unsigned char)low[m - 1]) || ispunct((unsigned char)low[m - 1]))) m--;
    if (!r && m < n) r = common_exact(low, m);
    if (!r) {
        for (size_t i = 0; i < m; i++) {
            char c = low[i];
            unleet[i] = c == '0' ? 'o' : c == '1' ? 'i' : c == '3' ? 'e' : c == '4' || c == '@' ? 'a' : c == '5' || c == '$' ? 's' : c == '7' ? 't' : c;
        }
        unleet[m] = 0;
        r = common_exact(unleet, m);
        if (!r) {                                /* 1 can also be l */
            for (size_t i = 0; i < m; i++) if (low[i] == '1') unleet[i] = 'l';
            r = common_exact(unleet, m);
        }
    }
    sm_wipe(low, sizeof low);
    sm_wipe(unleet, sizeof unleet);
    return r;
}

/* ---- strength (simple zxcvbn-style estimate: patterns that lower entropy) ---- */

static const char *ROWS[] = { "1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm", "qwertzuiop", "azertyuiop", NULL };

static int keyboard_adjacent(char a, char b) {
    a = (char)tolower((unsigned char)a);
    b = (char)tolower((unsigned char)b);
    for (int r = 0; ROWS[r]; r++) {
        const char *p = strchr(ROWS[r], a), *q = strchr(ROWS[r], b);
        if (p && q && (p - q == 1 || q - p == 1)) return 1;
    }
    return 0;
}

int vg_strength(const char *pw, double *bits_out) {
    size_t n = strlen(pw);
    if (!n) { if (bits_out) *bits_out = 0; return 0; }
    int lo = 0, up = 0, di = 0, sy = 0, other = 0;
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)pw[i];
        if (c >= 0x80) other = 1;
        else if (islower(c)) lo = 1;
        else if (isupper(c)) up = 1;
        else if (isdigit(c)) di = 1;
        else sy = 1;
    }
    double pool = lo * 26 + up * 26 + di * 10 + sy * 33 + other * 100;
    if (pool < 10) pool = 10;
    double per = log2(pool), bits = 0;
    for (size_t i = 0; i < n; i++) {
        if (i && pw[i] == pw[i - 1]) { bits += 1; continue; }                          /* aaaa */
        if (i && (pw[i] - pw[i - 1] == 1 || pw[i - 1] - pw[i] == 1)) { bits += 1.5; continue; }   /* abcd, 4321 */
        if (i && keyboard_adjacent(pw[i], pw[i - 1])) { bits += 2; continue; }       /* qwerty */
        bits += per;
    }
    /* years (19xx/20xx) and dates count for little */
    for (size_t i = 0; i + 4 <= n; i++)
        if ((!strncmp(pw + i, "19", 2) || !strncmp(pw + i, "20", 2)) && isdigit((unsigned char)pw[i + 2]) && isdigit((unsigned char)pw[i + 3])) { bits -= 2 * per - 7; break; }
    int rank = vg_common_rank(pw);
    if (rank) bits = log2((double)rank + 1) + 1;
    if (bits < 0) bits = 0;
    if (bits_out) *bits_out = bits;
    if (rank || bits < 28) return 0;
    if (bits < 36) return 1;
    if (bits < 60) return 2;
    if (bits < 80) return 3;
    return 4;
}

/* ---- local HIBP ---- */

long vg_hibp_lookup(int fd, const char *pw) {
    struct stat st;
    if (fstat(fd, &st) || st.st_size < 42) return -1;
    uint8_t h[20];
    sha1(pw, strlen(pw), h);
    char hex[41];
    for (int i = 0; i < 20; i++) snprintf(hex + 2 * i, 3, "%02X", h[i]);
    off_t lo = 0, hi = st.st_size;
    char buf[256];
    for (int iter = 0; iter < 64 && lo < hi; iter++) {
        off_t mid = lo + (hi - lo) / 2;
        /* go to the start of the line after mid (or mid if it is 0) */
        ssize_t r = pread(fd, buf, sizeof buf - 1, mid);
        if (r <= 0) break;
        buf[r] = 0;
        off_t start = mid;
        if (mid) {
            char *nl = memchr(buf, '\n', (size_t)r);
            if (!nl) { hi = mid; continue; }
            start = mid + (nl - buf) + 1;
            r = pread(fd, buf, sizeof buf - 1, start);
            if (r <= 0) { hi = mid; continue; }
            buf[r] = 0;
        }
        if (r < 41) { hi = mid; continue; }
        int c = strncasecmp(buf, hex, 40);
        if (c == 0) return buf[40] == ':' ? strtol(buf + 41, NULL, 10) : 1;
        if (c < 0) lo = start + 1; else hi = mid;
    }
    /* the search may skip the first line: check it separately */
    ssize_t r = pread(fd, buf, sizeof buf - 1, 0);
    if (r >= 41) { buf[r] = 0; if (!strncasecmp(buf, hex, 40)) return strtol(buf + 41, NULL, 10); }
    return 0;
}
