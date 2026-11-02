/* jbuf.c — growable JSON buffer (see jbuf.h). */
#include "jbuf.h"
#include "../sec/json.h"

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int reserve(HBuf *b, size_t more) {
    if (b->oom) return -1;
    if (b->n + more + 1 <= b->cap) return 0;
    size_t cap = b->cap ? b->cap : 256;
    while (cap < b->n + more + 1) cap *= 2;
    char *p = realloc(b->p, cap);
    if (!p) { b->oom = 1; return -1; }
    b->p = p;
    b->cap = cap;
    return 0;
}

void hb_printf(HBuf *b, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(NULL, 0, fmt, ap);
    va_end(ap);
    if (n < 0 || reserve(b, (size_t)n)) return;
    va_start(ap, fmt);
    vsnprintf(b->p + b->n, b->cap - b->n, fmt, ap);
    va_end(ap);
    b->n += (size_t)n;
}

void hb_str(HBuf *b, const char *s) {
    size_t worst = strlen(s) * 6 + 12;     /* json_quote keeps 8 bytes of slack */
    if (reserve(b, worst)) return;
    b->n += json_quote(b->p + b->n, b->cap - b->n, s);
}

char *hb_take(HBuf *b) {
    if (b->oom) { free(b->p); b->p = NULL; return NULL; }
    if (!b->p && reserve(b, 0)) return NULL;
    b->p[b->n] = 0;
    char *p = b->p;
    b->p = NULL;
    b->n = b->cap = 0;
    return p;
}
