/* vpnproc.c — VPNs that are a Debian program driven by LordOS (root side):
 *
 *   PPTP         pppd + pptp (pptp-linux). Included, but obsolete and insecure (MS-CHAPv2/MPPE).
 *   OpenConnect  openconnect (optional: net-vpn-enable openconnect). Cisco AnyConnect,
 *                GlobalProtect, Pulse/Ivanti and Fortinet over SSL.
 *   Fortinet     openfortivpn (optional: net-vpn-enable fortinet).
 *
 * There are never free-form options: the program's config is written by LordOS (0600) with only
 * the validated values from the import; passwords go via a 0600 file or stdin, never
 * on the command line. pppd doesn't run the ip-up/ip-down scripts (they point at a file that doesn't
 * exist) and openconnect uses a fixed-content script of ours that only records the configuration
 * the server sends so this process can apply it.
 *
 * Import formats (key=value, see vpn.c):
 *   pptp:         server, user, password, mppe=required|optional|off (required), routes (optional)
 *   openconnect:  server, protocol=anyconnect|gp|pulse|fortinet, user, password, usergroup?, servercert?
 *   fortinet:     host, port (443), user, password, trusted-cert (sha256 hex, several comma-separated) */
#include "vpn.h"
#include "../admin.h"

#include <net/if.h>                 /* before the linux/ ones */
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/netlink.h>
#include <linux/rtnetlink.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define PPPD "/usr/sbin/pppd"
#define PPTP "/usr/sbin/pptp"
#define OPENCONNECT "/usr/sbin/openconnect"
#define OPENFORTIVPN "/usr/bin/openfortivpn"
#define PPP_DIR NET_DIR "/ppp"
#define OC_SCRIPT NET_DIR "/oc-script"
#define OC_RUN NET_RUN_DIR "/oc"
#define FORTI_DIR NET_DIR "/fortinet"

static void meta(const char *name, const char *type, const char *server, const char *user, int full, int insecure) {
    char ifn[IFNAMSIZ];
    vpn_ifname(type, name, ifn, sizeof ifn);
    Buf m = {0};
    buf_printf(&m, "{\"name\":");
    buf_json_str(&m, name);
    buf_printf(&m, ",\"type\":\"%s\",\"ifname\":\"%s\",\"server\":", type, ifn);
    buf_json_str(&m, server);
    buf_printf(&m, ",\"user\":");
    buf_json_str(&m, user ? user : "");
    buf_printf(&m, ",\"fullTunnel\":%s,\"insecure\":%s}\n", full ? "true" : "false", insecure ? "true" : "false");
    if (m.p) vpn_meta_save(name, m.p);
    buf_free(&m);
}

/* Reads lines from fd; calls fn for each one. 0 if still open, -1 if it ended. */
typedef struct { char buf[8192]; size_t len; } LineBuf;
static int read_lines(int fd, LineBuf *lb, void (*fn)(const char *line, void *ud), void *ud) {
    ssize_t r = read(fd, lb->buf + lb->len, sizeof lb->buf - 1 - lb->len);
    if (r <= 0) return r < 0 && errno == EINTR ? 0 : -1;
    lb->len += (size_t)r;
    char *nl;
    while ((nl = memchr(lb->buf, '\n', lb->len))) {
        *nl = 0;
        if (nl > lb->buf && nl[-1] == '\r') nl[-1] = 0;
        fn(lb->buf, ud);
        size_t used = (size_t)(nl - lb->buf) + 1;
        memmove(lb->buf, nl + 1, lb->len - used);
        lb->len -= used;
    }
    if (lb->len >= sizeof lb->buf - 1) lb->len = 0;
    return 0;
}

/* State common to the three. */
typedef struct {
    const char *name, *type;
    char ifn[IFNAMSIZ], ip[64], dns[400], domains[400];
    int sfam;
    uint8_t sip[16];
    int connected, auth_failed, full;
    const char *routes;
    double ip_at;                       /* when the IP arrived (PPTP waits for the DNS) */
    char last[200];                     /* the program's last error: reported if it didn't connect */
} Run;

static double mono(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }

static void up(Run *r) {
    if (r->connected) return;
    if (r->routes && *r->routes) vpn_routes_list(r->ifn, r->routes);
    else if (r->full) vpn_routes_full(r->ifn, r->sfam, r->sip);
    if (r->dns[0]) dns_set(r->ifn, r->dns, r->domains, r->full ? DNS_FULL : DNS_SPLIT);
    r->connected = 1;
    admin_event("net vpn %s connected %s", r->name, r->ip);
}

