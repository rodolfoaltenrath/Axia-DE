/* ipnet.c — addresses, networks and access rules of the remote desktop (see ipnet.h). */
#include "ipnet.h"

#include <arpa/inet.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const unsigned char mapped_prefix[12] = { 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff };

int rs_addr_parse(const char *s, RsAddr *a) {
    memset(a, 0, sizeof *a);
    size_t n = s ? strlen(s) : 0;
    if (!n || n > 45) return -1;
    for (size_t i = 0; i < n; i++) {
        char ch = s[i];
        if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f') || (ch >= 'A' && ch <= 'F') || ch == '.' || ch == ':')) return -1;
    }
    unsigned char v4[4];
    if (!strchr(s, ':')) {
        /* dotted quad without leading zeros (some libcs read 010 as octal or accept it) */
        for (size_t i = 0; i < n; i++)
            if (s[i] == '0' && (i == 0 || s[i - 1] == '.') && i + 1 < n && s[i + 1] != '.') return -1;
        if (inet_pton(AF_INET, s, v4) != 1) return -1;
        memcpy(a->a, mapped_prefix, 12);
        memcpy(a->a + 12, v4, 4);
        a->v4 = 1;
        return 0;
    }
    if (inet_pton(AF_INET6, s, a->a) != 1) return -1;
    a->v4 = !memcmp(a->a, mapped_prefix, 12);
    return 0;
}

static void mask(RsAddr *a, int prefix) {
    for (int i = 0; i < 16; i++) {
        int bits = prefix - i * 8;
        if (bits >= 8) continue;
        a->a[i] &= bits <= 0 ? 0 : (unsigned char)(0xff << (8 - bits));
    }
}

static int cidr_parse(const char *s, RsCidr *c, int min4, int min6) {
    memset(c, 0, sizeof *c);
    if (!s) return -1;
    char buf[64];
    size_t n = strlen(s);
    if (!n || n >= sizeof buf) return -1;
    memcpy(buf, s, n + 1);
    char *slash = strchr(buf, '/');
    int len = -1;
    if (slash) {
        *slash = 0;
        const char *d = slash + 1;
        size_t dl = strlen(d);
        if (!dl || dl > 3 || (dl > 1 && d[0] == '0')) return -1;
        len = 0;
        for (size_t i = 0; i < dl; i++) {
            if (d[i] < '0' || d[i] > '9') return -1;
            len = len * 10 + (d[i] - '0');
        }
    }
    if (rs_addr_parse(buf, &c->net)) return -1;
    if (c->net.v4) {
        if (len < 0) len = 32;
        if (len < min4 || len > 32) return -1;
        c->prefix = len + 96;
    } else {
        if (len < 0) len = 128;
        if (len < min6 || len > 128) return -1;
        c->prefix = len;
    }
    mask(&c->net, c->prefix);
    return 0;
}

int rs_cidr_parse(const char *s, RsCidr *c) { return cidr_parse(s, c, RS_MIN_PREFIX4, RS_MIN_PREFIX6); }

int rs_cidr_contains(const RsCidr *c, const RsAddr *a) {
    RsAddr m = *a;
    mask(&m, c->prefix);
    return !memcmp(m.a, c->net.a, 16);
}

int rs_cidr_covers(const RsCidr *outer, const RsCidr *inner) {
    return outer->prefix <= inner->prefix && rs_cidr_contains(outer, &inner->net);
}

int rs_cidr_equal(const RsCidr *x, const RsCidr *y) { return x->prefix == y->prefix && !memcmp(x->net.a, y->net.a, 16); }

void rs_addr_format(const RsAddr *a, char *out, size_t n) {
    char t[64] = "";
    if (a->v4) inet_ntop(AF_INET, a->a + 12, t, sizeof t);
    else inet_ntop(AF_INET6, a->a, t, sizeof t);
    snprintf(out, n, "%s", t);
}

void rs_cidr_format(const RsCidr *c, char *out, size_t n) {
    char t[64];
    rs_addr_format(&c->net, t, sizeof t);
    if (c->prefix == 128) snprintf(out, n, "%s", t);
    else snprintf(out, n, "%s/%d", t, c->net.v4 ? c->prefix - 96 : c->prefix);
}

void rs_addr_key(const RsAddr *a, char *out, size_t n) {
    if (a->v4) { rs_addr_format(a, out, n); return; }
    RsCidr c = { *a, 64 };
    mask(&c.net, 64);
    rs_cidr_format(&c, out, n);
}

int rs_key_parse(const char *key, RsCidr *c) {
    if (rs_cidr_parse(key, c)) return -1;
    if (c->net.v4) return c->prefix == 128 ? 0 : -1;
    return c->prefix == 64 ? 0 : -1;
}

int rs_addr_is_local(const RsAddr *a) {
    static const char *const nets[] = {
        "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.0/8", "169.254.0.0/16", "100.64.0.0/10",
        "::1", "fe80::/10", "fc00::/7", NULL,
    };
    for (int i = 0; nets[i]; i++) {
        RsCidr c;
        if (!cidr_parse(nets[i], &c, 0, 0) && rs_cidr_contains(&c, a)) return 1;
    }
    return 0;
}

