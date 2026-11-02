/* cups.h — our own IPP client for CUPS over its local socket (/run/cups/cups.sock), without libcups.
 *
 * CUPS manages the queue and the drivers; LordOS only speaks IPP to it. Administration
 * operations (add, remove, choose the default) are authorized by CUPS's own policy:
 * the user has to be in the lpadmin group (LordOS administrators are) and
 * identifies with «PeerCred» (CUPS looks at the socket credentials). There is no root action in
 * admin.c for this. See docs/PRINTING.md.
 *
 * Each function builds its result as JSON in *out (JBuf from doc/wire.h) and returns 0, or -1 with
 * {"error": "..."} in *out. They block: they're called from print.c's threads. */
#ifndef LORD_PRINT_CUPS_H
#define LORD_PRINT_CUPS_H

#include <stddef.h>
#include "../doc/wire.h"
#include "../sec/json.h"

extern const char *cups_socket;            /* "/run/cups/cups.sock" (the tests change it) */

int cups_printers(JBuf *out);
int cups_jobs(JBuf *out);
int cups_cancel(int job, JBuf *out);
/* opts: {name, copies, pages:"1-3,5", sides, color, media} */
int cups_submit(const char *printer, int docfd, size_t len, const JVal *opts, JBuf *out);
int cups_add(const char *name, const char *uri, const char *info, const char *location, JBuf *out);
int cups_remove(const char *name, JBuf *out);
int cups_set_default(const char *name, JBuf *out);

/* validations (js_print.c uses them too) */
int cups_valid_name(const char *s);         /* letters, digits, _ and -, up to 127 */
int cups_valid_uri(const char *s);          /* ipp:// ipps:// http:// https:// dnssd:// without spaces */
int cups_valid_text(const char *s, size_t max);
/* "1-3,5,8-9" → ranges; returns how many (max. 32) or -1 */
int cups_page_ranges(const char *s, int lo[], int hi[], int max);

#endif
