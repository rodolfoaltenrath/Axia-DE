/* escl.h — our own eSCL client («driverless» scanners, AirScan) over HTTP on the local network.
 * No SANE. Only LAN IPv4 addresses and HTTP (no TLS: _uscans is not supported). */
#ifndef LORD_PRINT_ESCL_H
#define LORD_PRINT_ESCL_H

#include "../doc/wire.h"
#include "../sec/json.h"

/* rs: the path from the TXT «rs» (normally "eSCL"). */
int escl_caps(const char *ip, int port, const char *rs, JBuf *out);
/* s: {source, color, res, format, x, y, w, h (in 1/300 inch)}; the result goes to a new
 * file in dir (folder fd), with a base name; out = {file, format, bytes}. */
int escl_scan(const char *ip, int port, const char *rs, const JVal *s, const char *dir, JBuf *out);
int escl_valid_rs(const char *rs);

#endif
