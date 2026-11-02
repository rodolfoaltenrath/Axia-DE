/* ovpn.c — OpenVPN (Debian's program, fixed path), root side.
 *
 * Import: the .ovpn is read line by line and only the client options in the `allowed` list pass
 * (with the certificates and keys embedded <ca>…</ca>); any other one (up, down,
 * script-security, plugin, route-up, management, log, config, cd, setenv…) rejects the file.
 * It's saved in /etc/lordos/net/vpn/<name>.ovpn (0600) and the metadata in <name>.json.
 *
 * Connect: a resident process "vpn:<name>" writes the final config (the saved one + the
 * options LordOS sets: interface, management interface over a Unix socket, verb 3,
 * script-security 1), launches openvpn and drives it through the management interface: answers
 * user and password (from <name>.auth, 0600), follows the states and applies the DNS the
 * server pushes (OpenVPN on Linux doesn't apply them without scripts; here they're read from PUSH_REPLY). */
#include "netpriv.h"
#include "../admin.h"

#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <net/if.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

#define OVPN_BIN "/usr/sbin/openvpn"
#define OVPN_RUN NET_RUN_DIR "/ovpn"

/* Allowed client options. 0 = no file arguments; the certificate ones only
 * embedded. */
static const char *const allowed[] = {
    "client", "tls-client", "pull", "nobind", "float", "persist-key", "persist-tun", "resolv-retry",
    "dev", "dev-type", "proto", "remote", "remote-random", "remote-random-hostname", "port", "rport",
    "connect-retry", "connect-retry-max", "connect-timeout", "server-poll-timeout", "hand-window",
    "cipher", "data-ciphers", "data-ciphers-fallback", "ncp-ciphers", "ncp-disable", "auth", "tls-cipher",
    "tls-ciphersuites", "tls-groups", "tls-version-min", "tls-version-max", "key-direction",
    "remote-cert-tls", "remote-cert-ku", "remote-cert-eku", "verify-x509-name", "ns-cert-type",
    "auth-user-pass", "auth-nocache", "auth-retry", "comp-lzo", "compress", "allow-compression",
    "tun-mtu", "tun-mtu-extra", "mssfix", "fragment", "link-mtu", "reneg-sec", "reneg-bytes",
    "ping", "ping-restart", "ping-exit", "keepalive", "inactive", "explicit-exit-notify",
    "redirect-gateway", "route", "route-ipv6", "route-nopull", "route-metric", "route-delay",
    "block-ipv6", "topology", "mute", "mute-replay-warnings", "replay-window", "sndbuf", "rcvbuf",
    "fast-io", "tun-ipv6", "push-peer-info", "pull-filter", "ignore-unknown-option", "dhcp-option",
    "verb", "tls-timeout", "tls-exit", "peer-fingerprint", "disable-dco", "remote-cert-tls",
    NULL,
};
static const char *const blocks[] = { "ca", "cert", "key", "tls-auth", "tls-crypt", "tls-crypt-v2", "extra-certs", "peer-fingerprint", NULL };
/* LordOS sets these: they're removed from the file. */
static const char *const replaced[] = { "dev", "verb", "auth-retry", NULL };
/* Only valid on other operating systems: ignored. */
static const char *const windows_only[] = { "block-outside-dns", "register-dns", "route-method", "ip-win32", "tap-sleep", NULL };

static int in_list(const char *const *l, const char *w) {
    for (int i = 0; l[i]; i++) if (!strcmp(l[i], w)) return 1;
    return 0;
}

static int arg_ok(const char *s) {
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) if (*p < 0x20 || *p == '\\' || *p == 0x7f) return 0;
    return 1;
}

