/* errcode.h — stable, translatable error and status codes (header only).
 *
 * Nothing that reaches the UI is prose: C code reports a *code line* and the UI translates it
 * with t('errors.<code>', params) (sdk/errors.js). Logs and the journal stay English prose.
 *
 *   code line := code (' ' key '=' value)*
 *   code      := [a-z][a-zA-Z0-9]* ('.' [a-zA-Z0-9]+)+       e.g. pkg.removeEssential
 *   key       := [a-zA-Z][a-zA-Z0-9]*
 *   value     := any bytes; '%', ' ', '=', '"' and bytes < 0x20 or 0x7f are written as %XX
 *
 *   char err[256];
 *   lerr_set(err, sizeof err, "pkg.removeEssential", "name", n->name, NULL);
 *     → "pkg.removeEssential name=dpkg"
 *   lerr_set(err, sizeof err, "fs.readFailed", "path", p, "errno", lerr_errno(errno), NULL);
 *
 * Two parameter names have a meaning for the UI:
 *   errno=ENOENT    replaced by the translated description of that errno (errors.errno.ENOENT);
 *   reason=<line>   a nested code line, translated too ("could not install: <reason>").
 * The locale strings live in assets/locales/<lang>/errors.json (status.json for OK/LOG
 * statuses); tools/i18n-check.mjs checks that every code used here has an English string. */
#ifndef LORD_ERRCODE_H
#define LORD_ERRCODE_H

#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Is s[0..n) a valid code? */
static inline int lerr_valid_code(const char *s, size_t n)
{
    if (!n || s[0] < 'a' || s[0] > 'z') return 0;
    int dots = 0, seg = 0;
    for (size_t i = 0; i < n; i++) {
        char ch = s[i];
        if (ch == '.') { if (!seg) return 0; dots++; seg = 0; continue; }
        if (!((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9'))) return 0;
        seg++;
    }
    return dots > 0 && seg > 0;
}

/* Does the line start with a code (followed by a space or the end)? */
static inline int lerr_is(const char *line)
{
    if (!line) return 0;
    size_t n = strcspn(line, " ");
    return lerr_valid_code(line, n);
}

/* Is the line's code exactly `code`? (tests and C consumers match on this, never on prose) */
static inline int lerr_has(const char *line, const char *code)
{
    if (!line || !code) return 0;
    size_t n = strlen(code);
    return !strncmp(line, code, n) && (line[n] == 0 || line[n] == ' ') && lerr_is(line);
}

static inline size_t lerr__put(char *out, size_t cap, size_t at, const char *s, int encode)
{
    static const char hex[] = "0123456789ABCDEF";
    for (; s && *s; s++) {
        unsigned char ch = (unsigned char)*s;
        int esc = encode && (ch < 0x20 || ch == 0x7f || ch == '%' || ch == ' ' || ch == '=' || ch == '"');
        size_t need = esc ? 3 : 1;
        if (at + need >= cap) return (size_t)-1;
        if (esc) { out[at++] = '%'; out[at++] = hex[ch >> 4]; out[at++] = hex[ch & 15]; }
        else out[at++] = (char)ch;
    }
    return at;
}

/* Writes "code k=v k=v" into out; the arguments after code are (key, value) pairs ending in NULL.
 * A NULL value is written as empty. A parameter that does not fit is dropped (the code always is
 * kept). Returns out. */
static inline const char *lerr_vset(char *out, size_t cap, const char *code, va_list ap)
{
    if (!out || !cap) return out;
    size_t at = lerr__put(out, cap, 0, code, 0);
    if (at == (size_t)-1) { snprintf(out, cap, "%s", code); return out; }
    for (;;) {
        const char *k = va_arg(ap, const char *);
        if (!k) break;
        const char *v = va_arg(ap, const char *);
        size_t a = lerr__put(out, cap, at, " ", 0);
        if (a != (size_t)-1) a = lerr__put(out, cap, a, k, 0);
        if (a != (size_t)-1) a = lerr__put(out, cap, a, "=", 0);
        if (a != (size_t)-1) a = lerr__put(out, cap, a, v ? v : "", 1);
        if (a != (size_t)-1) at = a;
    }
    out[at] = 0;
    return out;
}

__attribute__((sentinel))
static inline const char *lerr_set(char *out, size_t cap, const char *code, ...)
{
    va_list ap;
    va_start(ap, code);
    lerr_vset(out, cap, code, ap);
    va_end(ap);
    return out;
}

/* A number as a parameter value: lerr_set(err, n, "pkg.tooMany", "count", lerr_int(n), NULL).
 * Rotating per-thread buffers: valid for the next 7 calls. */
static inline const char *lerr_int(long long v)
{
    static _Thread_local char buf[8][24];
    static _Thread_local unsigned next;
    char *b = buf[next++ & 7];
    snprintf(b, sizeof buf[0], "%lld", v);
    return b;
}

/* errno as a parameter value ("ENOENT"); the UI shows its translated description. */
static inline const char *lerr_errno(int e)
{
#if defined(__GLIBC__) && (__GLIBC__ > 2 || (__GLIBC__ == 2 && __GLIBC_MINOR__ >= 32))
    const char *n = strerrorname_np(e);
    if (n) return n;
#endif
    return lerr_int(e);
}

/* Decoded value of parameter `key` of a code line into out; returns out, or NULL if it is not there. */
static inline const char *lerr_param(const char *line, const char *key, char *out, size_t cap)
{
    if (!line || !key || !out || !cap || !lerr_is(line)) return NULL;
    size_t kl = strlen(key);
    for (const char *p = strchr(line, ' '); p; p = strchr(p + 1, ' ')) {
        if (strncmp(p + 1, key, kl) || p[1 + kl] != '=') continue;
        const char *v = p + 2 + kl;
        size_t o = 0;
        for (; *v && *v != ' ' && o + 1 < cap; v++) {
            if (v[0] == '%' && v[1] && v[2]) {
                char h[3] = { v[1], v[2], 0 };
                out[o++] = (char)strtol(h, NULL, 16);
                v += 2;
            } else out[o++] = *v;
        }
        out[o] = 0;
        return out;
    }
    return NULL;
}

#endif
