/* search.c — the local, offline search index of Mail (see maild.h).
 *
 * Every cached message is split into folded words (lower case, without accents): subject,
 * people, the preview and, once downloaded, the body text. Each word maps to the list of
 * messages that contain it. A query is a few words; the last one also matches as a prefix
 * ("presu" finds "presupuesto"). Words are kept sorted (rebuilt lazily after changes) so a
 * prefix is a binary search, and the whole thing answers in milliseconds for tens of
 * thousands of messages without touching the network. */
#include "maild.h"

#include <stdlib.h>
#include <string.h>

typedef struct { int acct; Folder *f; uint32_t uid; int alive; } Doc;
typedef struct { char *w; int *docs; int n, cap; } Word;

static Doc *docs;
static int ndocs, capdocs;
static Word *words;              /* hash table, open addressing */
static size_t cap_words, nwords;
static Word **sorted;
static size_t nsorted;
static int sorted_dirty = 1;

static uint64_t hash(const char *s, size_t n) {
    uint64_t h = 1469598103934665603ull;
    for (size_t i = 0; i < n; i++) { h ^= (unsigned char)s[i]; h *= 1099511628211ull; }
    return h;
}

/* lower case without accents (Latin letters); other bytes are kept */
void search_fold(const char *in, char *out, size_t cap) {
    static const struct { const char *from; const char *to; } map[] = {
        { "á", "a" }, { "à", "a" }, { "â", "a" }, { "ä", "a" }, { "ã", "a" }, { "å", "a" }, { "Á", "a" }, { "À", "a" }, { "Â", "a" }, { "Ä", "a" }, { "Ã", "a" }, /* i18n-ok */
        { "é", "e" }, { "è", "e" }, { "ê", "e" }, { "ë", "e" }, { "É", "e" }, { "È", "e" }, { "Ê", "e" }, { "Ë", "e" }, /* i18n-ok */
        { "í", "i" }, { "ì", "i" }, { "î", "i" }, { "ï", "i" }, { "Í", "i" }, { "Ì", "i" }, { "Î", "i" }, { "Ï", "i" }, /* i18n-ok */
        { "ó", "o" }, { "ò", "o" }, { "ô", "o" }, { "ö", "o" }, { "õ", "o" }, { "Ó", "o" }, { "Ò", "o" }, { "Ô", "o" }, { "Ö", "o" }, { "Õ", "o" }, /* i18n-ok */
        { "ú", "u" }, { "ù", "u" }, { "û", "u" }, { "ü", "u" }, { "Ú", "u" }, { "Ù", "u" }, { "Û", "u" }, { "Ü", "u" }, /* i18n-ok */
        { "ñ", "n" }, { "Ñ", "n" }, { "ç", "c" }, { "Ç", "c" }, { "ß", "ss" }, { "ø", "o" }, { "Ø", "o" }, { "æ", "ae" }, { "œ", "oe" }, /* i18n-ok */
    };
    size_t o = 0;
    if (!cap) return;
    for (const char *s = in ? in : ""; *s && o + 3 < cap;) {
        unsigned char c = (unsigned char)*s;
        if (c < 0x80) { out[o++] = (char)((c >= 'A' && c <= 'Z') ? c + 32 : c); s++; continue; }
        int hit = 0;
        if (c == 0xc3 || c == 0xc5) {
            for (size_t k = 0; k < sizeof map / sizeof map[0]; k++) {
                size_t l = strlen(map[k].from);
                if (!strncmp(s, map[k].from, l)) { for (const char *t = map[k].to; *t && o + 1 < cap; t++) out[o++] = *t; s += l; hit = 1; break; }
            }
        }
        if (!hit) out[o++] = *s++;
    }
    out[o] = 0;
}