int ovpn_import(const char *name, const char *text, char *err, size_t errn) {
    char *copy = strdup(text);
    if (!copy) { lerr_set(err, errn, "sys.outOfMemory", NULL); return -1; }
    Buf out = {0};
    char block[24] = "", dev_type[8] = "tun", server[200] = "";
    int lineno = 0, rc = 0, needs_auth = 0, has_ca = 0, client = 0, ignored = 0;
    for (char *l = copy, *next; l && !rc; l = next) {
        next = strchr(l, '\n');
        if (next) *next++ = 0;
        lineno++;
        size_t n = strlen(l);
        if (n && l[n - 1] == '\r') l[--n] = 0;
        if (block[0]) {
            char close_tag[32];
            snprintf(close_tag, sizeof close_tag, "</%s>", block);
            char *t = l;
            while (isspace((unsigned char)*t)) t++;
            if (!strncmp(t, close_tag, strlen(close_tag))) { buf_printf(&out, "%s\n", close_tag); block[0] = 0; continue; }
            /* certificate/key contents: base64, dashes, PEM headers, hex */
            for (char *p = l; *p; p++) if (!isalnum((unsigned char)*p) && !strchr("+/=- :\t.#,()", *p)) { lerr_set(err, errn, "vpn.badChar", "line", lerr_int(lineno), "key", block, NULL); rc = -1; break; }
            if (!rc) buf_printf(&out, "%s\n", l);
            continue;
        }
        char *t = l;
        while (isspace((unsigned char)*t)) t++;
        if (!*t || *t == '#' || *t == ';') continue;
        if (*t == '<') {
            char tag[24];
            if (sscanf(t, "<%23[a-z0-9-]>", tag) != 1 || !in_list(blocks, tag)) { lerr_set(err, errn, "ovpn.blockNotAllowed", "line", lerr_int(lineno), "block", t, NULL); rc = -1; break; }
            snprintf(block, sizeof block, "%s", tag);
            if (!strcmp(tag, "ca")) has_ca = 1;
            buf_printf(&out, "<%s>\n", tag);
            continue;
        }
        if (!strncmp(t, "--", 2)) t += 2;
        char w[40] = "";
        int k = 0;
        while (t[k] && !isspace((unsigned char)t[k]) && k < 39) { w[k] = t[k]; k++; }
        w[k] = 0;
        char *args = t + k;
        while (isspace((unsigned char)*args)) args++;
        if (in_list(windows_only, w)) { ignored++; continue; }
        if (in_list(blocks, w)) {
            /* "ca file": it has to come embedded (lord.net.prepareOvpn does that) */
            if (strncmp(args, "[inline]", 8)) { lerr_set(err, errn, "ovpn.mustBeInline", "line", lerr_int(lineno), "option", w, NULL); rc = -1; break; }
            continue;
        }
        if (!in_list(allowed, w)) { lerr_set(err, errn, "ovpn.optionNotAllowed", "line", lerr_int(lineno), "option", w, NULL); rc = -1; break; }
        if (!arg_ok(args)) { lerr_set(err, errn, "ovpn.invalidArgument", "line", lerr_int(lineno), NULL); rc = -1; break; }
        if (!strcmp(w, "client") || !strcmp(w, "tls-client")) client = 1;
        if (!strcmp(w, "auth-user-pass")) {
            if (*args) { lerr_set(err, errn, "ovpn.authFile", "line", lerr_int(lineno), NULL); rc = -1; break; }
            needs_auth = 1;
        }
        if (!strcmp(w, "dev")) { if (!strncmp(args, "tap", 3)) { lerr_set(err, errn, "ovpn.onlyTun", NULL); rc = -1; } continue; }
        if (!strcmp(w, "dev-type")) { if (strcmp(args, "tun")) { lerr_set(err, errn, "ovpn.onlyTun", NULL); rc = -1; } snprintf(dev_type, sizeof dev_type, "tun"); continue; }
        if (!strcmp(w, "remote") && !server[0]) snprintf(server, sizeof server, "%s", args);
        if (in_list(replaced, w)) continue;
        buf_printf(&out, "%s%s%s\n", w, *args ? " " : "", args);
    }
    free(copy);
    if (!rc && block[0]) { lerr_set(err, errn, "ovpn.blockUnclosed", "block", block, NULL); rc = -1; }
    if (!rc && !client) { lerr_set(err, errn, "ovpn.notClient", NULL); rc = -1; }
    if (!rc && !has_ca) { lerr_set(err, errn, "ovpn.noCa", NULL); rc = -1; }
    if (!rc && !server[0]) { lerr_set(err, errn, "vpn.missingKey", "key", "remote", NULL); rc = -1; }
    if (rc || out.oom) { buf_free(&out); if (!rc) lerr_set(err, errn, "sys.outOfMemory", NULL); return -1; }
    if (ignored) admin_log("ovpn.ignoredWindows", "count", lerr_int(ignored), NULL);
    char path[160];
    snprintf(path, sizeof path, NET_VPN_DIR "/%s.ovpn", name);
    int ok = write_file_atomic(path, out.p, out.len, 0600) == 0;
    buf_free(&out);
    char ifn[IFNAMSIZ], authp[160];
    vpn_ifname("openvpn", name, ifn, sizeof ifn);
    snprintf(authp, sizeof authp, NET_VPN_DIR "/%s.auth", name);
    Buf m = {0};
    buf_printf(&m, "{\"name\":");
    buf_json_str(&m, name);
    buf_printf(&m, ",\"type\":\"openvpn\",\"ifname\":\"%s\",\"server\":", ifn);
    buf_json_str(&m, server);
    buf_printf(&m, ",\"needsAuth\":%s,\"hasCredentials\":%s}\n", needs_auth ? "true" : "false", access(authp, F_OK) == 0 ? "true" : "false");
    snprintf(path, sizeof path, NET_VPN_DIR "/%s.json", name);
    ok = ok && m.p && write_file_atomic(path, m.p, m.len, 0644) == 0;
    buf_free(&m);
    if (!ok) { lerr_set(err, errn, "net.saveFailed", NULL); return -1; }
    return 0;
}

