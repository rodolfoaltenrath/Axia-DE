/* snpf.c — see snpf.h. */
#include "snpf.h"
#include <stdarg.h>
#include <stdio.h>

void snpf(char *dst, size_t cap, const char *fmt, ...) {
    if (!cap) return;
    va_list ap;
    va_start(ap, fmt);
    if (vsnprintf(dst, cap, fmt, ap) < 0) dst[0] = 0;
    va_end(ap);
}
