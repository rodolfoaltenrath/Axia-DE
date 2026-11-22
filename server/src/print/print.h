/* print.h — printers and scanners for lord.print (ui/js_print.c). Everything behind this header is
 * Rust (rust/axia-print): our own IPP client for CUPS over its local socket, the eSCL scanner client
 * and the mDNS discovery. Each request runs in its own thread and its result reaches JS with
 * print_poll(); the compositor never waits. See docs/PRINTING.md. */
#ifndef LORD_PRINT_PRINT_H
#define LORD_PRINT_PRINT_H

#include <stddef.h>
#include <sys/socket.h>
#include "../sec/netconn.h"

/* kind: printers | jobs | cancel | submit | add | remove | default | discover | scancaps | scan.
 * json: arguments (already validated by js_print.c; validated again there). docfd (submit): the
 * PDF to print; the task closes it, also when it refuses to start. Returns the id (> 0) or -1. */
int   print_task(const char *kind, const char *json, size_t len, int docfd, size_t doclen);
char *print_poll(void);             /* [{id, kind, ok, data}] (malloc) or NULL */
int   print_pending(void);
int   print_has_cups(void);         /* is CUPS's socket (/run/cups/cups.sock) there? */

/* The printers' sockets are made by their own connector (sec/netconn.h): the compositor hands over
 * netconn_socket once; with none set (the tests) the sockets are made directly. */
typedef int (*PrintConnector)(NcComponent comp, int fam, int type, const struct sockaddr *to, socklen_t tolen);
void  print_set_connector(PrintConnector f);

/* validations (js_print.c checks its arguments with them too) */
int   cups_valid_name(const char *s);       /* letters, digits, _ and -, up to 127 */
int   cups_valid_uri(const char *s);        /* ipp:// ipps:// http:// https:// dnssd:// without spaces */
int   cups_valid_text(const char *s, size_t max);
int   phttp_lan_ipv4(const char *ip);       /* private, link-local or this machine */
int   escl_valid_rs(const char *rs);        /* the eSCL resource path from the TXT «rs» */

#endif