int ovpn_set_credentials(const char *name, const char *user, const char *pass) {
    if (!arg_ok(user) || !arg_ok(pass) || strlen(user) > 128 || strlen(pass) > 256 || !*user) return -1;
    char path[160], buf[420];
    int n = snprintf(buf, sizeof buf, "%s\n%s\n", user, pass);
    snprintf(path, sizeof path, NET_VPN_DIR "/%s.auth", name);
    int r = write_file_atomic(path, buf, (size_t)n, 0600);
    memset(buf, 0, sizeof buf);
    /* update hasCredentials in the metadata */
    snprintf(path, sizeof path, NET_VPN_DIR "/%s.json", name);
    char *t = read_file(path, 8192, NULL);
    if (t) {
        char *p = strstr(t, "\"hasCredentials\":false");
        if (p) { memmove(p + 21, p + 22, strlen(p + 22) + 1); memcpy(p, "\"hasCredentials\":true", 21); write_file_atomic(path, t, strlen(t), 0644); }
        free(t);
    }
    return r;
}

/* ---- the resident process ---- */

static volatile sig_atomic_t stop_flag;
static void on_term(int s) { (void)s; stop_flag = 1; }

/* Management interface quoting: \ and " with a backslash. */
static void mquote(const char *in, char *out, size_t n) {
    size_t o = 0;
    for (const char *p = in; *p && o + 3 < n; p++) {
        if (*p == '\\' || *p == '"') out[o++] = '\\';
        out[o++] = *p;
    }
    out[o] = 0;
}

static void msend(int fd, const char *s) {
    size_t n = strlen(s);
    while (n) { ssize_t w = write(fd, s, n); if (w <= 0) return; s += w; n -= (size_t)w; }
}

typedef struct { char dns[600], domains[600]; int applied, full; } PushDns;

/* PUSH: Received control message: 'PUSH_REPLY,route ...,dhcp-option DNS 10.8.0.1,...' */
static void parse_push(const char *line, PushDns *d) {
    const char *p = strstr(line, "PUSH_REPLY,");
    if (!p) return;
    d->dns[0] = d->domains[0] = 0;
    if (strstr(p, "redirect-gateway")) d->full = 1;
    char copy[2048];
    snprintf(copy, sizeof copy, "%s", p + 11);
    char *q = strrchr(copy, '\'');
    if (q) *q = 0;
    for (char *t = strtok(copy, ","); t; t = strtok(NULL, ",")) {
        char kind[16], val[80];
        if (sscanf(t, "dhcp-option %15s %79s", kind, val) == 2) {
            int fam;
            uint8_t a[16];
            if (!strcmp(kind, "DNS") || !strcmp(kind, "DNS6")) {
                if (!parse_ip(val, &fam, a) && strlen(d->dns) + strlen(val) + 2 < sizeof d->dns)
                    snprintf(d->dns + strlen(d->dns), sizeof d->dns - strlen(d->dns), "%s%s", d->dns[0] ? " " : "", val);
            } else if (!strcmp(kind, "DOMAIN") || !strcmp(kind, "DOMAIN-SEARCH")) {
                int ok = 1;
                for (char *c = val; *c; c++) if (!isalnum((unsigned char)*c) && *c != '-' && *c != '.') ok = 0;
                if (ok && strlen(d->domains) + strlen(val) + 2 < sizeof d->domains)
                    snprintf(d->domains + strlen(d->domains), sizeof d->domains - strlen(d->domains), "%s%s", d->domains[0] ? " " : "", val);
            }
        }
    }
}

