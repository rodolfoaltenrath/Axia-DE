/* jbuf.h — growable buffer for building JSON in src/hw (strings are escaped with
 * sec/json.c's json_quote). On out-of-memory the buffer is marked and later writes are dropped. */
#ifndef LORD_HW_JBUF_H
#define LORD_HW_JBUF_H

#include <stddef.h>

typedef struct { char *p; size_t n, cap; int oom; } HBuf;

void hb_printf(HBuf *b, const char *fmt, ...) __attribute__((format(printf, 2, 3)));
void hb_str(HBuf *b, const char *s);            /* "escaped" */
char *hb_take(HBuf *b);                          /* the string (malloc), or NULL on OOM */

#endif
