/* print.h — printers and scanners for lord.print (ui/js_print.c): each request runs in its own
 * thread (IPP to CUPS, mDNS, eSCL) and its result reaches JS with print_poll(). The compositor never
 * waits. See docs/PRINTING.md. */
#ifndef LORD_PRINT_PRINT_H
#define LORD_PRINT_PRINT_H

#include <stddef.h>

/* kind: printers | jobs | cancel | submit | add | remove | default | discover | scancaps | scan.
 * json: arguments (already validated by js_print.c; validated again here). docfd (submit): the
 * PDF to print; print_task keeps it. Returns the id (> 0) or -1. */
int   print_task(const char *kind, const char *json, size_t len, int docfd, size_t doclen);
char *print_poll(void);             /* [{id, kind, ok, data}] (malloc) or NULL */
int   print_pending(void);

#endif