void ovpn_run(const char *name) {
    struct sigaction sa = { .sa_handler = on_term };
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);
    char ifn[IFNAMSIZ], path[160], sock[100], conf[160];
    vpn_ifname("openvpn", name, ifn, sizeof ifn);
    snprintf(path, sizeof path, NET_VPN_DIR "/%s.ovpn", name);
    char *stored = read_file(path, 65536, NULL);
    if (!stored) { admin_event("net vpn %s error vpn.noConfig", name); return; }
    mkdir(NET_RUN_DIR, 0755);
    mkdir(OVPN_RUN, 0700);
    snprintf(sock, sizeof sock, OVPN_RUN "/%s.sock", name);
    snprintf(conf, sizeof conf, OVPN_RUN "/%s.conf", name);
    unlink(sock);
    Buf c = {0};
    buf_printf(&c, "%s\n# --- LordOS ---\ndev %s\ndev-type tun\nverb 3\nscript-security 1\nmanagement %s unix\n"
               "management-query-passwords\nauth-retry interact\n", stored, ifn, sock);
    int local_full = strstr(stored, "\nredirect-gateway") != NULL || !strncmp(stored, "redirect-gateway", 16);
    memset(stored, 0, strlen(stored));
    free(stored);
    if (c.oom || write_file_atomic(conf, c.p, c.len, 0600)) { buf_free(&c); admin_event("net vpn %s error vpn.writeFailed", name); return; }
    memset(c.p, 0, c.len);
    buf_free(&c);

    int p[2];
    if (pipe2(p, O_CLOEXEC)) return;
    pid_t pid = fork();
    if (pid == 0) {
        int null = open("/dev/null", O_RDWR);
        dup2(null, 0);
        dup2(p[1], 1);
        dup2(p[1], 2);
        syscall(SYS_close_range, 3, ~0u, 0);
        prctl(PR_SET_PDEATHSIG, SIGTERM);
        char *const env[] = { "PATH=/usr/sbin:/usr/bin:/sbin:/bin", "LANG=C.UTF-8", NULL };
        char *const argv[] = { OVPN_BIN, "--config", conf, NULL };
        execve(OVPN_BIN, argv, env);
        _exit(127);
    }
    close(p[1]);
    if (pid < 0) { close(p[0]); admin_event("net vpn %s error vpn.startFailed", name); return; }
    mkdir(NET_RUN_DIR "/vpn", 0755);
    char mark[128], content[64];
    snprintf(mark, sizeof mark, NET_RUN_DIR "/vpn/%s", ifn);
    int n = snprintf(content, sizeof content, "%s\n0\n", name);
    write_file_atomic(mark, content, (size_t)n, 0644);
    admin_event("net vpn %s connecting", name);

    int mfd = -1;
    char lbuf[4096], mbuf[4096];
    size_t llen = 0, mlen = 0;
    PushDns dns = { .full = local_full };
    int connected = 0;
    while (!stop_flag) {
        if (mfd < 0 && access(sock, F_OK) == 0) {
            mfd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
            struct sockaddr_un su = { .sun_family = AF_UNIX };
            snprintf(su.sun_path, sizeof su.sun_path, "%s", sock);
            if (connect(mfd, (struct sockaddr *)&su, sizeof su)) { close(mfd); mfd = -1; }
            else msend(mfd, "state on\n");
        }
        struct pollfd pf[2] = { { p[0], POLLIN, 0 }, { mfd, POLLIN, 0 } };
        if (poll(pf, 2, mfd < 0 ? 200 : 2000) < 0) continue;
        if (pf[0].revents) {
            ssize_t r = read(p[0], lbuf + llen, sizeof lbuf - 1 - llen);
            if (r <= 0) break;                          /* openvpn exited */
            llen += (size_t)r;
            char *nl;
            while ((nl = memchr(lbuf, '\n', llen))) {
                *nl = 0;
                if (strstr(lbuf, "PUSH_REPLY")) parse_push(lbuf, &dns);
                if (strstr(lbuf, "Options error") || strstr(lbuf, "Cannot") || strstr(lbuf, "TLS Error")) admin_event("net vpn %s log %.200s", name, lbuf);
                size_t used = (size_t)(nl - lbuf) + 1;
                memmove(lbuf, nl + 1, llen - used);
                llen -= used;
            }
            if (llen >= sizeof lbuf - 1) llen = 0;
        }
        if (mfd >= 0 && pf[1].revents) {
            ssize_t r = read(mfd, mbuf + mlen, sizeof mbuf - 1 - mlen);
            if (r <= 0) { close(mfd); mfd = -1; continue; }
            mlen += (size_t)r;
            char *nl;
            while ((nl = memchr(mbuf, '\n', mlen))) {
                *nl = 0;
                if (nl > mbuf && nl[-1] == '\r') nl[-1] = 0;
                if (!strncmp(mbuf, ">PASSWORD:Need 'Auth'", 21)) {
                    char ap[160];
                    snprintf(ap, sizeof ap, NET_VPN_DIR "/%s.auth", name);
                    char *a = read_file(ap, 512, NULL);
                    char *nl2 = a ? strchr(a, '\n') : NULL;
                    if (!nl2) { admin_event("net vpn %s need-auth", name); stop_flag = 1; }
                    else {
                        *nl2 = 0;
                        char *pw = nl2 + 1;
                        pw[strcspn(pw, "\n")] = 0;
                        char qu[300], qp[520], cmd[900];
                        mquote(a, qu, sizeof qu);
                        mquote(pw, qp, sizeof qp);
                        snprintf(cmd, sizeof cmd, "username \"Auth\" \"%s\"\npassword \"Auth\" \"%s\"\n", qu, qp);
                        msend(mfd, cmd);
                        memset(cmd, 0, sizeof cmd); memset(qp, 0, sizeof qp);
                    }
                    if (a) { memset(a, 0, strlen(a)); free(a); }
                } else if (!strncmp(mbuf, ">PASSWORD:Verification Failed", 29)) {
                    admin_event("net vpn %s auth-failed", name);
                    stop_flag = 1;
                } else if (!strncmp(mbuf, ">STATE:", 7)) {
                    /* >STATE:time,STATE,detail,local_ip,remote_ip,... */
                    char st[32] = "", rest[256] = "";
                    sscanf(mbuf + 7, "%*[^,],%31[^,],%255[^\n]", st, rest);
                    if (!strcmp(st, "CONNECTED")) {
                        char local[64] = "";
                        sscanf(rest, "%*[^,],%63[^,]", local);
                        if (dns.dns[0] && !dns.applied) { dns_set(ifn, dns.dns, dns.domains, dns.full ? DNS_FULL : DNS_SPLIT); dns.applied = 1; }
                        connected = 1;
                        admin_event("net vpn %s connected %s", name, local);
                    } else if (!strcmp(st, "RECONNECTING") || !strcmp(st, "EXITING")) {
                        if (connected) admin_event("net vpn %s %s", name, !strcmp(st, "EXITING") ? "disconnected" : "reconnecting");
                        connected = 0;
                        dns.applied = 0;
                    } else if (st[0]) {
                        for (char *q = st; *q; q++) *q = (char)tolower((unsigned char)*q);
                        admin_event("net vpn %s state %s", name, st);
                    }
                }
                size_t used = (size_t)(nl - mbuf) + 1;
                memmove(mbuf, nl + 1, mlen - used);
                mlen -= used;
            }
            if (mlen >= sizeof mbuf - 1) mlen = 0;
        }
    }
    dns_revert(ifn);                        /* while the interface exists */
    if (mfd >= 0) { msend(mfd, "signal SIGTERM\n"); close(mfd); }
    kill(pid, SIGTERM);
    for (int i = 0; i < 50; i++) { if (waitpid(pid, NULL, WNOHANG) == pid) { pid = 0; break; } usleep(100000); }
    if (pid) { kill(pid, SIGKILL); waitpid(pid, NULL, 0); }
    close(p[0]);
    unlink(mark);
    unlink(conf);
    unlink(sock);
    admin_event("net vpn %s disconnected", name);
}

int ovpn_down(const char *name) {
    char d[40];
    snprintf(d, sizeof d, "vpn:%s", name);
    return admin_daemon_stop(d);
}

int ovpn_status_json(const char *name, Buf *out) {
    char ifn[IFNAMSIZ], d[40];
    vpn_ifname("openvpn", name, ifn, sizeof ifn);
    snprintf(d, sizeof d, "vpn:%s", name);
    buf_printf(out, "{\"name\":");
    buf_json_str(out, name);
    buf_printf(out, ",\"type\":\"openvpn\",\"ifname\":\"%s\",\"running\":%s,\"active\":%s}", ifn,
               admin_daemon_running(d) ? "true" : "false", rt_ifindex(ifn) ? "true" : "false");
    return 0;
}