void rs_access_parse(const char *text, RsAccess *acc, int *bad) {
    memset(acc, 0, sizeof *acc);
    if (bad) *bad = 0;
    const char *p = text ? text : "";
    while (*p) {
        const char *e = strchr(p, '\n');
        size_t l = e ? (size_t)(e - p) : strlen(p);
        char line[96];
        if (l < sizeof line) {
            memcpy(line, p, l);
            line[l] = 0;
            char kind[8], net[64], extra;
            RsCidr c;
            if (line[0] == '#' || !line[0]) { /* comment */ }
            else if (sscanf(line, "%7s %63s %c", kind, net, &extra) == 2 && !rs_cidr_parse(net, &c) && !strcmp(kind, "allow"))
                { if (rs_access_add(acc->allow, &acc->nallow, &c) < 0 && bad) (*bad)++; }
            else if (sscanf(line, "%7s %63s %c", kind, net, &extra) == 2 && !rs_cidr_parse(net, &c) && !strcmp(kind, "block"))
                { if (rs_access_add(acc->block, &acc->nblock, &c) < 0 && bad) (*bad)++; }
            else if (bad) (*bad)++;
        } else if (bad) (*bad)++;
        p += l;
        if (*p == '\n') p++;
    }
}

int rs_access_format(const RsAccess *acc, char *out, size_t n) {
    size_t o = 0;
    int k = snprintf(out, n, "# LordOS remote desktop: manual address lists (Settings -> Sharing). See docs/REMOTE.md\n");
    if (k < 0 || (size_t)k >= n) return -1;
    o = (size_t)k;
    for (int pass = 0; pass < 2; pass++) {
        const RsCidr *l = pass ? acc->block : acc->allow;
        int cnt = pass ? acc->nblock : acc->nallow;
        for (int i = 0; i < cnt; i++) {
            char t[64];
            rs_cidr_format(&l[i], t, sizeof t);
            k = snprintf(out + o, n - o, "%s %s\n", pass ? "block" : "allow", t);
            if (k < 0 || (size_t)k >= n - o) return -1;
            o += (size_t)k;
        }
    }
    return (int)o;
}

int rs_access_add(RsCidr *list, int *n, const RsCidr *c) {
    for (int i = 0; i < *n; i++) if (rs_cidr_equal(&list[i], c)) return 1;
    if (*n >= RS_ACCESS_MAX) return -1;
    list[(*n)++] = *c;
    return 0;
}

int rs_access_del(RsCidr *list, int *n, const RsCidr *c) {
    int removed = 0;
    for (int i = 0; i < *n;) {
        if (rs_cidr_covers(c, &list[i])) { list[i] = list[--(*n)]; removed++; }
        else i++;
    }
    return removed;
}

int rs_net_mode(const char *s) {
    if (!s) return -1;
    if (!strcmp(s, "lan")) return RS_NET_LAN;
    if (!strcmp(s, "list")) return RS_NET_LIST;
    if (!strcmp(s, "any")) return RS_NET_ANY;
    return -1;
}

const char *rs_net_name(int mode) { return mode == RS_NET_LIST ? "list" : mode == RS_NET_ANY ? "any" : "lan"; }

int rs_access_decide(const RsAccess *acc, int mode, const RsAddr *peer, int via_vpn) {
    for (int i = 0; i < acc->nblock; i++) if (rs_cidr_contains(&acc->block[i], peer)) return RS_ACC_BLOCKED;
    if (mode == RS_NET_ANY) return RS_ACC_OK;
    if (mode == RS_NET_LIST) {
        for (int i = 0; i < acc->nallow; i++) if (rs_cidr_contains(&acc->allow[i], peer)) return RS_ACC_OK;
        return RS_ACC_NOT_ALLOWED;
    }
    return rs_addr_is_local(peer) || via_vpn ? RS_ACC_OK : RS_ACC_NOT_ALLOWED;
}

int rs_vpn_ifname(const char *name) {
    /* exactly the names LordOS's VPN manager gives (vpn_ifname in server/src/net/wg.c): "<type>-<name>".
     * Never a bare ppp0 (often a PPPoE internet uplink), tun0 or wg0, which anything can create. */
    static const char *const pre[] = { "wg-", "ovpn-", "ips-", "ppp-", "oc-", "fvpn-", NULL };
    if (!name) return 0;
    for (int i = 0; pre[i]; i++) {
        size_t l = strlen(pre[i]);
        if (!strncmp(name, pre[i], l) && name[l]) return 1;
    }
    return 0;
}

int rs_vpn_accepts(const char *ifname, const RsAddr *ifaddr, int prefix, const RsAddr *ptp, const RsAddr *local, const RsAddr *peer) {
    if (!rs_vpn_ifname(ifname) || !ifaddr || !local || !peer) return 0;
    if (memcmp(ifaddr->a, local->a, 16) || ifaddr->v4 != peer->v4) return 0;      /* arrived on that interface's address */
    if (ptp && !memcmp(ptp->a, peer->a, 16)) return 1;                           /* its point-to-point peer */
    /* inside its own subnet, and never a subnet so wide it means "anywhere" */
    if (peer->v4 ? prefix < 96 + RS_MIN_PREFIX4 || prefix > 128 : prefix < RS_MIN_PREFIX6 || prefix > 128) return 0;
    RsCidr c = { *ifaddr, prefix };
    mask(&c.net, prefix);
    return rs_cidr_contains(&c, peer);
}
