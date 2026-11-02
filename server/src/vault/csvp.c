/* csvp.c — CSV reader (see csvp.h). */
#include "csvp.h"
#include "smem.h"

#include <string.h>

void csv_init(Csv *c, const char *s, size_t len) {
    c->s = s;
    c->len = len;
    c->p = len >= 3 && !memcmp(s, "\xef\xbb\xbf", 3) ? 3 : 0;
    c->bad = 0;
}

void csv_row_free(CsvRow *row) {
    for (int i = 0; i < row->n; i++) sm_free(row->f[i]);
    sm_free(row->f);
    row->f = NULL;
    row->n = row->cap = 0;
}

static int push(CsvRow *row, const char *s, size_t a, size_t b, int quoted) {
    if (row->n == row->cap) {
        int nc = row->cap ? row->cap * 2 : 16;
        if (nc > 4096) return -1;
        char **f = sm_realloc(row->f, sizeof(char *) * (size_t)nc);
        if (!f) return -1;
        row->f = f;
        row->cap = nc;
    }
    char *d = sm_alloc(b - a + 1);
    if (!d) return -1;
    size_t o = 0;
    for (size_t i = a; i < b; i++) {
        d[o++] = s[i];
        if (quoted && s[i] == '"' && i + 1 < b && s[i + 1] == '"') i++;
    }
    d[o] = 0;
    row->f[row->n++] = d;
    return 0;
}

int csv_next(Csv *c, CsvRow *row) {
    csv_row_free(row);
    const char *s = c->s;
    for (;;) {
        if (c->p >= c->len) return 0;
        /* empty row */
        if (s[c->p] == '\n') { c->p++; continue; }
        if (s[c->p] == '\r') { c->p++; if (c->p < c->len && s[c->p] == '\n') c->p++; continue; }
        break;
    }
    for (;;) {
        size_t p = c->p;
        if (p < c->len && s[p] == '"') {
            size_t a = ++p;
            for (;;) {
                if (p >= c->len) { c->bad = 1; csv_row_free(row); c->p = c->len; return -1; }
                if (s[p] == '"') {
                    if (p + 1 < c->len && s[p + 1] == '"') { p += 2; continue; }
                    break;
                }
                p++;
            }
            if (push(row, s, a, p, 1)) { csv_row_free(row); return -1; }
            p++;
            /* garbage between the closing quote and the separator: ignored (like sloppy exporters) */
            while (p < c->len && s[p] != ',' && s[p] != '\n' && s[p] != '\r') p++;
        } else {
            size_t a = p;
            while (p < c->len && s[p] != ',' && s[p] != '\n' && s[p] != '\r') p++;
            if (push(row, s, a, p, 0)) { csv_row_free(row); return -1; }
        }
        if (p < c->len && s[p] == ',') { c->p = p + 1; if (c->p >= c->len) { if (push(row, s, 0, 0, 0)) { csv_row_free(row); return -1; } return 1; } continue; }
        if (p < c->len && s[p] == '\r') p++;
        if (p < c->len && s[p] == '\n') p++;
        c->p = p;
        return 1;
    }
}

static int ieq_trim(const char *a, const char *b) {
    while (*a == ' ' || *a == '\t') a++;
    size_t n = strlen(a);
    while (n && (a[n - 1] == ' ' || a[n - 1] == '\t')) n--;
    if (strlen(b) != n) return 0;
    for (size_t i = 0; i < n; i++) {
        char x = a[i], y = b[i];
        if (x >= 'A' && x <= 'Z') x = (char)(x + 32);
        if (y >= 'A' && y <= 'Z') y = (char)(y + 32);
        if (x != y) return 0;
    }
    return 1;
}

int csv_col(const CsvRow *hdr, const char *name) {
    for (int i = 0; i < hdr->n; i++) if (ieq_trim(hdr->f[i], name)) return i;
    return -1;
}

const char *csv_get(const CsvRow *row, int i) { return i >= 0 && i < row->n ? row->f[i] : ""; }