static void down(Run *r) {
    if (!r->connected && !r->auth_failed && r->last[0]) admin_event("net vpn %s error %s", r->name, r->last);
    if (r->connected) dns_revert(r->ifn);
    vpn_routes_clear(r->sfam, r->sip);
    vpn_mark(r->ifn, r->name, 0);
    admin_event("net vpn %s disconnected", r->name);
}

static void add_dns(Run *r, const char *ip) {
    int fam;
    uint8_t a[16];
    if (parse_ip(ip, &fam, a) || strlen(r->dns) + strlen(ip) + 2 >= sizeof r->dns || strstr(r->dns, ip)) return;
    snprintf(r->dns + strlen(r->dns), sizeof r->dns - strlen(r->dns), "%s%s", r->dns[0] ? " " : "", ip);
}

/* Waits for the process to end or SIGTERM to arrive, reading its output. tick() every ~200 ms. */
static void supervise(Run *r, pid_t pid, int out, void (*line)(const char *, void *), void (*tick)(Run *)) {
    LineBuf lb = { .len = 0 };
    while (!vpn_stop) {
        struct pollfd pf = { out, POLLIN, 0 };
        int pr = poll(&pf, 1, 200);
        if (pr > 0 && read_lines(out, &lb, line, r)) break;
        if (tick) tick(r);
    }
    vpn_reap(pid);
}

/* ======================= PPTP ======================= */

static const char *const pptp_keys[] = { "server", "user", "password", "mppe", "routes", NULL };

