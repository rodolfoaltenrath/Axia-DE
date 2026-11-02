/* imapio.c — reading one complete IMAP response (line + literals) from a connection, with
 * bounded, overflow-checked sizes. Separate from imap.c so the tests can drive it over a
 * socketpair. */
#include "imapio.h"

#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* "{123}" or "{123+}" right before the final CRLF/LF: the literal size, or -1. */
long long imap_literal_size(const char *buf, size_t n) {
    size_t e = n;
    if (e >= 1 && buf[e - 1] == '\n') e--;
    if (e >= 1 && buf[e - 1] == '\r') e--;
    if (e < 3 || buf[e - 1] != '}') return -1;
    size_t close = e - 1, d = close;
    if (d > 0 && buf[d - 1] == '+') d--;
    size_t digits_end = d;
    while (d > 0 && isdigit((unsigned char)buf[d - 1])) d--;
    if (d == digits_end || d == 0 || buf[d - 1] != '{') return -1;
    if (digits_end - d > 12) return -2;              /* ≥ 10^12: hostile */
    long long v = 0;
    for (size_t i = d; i < digits_end; i++) v = v * 10 + (buf[i] - '0');
    return v;
}

char *imap_read_response(Conn *c, size_t *len, size_t line_max, size_t literal_max, char *err, size_t ecap) {
    size_t n = 0, cap = 0, total_max = line_max + literal_max * 4;
    char *buf = NULL;
    for (int parts = 0; parts < 10000; parts++) {
        size_t ll;
        char *line = conn_line(c, &ll, line_max);
        if (!line) { free(buf); snprintf(err, ecap, "%s", conn_error(c)); return NULL; }
        if (ll > total_max || n > total_max - ll) { free(line); free(buf); snprintf(err, ecap, "response too big"); return NULL; }
        if (n + ll + 1 > cap) {
            size_t nc = (n + ll + 1) * 2;
            char *nb = realloc(buf, nc);
            if (!nb) { free(line); free(buf); snprintf(err, ecap, "memory"); return NULL; }
            buf = nb;
            cap = nc;
        }
        memcpy(buf + n, line, ll);
        n += ll;
        buf[n] = 0;                                   /* n + 1 <= cap */
        free(line);
        long long lit = imap_literal_size(buf, n);
        if (lit == -1) { *len = n; return buf; }
        if (lit < 0 || (unsigned long long)lit > literal_max || n > total_max - (size_t)lit) { free(buf); snprintf(err, ecap, "literal too big"); return NULL; }
        if (conn_read_n(c, (size_t)lit, &buf, &n, &cap)) { free(buf); snprintf(err, ecap, "%s", conn_error(c)); return NULL; }
    }
    free(buf);
    snprintf(err, ecap, "too many literals");
    return NULL;
}
