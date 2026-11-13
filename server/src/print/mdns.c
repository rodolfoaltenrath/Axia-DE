/* mdns.c — see mdns.h. Only the socket loop: reading replies, building queries and turning
 * the cache into services is Rust (rust/axia-codecs/src/mdns.rs), so this file links with the
 * codecs library. */
#include "mdns.h"
#include "../sec/netconn.h"

#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

enum { T_A = MDNS_T_A, T_PTR = MDNS_T_PTR, T_TXT = MDNS_T_TXT, T_SRV = MDNS_T_SRV };

static double now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

static const char *find_a(const MdnsCache *c, const char *host) {
    for (int i = 0; i < c->na; i++) if (!strcasecmp(c->a[i].host, host)) return c->a[i].addr;
    return NULL;
}

int mdns_browse(const char *const *types, int ntypes, int ms, MdnsSvc *out, int max) {
    int fd = netconn_socket(NC_PRINT, AF_INET, SOCK_DGRAM, NULL, 0);   /* the printers' own (sec/netconn.h) */
    if (fd < 0) return 0;
    unsigned char ttl = 255, loop = 1;
    setsockopt(fd, IPPROTO_IP, IP_MULTICAST_TTL, &ttl, sizeof ttl);
    setsockopt(fd, IPPROTO_IP, IP_MULTICAST_LOOP, &loop, sizeof loop);
    struct sockaddr_in to = { .sin_family = AF_INET, .sin_port = htons(5353) };
    inet_pton(AF_INET, "224.0.0.251", &to.sin_addr);
    MdnsCache *c = calloc(1, sizeof *c);
    if (!c) { close(fd); return 0; }
    const char *names[8];
    int qt[8], nq = 0;
    char full[8][64];
    for (int i = 0; i < ntypes && i < 8; i++) {
        snprintf(full[i], sizeof full[i], "%s.local", types[i]);
        names[nq] = full[i];
        qt[nq++] = T_PTR;
    }
    uint8_t pkt[1500];
    uint16_t id = (uint16_t)(getpid() ^ (unsigned)time(NULL));
    double start = now_ms(), next_send = start;
    int sends = 0, followed = 0;
    while (now_ms() - start < ms) {
        if (now_ms() >= next_send && sends < 3) {
            size_t qn = mdns_query(pkt, sizeof pkt, names, qt, nq, id);
            if (qn) sendto(fd, pkt, qn, 0, (struct sockaddr *)&to, sizeof to);
            sends++;
            next_send = now_ms() + (sends == 1 ? 350 : 700);
        }
        /* halfway: the ones that still have no SRV or address are asked one by one */
        if (!followed && now_ms() - start > ms / 2) {
            followed = 1;
            const char *fn[16];
            int ft[16], nf = 0;
            static char hosts[16][256];
            for (int i = 0; i < c->nptr && nf < 14; i++) {
                int has = 0;
                for (int k = 0; k < c->nsrv; k++) if (!strcasecmp(c->srv[k].name, c->ptr[i].target)) {
                    has = 1;
                    if (!find_a(c, c->srv[k].host) && nf < 14) { snprintf(hosts[nf], sizeof hosts[0], "%s", c->srv[k].host); fn[nf] = hosts[nf]; ft[nf++] = T_A; }
                }
                int txt = 0;
                for (int k = 0; k < c->ntxt; k++) if (!strcasecmp(c->txt[k].name, c->ptr[i].target)) txt = 1;
                if (!has && nf < 14) { fn[nf] = c->ptr[i].target; ft[nf++] = T_SRV; }
                if (!txt && nf < 14) { fn[nf] = c->ptr[i].target; ft[nf++] = T_TXT; }
            }
            if (nf) { size_t qn = mdns_query(pkt, sizeof pkt, fn, ft, nf, id); if (qn) sendto(fd, pkt, qn, 0, (struct sockaddr *)&to, sizeof to); }
        }
        int left = (int)(ms - (now_ms() - start));
        int w = (int)(next_send - now_ms());
        if (sends >= 3 || w > left) w = left;
        if (!followed && w > (int)(start + ms / 2 - now_ms())) w = (int)(start + ms / 2 - now_ms()) + 1;
        if (w < 1) w = 1;
        struct pollfd pf = { fd, POLLIN, 0 };
        if (poll(&pf, 1, w) <= 0) continue;
        for (;;) {
            ssize_t r = recv(fd, pkt, sizeof pkt, 0);
            if (r <= 0) break;
            mdns_parse(pkt, (size_t)r, c);
        }
    }
    close(fd);
    int n = mdns_collect(c, types, ntypes, out, max);
    free(c);
    return n;
}