static int wchar(unsigned char c) { return (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c >= 0x80 || c == '_'; }

static Word *word_get(const char *w, size_t n, int create) {
    if (!cap_words) {
        if (!create) return NULL;
        cap_words = 1 << 14;
        words = calloc(cap_words, sizeof *words);
        if (!words) { cap_words = 0; return NULL; }
    }
    if (create && (nwords + 1) * 10 > cap_words * 7) {
        size_t nc = cap_words * 2;
        Word *nw = calloc(nc, sizeof *nw);
        if (!nw) return NULL;
        for (size_t i = 0; i < cap_words; i++) {
            if (!words[i].w) continue;
            size_t h = hash(words[i].w, strlen(words[i].w)) & (nc - 1);
            while (nw[h].w) h = (h + 1) & (nc - 1);
            nw[h] = words[i];
        }
        free(words);
        words = nw;
        cap_words = nc;
        sorted_dirty = 1;
    }
    size_t h = hash(w, n) & (cap_words - 1);
    while (words[h].w) {
        if (strlen(words[h].w) == n && !memcmp(words[h].w, w, n)) return &words[h];
        h = (h + 1) & (cap_words - 1);
    }
    if (!create) return NULL;
    words[h].w = strndup(w, n);
    if (!words[h].w) return NULL;
    nwords++;
    sorted_dirty = 1;
    return &words[h];
}

int search_add(const char *text, int acct, Folder *f, uint32_t uid) {
    if (ndocs == capdocs) {
        int nc = capdocs ? capdocs * 2 : 4096;
        Doc *nd = realloc(docs, (size_t)nc * sizeof *nd);
        if (!nd) return -1;
        docs = nd;
        capdocs = nc;
    }
    int id = ndocs++;
    docs[id] = (Doc){ acct, f, uid, 1 };
    const char *s = text ? text : "";
    while (*s) {
        while (*s && !wchar((unsigned char)*s)) s++;
        const char *a = s;
        while (*s && wchar((unsigned char)*s)) s++;
        size_t n = (size_t)(s - a);
        if (n < 2 || n > 40) continue;
        Word *w = word_get(a, n, 1);
        if (!w) continue;
        if (w->n && w->docs[w->n - 1] == id) continue;
        if (w->n == w->cap) {
            int nc = w->cap ? w->cap * 2 : 4;
            int *nd = realloc(w->docs, (size_t)nc * sizeof *nd);
            if (!nd) continue;
            w->docs = nd;
            w->cap = nc;
        }
        w->docs[w->n++] = id;
    }
    return id;
}

void search_remove(int doc) { if (doc >= 0 && doc < ndocs) docs[doc].alive = 0; }

static int cmpw(const void *a, const void *b) { return strcmp((*(Word *const *)a)->w, (*(Word *const *)b)->w); }

static void build_sorted(void) {
    if (!sorted_dirty) return;
    free(sorted);
    sorted = malloc((nwords ? nwords : 1) * sizeof *sorted);
    nsorted = 0;
    if (!sorted) return;
    for (size_t i = 0; i < cap_words; i++) if (words[i].w) sorted[nsorted++] = &words[i];
    qsort(sorted, nsorted, sizeof *sorted, cmpw);
    sorted_dirty = 0;
}

int search_query(const char *q, Hit *out, int max) {
    char fq[512];
    search_fold(q, fq, sizeof fq);
    char *terms[8];
    int nt = 0;
    for (char *s = fq; *s && nt < 8;) {
        while (*s && !wchar((unsigned char)*s)) s++;
        if (!*s) break;
        terms[nt++] = s;
        while (*s && wchar((unsigned char)*s)) s++;
        if (*s) *s++ = 0;
    }
    if (!nt || !ndocs) return 0;
    build_sorted();
    if (!sorted) return 0;
    unsigned char *mark = calloc((size_t)ndocs, 1);
    if (!mark) return 0;
    for (int t = 0; t < nt; t++) {
        size_t tl = strlen(terms[t]);
        int prefix = 1;               /* every word matches by prefix: "factur" → "factura", "facturas" */
        size_t lo = 0, hi = nsorted;
        while (lo < hi) { size_t mid = (lo + hi) / 2; if (strcmp(sorted[mid]->w, terms[t]) < 0) lo = mid + 1; else hi = mid; }
        for (size_t i = lo; i < nsorted; i++) {
            if (strncmp(sorted[i]->w, terms[t], tl)) break;
            if (!prefix && strlen(sorted[i]->w) != tl) continue;
            Word *w = sorted[i];
            for (int k = 0; k < w->n; k++) if (mark[w->docs[k]] == t) mark[w->docs[k]] = (unsigned char)(t + 1);
        }
    }
    int n = 0;
    for (int dd = ndocs - 1; dd >= 0 && n < max; dd--) if (mark[dd] == nt && docs[dd].alive) out[n++] = (Hit){ docs[dd].acct, docs[dd].f, docs[dd].uid };
    free(mark);
    return n;
}
