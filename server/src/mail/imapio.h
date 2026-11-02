/* imapio.h — see imapio.c. */
#ifndef LORD_MAIL_IMAPIO_H
#define LORD_MAIL_IMAPIO_H

#include <stddef.h>
#include "net.h"

/* Size of the literal announced at the end of a line ("{n}" / "{n+}"), -1 if none, -2 if absurd. */
long long imap_literal_size(const char *buf, size_t n);
/* One response with its literals inline (malloc, NUL-terminated; *len without the NUL). */
char *imap_read_response(Conn *c, size_t *len, size_t line_max, size_t literal_max, char *err, size_t ecap);

#endif
