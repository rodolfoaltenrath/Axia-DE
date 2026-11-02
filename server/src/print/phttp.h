/* phttp.h — minimal HTTP/1.1 client for printing: IPP to CUPS over its local socket and eSCL to
 * the network's scanners. One connection per request (Connection: close), deadlines with poll and size
 * caps; replies from the network are treated as untrusted. */
#ifndef LORD_PRINT_PHTTP_H
#define LORD_PRINT_PHTTP_H

#include <stddef.h>

typedef struct {
    int status;
    char *body;
    size_t len;
    char location[512];
    char www_auth[256];
    char content_type[128];
    int retry_after;
} PResp;

int  phttp_unix(const char *path);                         /* connected fd or -1 */
int  phttp_tcp(const char *ipv4, int port, int timeout_ms); /* connected fd or -1 */
/* Sends the request (body and, if docfd >= 0, doclen bytes from that fd afterwards) and reads the
 * complete reply (up to max bytes). 0 if an HTTP reply arrived; -1 if the connection failed. */
int  phttp_request(int fd, const char *method, const char *host, const char *path, const char *headers,
                   const char *ctype, const void *body, size_t blen, int docfd, size_t doclen,
                   size_t max, int timeout_ms, PResp *r);
void presp_free(PResp *r);

/* Is it a local network IPv4 address (private, link-local or this machine)? */
int  phttp_lan_ipv4(const char *ip);

#endif