int pptp_import(const char *name, const char *text, char *err, size_t errn) {
    VpnKv kv;
    if (vpn_kv_parse(text, pptp_keys, NULL, &kv, err, errn)) return -1;
    const char *server = vpn_kv(&kv, "server"), *user = vpn_kv(&kv, "user"), *pw = vpn_kv(&kv, "password"), *mppe = vpn_kv(&kv, "mppe");
    int rc = 0;
    if (!server || !vpn_host_ok(server) || strchr(server, ':')) { lerr_set(err, errn, "vpn.missingKey", "key", "server", NULL); rc = -1; }
    else if (!user || !*user || !pw) { lerr_set(err, errn, "vpn.missingCredentials", NULL); rc = -1; }
    else if (mppe && strcmp(mppe, "required") && strcmp(mppe, "optional") && strcmp(mppe, "off")) { lerr_set(err, errn, "vpn.invalidValue", "key", "mppe", NULL); rc = -1; }
    else if (vpn_kv(&kv, "routes") && !vpn_routes_valid(vpn_kv(&kv, "routes"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "routes", NULL); rc = -1; }
    if (!rc && vpn_kv_save(name, "pptp", &kv)) { lerr_set(err, errn, "net.saveFailed", NULL); rc = -1; }
    if (!rc) meta(name, "pptp", server, user, !vpn_kv(&kv, "routes"), 1);
    vpn_kv_free(&kv);
    return rc;
}

/* pppd option in quotes: \ and " with a backslash. */
static void pquote(const char *in, char *out, size_t n) {
    size_t o = 0;
    out[o++] = '"';
    for (const char *p = in; *p && o + 4 < n; p++) { if (*p == '\\' || *p == '"') out[o++] = '\\'; out[o++] = *p; }
    out[o++] = '"';
    out[o] = 0;
}

static void pptp_line(const char *l, void *ud) {
    Run *r = ud;
    char ip[64];
    if (sscanf(l, "local IP address %63s", ip) == 1 || sscanf(l, "local  IP address %63s", ip) == 1) { snprintf(r->ip, sizeof r->ip, "%s", ip); r->ip_at = mono(); }
    else if (sscanf(l, "primary DNS address %63s", ip) == 1 || sscanf(l, "primary   DNS address %63s", ip) == 1) add_dns(r, ip);
    else if (sscanf(l, "secondary DNS address %63s", ip) == 1) add_dns(r, ip);
    else if (strstr(l, "authentication failed")) { if (!r->auth_failed) admin_event("net vpn %s auth-failed", r->name); r->auth_failed = 1; }
    else if (strstr(l, "MPPE required but") || strstr(l, "Couldn't") || strstr(l, "Connection refused") || strstr(l, "anon fatal") || strstr(l, "LCP: timeout"))
        snprintf(r->last, sizeof r->last, "%.190s", l);
}

static void pptp_tick(Run *r) {
    /* "remote IP address" arrives right after "local": with the interface up, connected */
    if (!r->connected && r->ip[0] && rt_ifindex(r->ifn) && mono() - r->ip_at > 0.5) up(r);   /* the DNS servers arrive right away */
}

void pptp_run(const char *name) {
    vpn_signals();
    VpnKv kv;
    if (vpn_kv_load(name, "pptp", &kv)) { admin_event("net vpn %s error vpn.noConfig", name); admin_event("net vpn %s disconnected", name); return; }
    Run r = { .name = name, .type = "pptp", .routes = vpn_kv(&kv, "routes") };
    r.full = !r.routes;
    vpn_ifname("pptp", name, r.ifn, sizeof r.ifn);
    admin_event("net vpn %s connecting", name);
    if (vpn_resolve(vpn_kv(&kv, "server"), &r.sfam, r.sip) || r.sfam != AF_INET) { admin_event("net vpn %s error vpn.serverUnresolved", name); admin_event("net vpn %s disconnected", name); vpn_kv_free(&kv); return; }
    char sip[64], qu[300], qp[600], path[200];
    fmt_ip(AF_INET, r.sip, sip, sizeof sip);
    pquote(vpn_kv(&kv, "user"), qu, sizeof qu);
    pquote(vpn_kv(&kv, "password"), qp, sizeof qp);
    const char *mppe = vpn_kv(&kv, "mppe") ? vpn_kv(&kv, "mppe") : "required";
    Buf o = {0};
    /* pty: the only command, built here with the already resolved IP (no user text) */
    buf_printf(&o, "# Generated by LordOS (do not edit)\npty \"" PPTP " %s --nolaunchpppd --loglevel 0\"\n", sip);
    buf_printf(&o, "name %s\npassword %s\nremotename lordos-%s\nifname %s\n", qu, qp, name, r.ifn);
    buf_printf(&o, "noauth\nrefuse-pap\nrefuse-chap\nrefuse-mschap\nrefuse-eap\n%s\n",
               !strcmp(mppe, "required") ? "require-mppe-128" : !strcmp(mppe, "off") ? "nomppe" : "");
    buf_printf(&o, "nodefaultroute\nnoipdefault\nusepeerdns\nnobsdcomp\nnodeflate\nmtu 1400\nmru 1400\n"
               "lcp-echo-interval 20\nlcp-echo-failure 4\nmaxfail 1\nnodetach\nlogfd 1\n"
               "ip-up-script " PPP_DIR "/no-script\nip-down-script " PPP_DIR "/no-script\n"
               "ipv6-up-script " PPP_DIR "/no-script\nipv6-down-script " PPP_DIR "/no-script\n");
    memset(qp, 0, sizeof qp);
    mkdir(NET_DIR, 0755);
    mkdir(PPP_DIR, 0700);
    snprintf(path, sizeof path, PPP_DIR "/%s.options", name);
    int ok = !o.oom && write_file_atomic(path, o.p, o.len, 0600) == 0;
    if (o.p) memset(o.p, 0, o.len);
    buf_free(&o);
    vpn_kv_free(&kv);
    if (!ok) { admin_event("net vpn %s error vpn.writeFailed", name); admin_event("net vpn %s disconnected", name); return; }
    vpn_mark(r.ifn, name, 1);
    const char *argv[] = { PPPD, "file", path, NULL };
    int out;
    pid_t pid = vpn_spawn(argv, &out, NULL, NULL);
    if (pid < 0) { admin_event("net vpn %s error vpn.startFailed", name); admin_event("net vpn %s disconnected", name); vpn_mark(r.ifn, name, 0); return; }
    r.routes = NULL;                      /* kv already freed: reread */
    VpnKv k2;
    char routes[256] = "";
    if (!vpn_kv_load(name, "pptp", &k2)) { if (vpn_kv(&k2, "routes")) snprintf(routes, sizeof routes, "%s", vpn_kv(&k2, "routes")); vpn_kv_free(&k2); }
    if (routes[0]) r.routes = routes;
    supervise(&r, pid, out, pptp_line, pptp_tick);
    close(out);
    down(&r);
}

/* ======================= OpenConnect ======================= */

static const char *const oc_keys[] = { "server", "protocol", "user", "password", "usergroup", "servercert", NULL };

static int oc_server_ok(const char *s) {
    size_t n = strlen(s);
    if (!n || n > 200 || s[0] == '-') return 0;
    for (const char *p = s; *p; p++) if (!isalnum((unsigned char)*p) && !strchr(".-:/_[]", *p)) return 0;
    return 1;
}

static int pin_ok(const char *s) {
    if (!strncmp(s, "pin-sha256:", 11)) {
        for (const char *p = s + 11; *p; p++) if (!isalnum((unsigned char)*p) && !strchr("+/=", *p)) return 0;
        return strlen(s) > 20;
    }
    size_t n = strlen(s);
    for (const char *p = s; *p; p++) if (!isxdigit((unsigned char)*p) && *p != ':') return 0;
    return n >= 40;
}

int oc_import(const char *name, const char *text, char *err, size_t errn) {
    VpnKv kv;
    if (vpn_kv_parse(text, oc_keys, NULL, &kv, err, errn)) return -1;
    const char *server = vpn_kv(&kv, "server"), *proto = vpn_kv(&kv, "protocol"), *user = vpn_kv(&kv, "user");
    int rc = 0;
    if (!server || !oc_server_ok(server)) { lerr_set(err, errn, "vpn.missingKey", "key", "server", NULL); rc = -1; }
    else if (proto && strcmp(proto, "anyconnect") && strcmp(proto, "gp") && strcmp(proto, "pulse") && strcmp(proto, "fortinet")) { lerr_set(err, errn, "vpn.invalidValue", "key", "protocol", NULL); rc = -1; }
    else if (!user || !*user || !vpn_kv(&kv, "password")) { lerr_set(err, errn, "vpn.missingCredentials", NULL); rc = -1; }
    else if (vpn_kv(&kv, "servercert") && !pin_ok(vpn_kv(&kv, "servercert"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "servercert", NULL); rc = -1; }
    if (!rc && vpn_kv_save(name, "openconnect", &kv)) { lerr_set(err, errn, "net.saveFailed", NULL); rc = -1; }
    if (!rc) meta(name, "openconnect", server, user, 1, 0);
    vpn_kv_free(&kv);
    return rc;
}

/* The script openconnect calls (instead of vpnc-script): fixed content, it only records the
 * configuration the server sent so LordOS can apply it. */
static const char oc_script[] =
    "#!/bin/sh\n"
    "# LordOS: openconnect calls this script; it only records the configuration for LordOS.\n"
    "umask 077\n"
    "case \"$TUNDEV\" in oc-*) ;; *) exit 0;; esac\n"
    "case \"$reason\" in connect|disconnect|reconnect) ;; *) exit 0;; esac\n"
    "f=\"" OC_RUN "/$TUNDEV.$reason\"\n"
    "env | grep -E '^(TUNDEV|VPNGATEWAY|INTERNAL_IP4_(ADDRESS|NETMASK|NETMASKLEN|MTU|DNS)|CISCO_DEF_DOMAIN|CISCO_SPLIT_INC(_[0-9]+_(ADDR|MASKLEN))?)=' > \"$f.tmp\"\n"
    "mv \"$f.tmp\" \"$f\"\n"
    "exit 0\n";

static void oc_line(const char *l, void *ud) {
    Run *r = ud;
    if (strstr(l, "Login failed") || strstr(l, "authentication failed") || strstr(l, "Failed to obtain WebVPN cookie") || strstr(l, "Invalid credentials")) {
        if (!r->auth_failed) admin_event("net vpn %s auth-failed", r->name);
        r->auth_failed = 1;
    } else if ((strstr(l, "certificate") && strstr(l, "fail")) || strstr(l, "Failed to") || strstr(l, "Error") || strstr(l, "Unknown"))
        snprintf(r->last, sizeof r->last, "%.190s", l);
}

/* Applies the connect file the script left. */
static void oc_tick(Run *r) {
    char path[160];
    snprintf(path, sizeof path, OC_RUN "/%s.connect", r->ifn);
    char *t = read_file(path, 16384, NULL);
    if (!t) return;
    unlink(path);
    char v[256], addr[64] = "", mask[64] = "", masklen[8] = "", mtu[8] = "", gw[64] = "", dns[256] = "", dom[256] = "", nsplit[8] = "";
    kv_get(t, "INTERNAL_IP4_ADDRESS", 0, addr, sizeof addr);
    kv_get(t, "INTERNAL_IP4_NETMASK", 0, mask, sizeof mask);
    kv_get(t, "INTERNAL_IP4_NETMASKLEN", 0, masklen, sizeof masklen);
    kv_get(t, "INTERNAL_IP4_MTU", 0, mtu, sizeof mtu);
    kv_get(t, "VPNGATEWAY", 0, gw, sizeof gw);
    kv_get(t, "INTERNAL_IP4_DNS", 0, dns, sizeof dns);
    kv_get(t, "CISCO_DEF_DOMAIN", 0, dom, sizeof dom);
    kv_get(t, "CISCO_SPLIT_INC", 0, nsplit, sizeof nsplit);
    int fam;
    uint8_t a[16];
    if (parse_ip(addr, &fam, a) || fam != AF_INET) { free(t); admin_event("net vpn %s error vpn.noAddress", r->name); return; }
    int prefix = masklen[0] ? atoi(masklen) : 32;
    if (!masklen[0] && mask[0]) {
        uint8_t m[16];
        int mf;
        if (!parse_ip(mask, &mf, m)) { uint32_t x = (uint32_t)m[0] << 24 | m[1] << 16 | m[2] << 8 | m[3]; prefix = 0; while (x & 0x80000000u) { prefix++; x <<= 1; } }
    }
    if (prefix < 1 || prefix > 32) prefix = 32;
    int fd = nl_open(NETLINK_ROUTE, 0);
    int idx = rt_ifindex(r->ifn);
    if (fd >= 0 && idx) {
        rt_addr(fd, 1, idx, AF_INET, a, prefix, 0, 0);
        if (atoi(mtu) >= 576) rt_link_mtu(fd, idx, (unsigned)atoi(mtu));
        rt_link_set(fd, idx, 1);
    }
    if (fd >= 0) close(fd);
    snprintf(r->ip, sizeof r->ip, "%s", addr);
    if (!parse_ip(gw, &fam, a)) { r->sfam = fam; memcpy(r->sip, a, 16); }
    for (char *tok = strtok(dns, " "); tok; tok = strtok(NULL, " ")) add_dns(r, tok);
    int ok_dom = 1;
    for (char *c = dom; *c; c++) if (!isalnum((unsigned char)*c) && !strchr(".- ", *c)) ok_dom = 0;
    if (ok_dom) snprintf(r->domains, sizeof r->domains, "%s", dom);
    /* routes: the ones the server sends (split) or all traffic */
    static char split[256];
    split[0] = 0;
    int ns = atoi(nsplit);
    for (int i = 0; i < ns && i < 32; i++) {
        char k[48], ad[64], ml[8];
        snprintf(k, sizeof k, "CISCO_SPLIT_INC_%d_ADDR", i);
        kv_get(t, k, 0, ad, sizeof ad);
        snprintf(k, sizeof k, "CISCO_SPLIT_INC_%d_MASKLEN", i);
        kv_get(t, k, 0, ml, sizeof ml);
        if (!parse_ip(ad, &fam, a) && fam == AF_INET && atoi(ml) > 0 && atoi(ml) <= 32 && strlen(split) + 24 < sizeof split)
            snprintf(split + strlen(split), sizeof split - strlen(split), "%s%s/%d", split[0] ? "," : "", ad, atoi(ml));
    }
    (void)v;
    free(t);
    r->full = !split[0];
    r->routes = split[0] ? split : NULL;
    up(r);
}

void oc_run(const char *name) {
    vpn_signals();
    VpnKv kv;
    if (vpn_kv_load(name, "openconnect", &kv)) { admin_event("net vpn %s error vpn.noConfig", name); admin_event("net vpn %s disconnected", name); return; }
    Run r = { .name = name, .type = "openconnect" };
    vpn_ifname("openconnect", name, r.ifn, sizeof r.ifn);
    admin_event("net vpn %s connecting", name);
    mkdir(NET_DIR, 0755);
    mkdir(NET_RUN_DIR, 0755);
    mkdir(OC_RUN, 0700);
    if (write_file_atomic(OC_SCRIPT, oc_script, sizeof oc_script - 1, 0755)) { admin_event("net vpn %s error vpn.writeFailed", name); admin_event("net vpn %s disconnected", name); vpn_kv_free(&kv); return; }
    char proto[40], user[300], group[300] = "", cert[300] = "", iface[40];
    snprintf(proto, sizeof proto, "--protocol=%s", vpn_kv(&kv, "protocol") ? vpn_kv(&kv, "protocol") : "anyconnect");
    snprintf(user, sizeof user, "--user=%s", vpn_kv(&kv, "user"));
    if (vpn_kv(&kv, "usergroup")) snprintf(group, sizeof group, "--authgroup=%s", vpn_kv(&kv, "usergroup"));
    if (vpn_kv(&kv, "servercert")) snprintf(cert, sizeof cert, "--servercert=%s", vpn_kv(&kv, "servercert"));
    snprintf(iface, sizeof iface, "--interface=%s", r.ifn);
    const char *argv[16] = { OPENCONNECT, proto, user, "--passwd-on-stdin", "--non-inter", iface, "--script=" OC_SCRIPT };
    int a = 7;
    if (group[0]) argv[a++] = group;
    if (cert[0]) argv[a++] = cert;
    argv[a++] = "--";
    argv[a++] = vpn_kv(&kv, "server");
    argv[a] = NULL;
    vpn_mark(r.ifn, name, 1);
    int out, in;
    pid_t pid = vpn_spawn(argv, &out, &in, NULL);
    if (pid >= 0) {
        const char *pw = vpn_kv(&kv, "password");
        if (write(in, pw, strlen(pw)) < 0 || write(in, "\n", 1) < 0) {}
        close(in);
    }
    vpn_kv_free(&kv);
    if (pid < 0) { admin_event("net vpn %s error vpn.startFailed", name); admin_event("net vpn %s disconnected", name); vpn_mark(r.ifn, name, 0); return; }
    supervise(&r, pid, out, oc_line, oc_tick);
    close(out);
    char path[160];
    snprintf(path, sizeof path, OC_RUN "/%s.disconnect", r.ifn);
    unlink(path);
    snprintf(path, sizeof path, OC_RUN "/%s.connect", r.ifn);
    unlink(path);
    down(&r);
}

/* ======================= Fortinet (openfortivpn) ======================= */

static const char *const forti_keys[] = { "host", "port", "user", "password", "trusted-cert", "routes", NULL };

int forti_import(const char *name, const char *text, char *err, size_t errn) {
    VpnKv kv;
    if (vpn_kv_parse(text, forti_keys, NULL, &kv, err, errn)) return -1;
    const char *host = vpn_kv(&kv, "host"), *port = vpn_kv(&kv, "port"), *user = vpn_kv(&kv, "user"), *tc = vpn_kv(&kv, "trusted-cert");
    int rc = 0;
    if (!host || !vpn_host_ok(host) || strchr(host, ':')) { lerr_set(err, errn, "vpn.missingKey", "key", "host", NULL); rc = -1; }
    else if (port && (atoi(port) < 1 || atoi(port) > 65535 || strspn(port, "0123456789") != strlen(port))) { lerr_set(err, errn, "vpn.invalidValue", "key", "port", NULL); rc = -1; }
    else if (!user || !*user || !vpn_kv(&kv, "password")) { lerr_set(err, errn, "vpn.missingCredentials", NULL); rc = -1; }
    else if (tc) {
        char copy[256];
        snprintf(copy, sizeof copy, "%s", tc);
        for (char *t = strtok(copy, ", "); t && !rc; t = strtok(NULL, ", "))
            if (strlen(t) != 64 || strspn(t, "0123456789abcdefABCDEF") != 64) { lerr_set(err, errn, "vpn.invalidValue", "key", "trusted-cert", NULL); rc = -1; }
    }
    if (!rc && vpn_kv(&kv, "routes") && !vpn_routes_valid(vpn_kv(&kv, "routes"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "routes", NULL); rc = -1; }
    if (!rc && vpn_kv_save(name, "fortinet", &kv)) { lerr_set(err, errn, "net.saveFailed", NULL); rc = -1; }
    if (!rc) meta(name, "fortinet", host, user, 1, 0);
    vpn_kv_free(&kv);
    return rc;
}

static void forti_line(const char *l, void *ud) {
    Run *r = ud;
    /* INFO:   Got addresses: [10.0.0.5], ns [1.1.1.1, 8.8.8.8] */
    const char *g = strstr(l, "Got addresses: [");
    if (g) {
        g += 16;
        size_t k = strcspn(g, "]");
        if (k < sizeof r->ip) { memcpy(r->ip, g, k); r->ip[k] = 0; }
        const char *ns = strstr(g, "ns [");
        if (ns) {
            char list[256];
            snprintf(list, sizeof list, "%.*s", (int)strcspn(ns + 4, "]"), ns + 4);
            for (char *t = strtok(list, ", "); t; t = strtok(NULL, ", ")) add_dns(r, t);
        }
    } else if (strstr(l, "Tunnel is up and running")) {
        r->full = !(r->routes && *r->routes);
        up(r);
    } else if (strstr(l, "Could not authenticate") || strstr(l, "authentication failed") || strstr(l, "Invalid credentials")) {
        if (!r->auth_failed) admin_event("net vpn %s auth-failed", r->name);
        r->auth_failed = 1;
    } else if (strstr(l, "ERROR:")) snprintf(r->last, sizeof r->last, "%.190s", l);
}

void forti_run(const char *name) {
    vpn_signals();
    VpnKv kv;
    if (vpn_kv_load(name, "fortinet", &kv)) { admin_event("net vpn %s error vpn.noConfig", name); admin_event("net vpn %s disconnected", name); return; }
    Run r = { .name = name, .type = "fortinet" };
    vpn_ifname("fortinet", name, r.ifn, sizeof r.ifn);
    admin_event("net vpn %s connecting", name);
    if (vpn_resolve(vpn_kv(&kv, "host"), &r.sfam, r.sip)) { admin_event("net vpn %s error vpn.serverUnresolved", name); admin_event("net vpn %s disconnected", name); vpn_kv_free(&kv); return; }
    static char routes[256];
    routes[0] = 0;
    if (vpn_kv(&kv, "routes")) snprintf(routes, sizeof routes, "%s", vpn_kv(&kv, "routes"));
    r.routes = routes[0] ? routes : NULL;
    /* openfortivpn config (0600): "key = value" up to the end of the line */
    Buf c = {0};
    buf_printf(&c, "# Generated by LordOS (do not edit)\nhost = %s\nport = %s\nusername = %s\npassword = %s\n",
               vpn_kv(&kv, "host"), vpn_kv(&kv, "port") ? vpn_kv(&kv, "port") : "443", vpn_kv(&kv, "user"), vpn_kv(&kv, "password"));
    if (vpn_kv(&kv, "trusted-cert")) {
        char copy[256];
        snprintf(copy, sizeof copy, "%s", vpn_kv(&kv, "trusted-cert"));
        for (char *t = strtok(copy, ", "); t; t = strtok(NULL, ", ")) buf_printf(&c, "trusted-cert = %s\n", t);
    }
    /* routes and DNS are applied by LordOS (openfortivpn with set-routes would change the system's
     * default route): full tunnel like "def1" or the import's routes */
    buf_printf(&c, "set-routes = 0\nset-dns = 0\npppd-use-peerdns = 0\n");
    vpn_kv_free(&kv);
    char path[200];
    mkdir(NET_DIR, 0755);
    mkdir(FORTI_DIR, 0700);
    snprintf(path, sizeof path, FORTI_DIR "/%s.conf", name);
    int ok = !c.oom && write_file_atomic(path, c.p, c.len, 0600) == 0;
    if (c.p) memset(c.p, 0, c.len);
    buf_free(&c);
    if (!ok) { admin_event("net vpn %s error vpn.writeFailed", name); admin_event("net vpn %s disconnected", name); return; }
    char ifarg[48];
    snprintf(ifarg, sizeof ifarg, "--pppd-ifname=%s", r.ifn);
    const char *argv[] = { OPENFORTIVPN, "-c", path, ifarg, "--pppd-no-peerdns", NULL };
    vpn_mark(r.ifn, name, 1);
    int out;
    pid_t pid = vpn_spawn(argv, &out, NULL, NULL);
    if (pid < 0) { admin_event("net vpn %s error vpn.startFailed", name); admin_event("net vpn %s disconnected", name); vpn_mark(r.ifn, name, 0); return; }
    supervise(&r, pid, out, forti_line, NULL);
    close(out);
    down(&r);
}
