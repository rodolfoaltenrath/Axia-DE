/* ipsec.c — IPsec/IKEv2 with strongSwan (Debian's charon, fixed path; no swanctl), root side.
 *
 * Import (net-vpn-import ipsec\n<name>\n...), key=value:
 *   server=vpn.example.com        required
 *   remote_id=vpn.example.com     server identity (if missing, server)
 *   auth=eap | psk | cert         eap = user and password with EAP-MSCHAPv2 (the usual)
 *   user=… password=…             (eap)
 *   psk=… local_id=…              (psk; local_id optional)
 *   cert=<PEM> key=<PEM>          (cert; local_id optional, otherwise the certificate subject)
 *   ca=<PEM>                      optional: if missing, the system authorities (/etc/ssl/certs)
 *   ike=aes256-sha256-modp2048 esp=aes256gcm16   optional
 *   routes=10.0.0.0/8,…           optional: split tunnel; if missing, full
 *
 * Connect: a resident process "vpn:<name>" creates the XFRM interface ips-<name> (if_id),
 * writes that VPN's strongswan.conf (0600) in /etc/strongswan.d/lordos/ and launches its own
 * charon (random ports, its own VICI socket, no updown or scripts). Over VICI (vici.c, no
 * swanctl) it passes keys, secrets, certificates and the connection: none of that is left in files
 * another program reads. It requests a virtual IP (vips), which charon puts on ips-<name>; routes
 * through that interface and the DNS the server pushes (from charon's log) only on it.
 * AppArmor's paths (Debian's charon profile) rule: config in /etc/strongswan.d and
 * sockets in /run/charon.*. Only one IPsec at a time: charon uses /run/charon.pid. */
#include "vpn.h"
#include "vici.h"
#include "../admin.h"

#include <net/if.h>                 /* before the linux/ ones */
#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/netlink.h>
#include <linux/rtnetlink.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define CHARON  "/usr/lib/ipsec/charon"
#define SCONF_DIR "/etc/strongswan.d/lordos"     /* each VPN's strongswan.conf (AppArmor) */
#define VICI_PREFIX "/run/charon.lordos-"        /* VICI socket and DNS (AppArmor: /run/charon.*) */

static const char *const keys[] = { "server", "remote_id", "auth", "user", "password", "psk", "local_id", "ike", "esp", "routes", NULL };
static const char *const pems[] = { "ca", "cert", "key", NULL };

/* Identities (they go over VICI, but are still limited to what makes sense in an identity). */
static int id_ok(const char *s) {
    if (!*s || strlen(s) > 200) return 0;
    for (const char *p = s; *p; p++) if (!isalnum((unsigned char)*p) && !strchr("@._:+=-/, *", *p)) return 0;
    return 1;
}

static int proposal_ok(const char *s) {
    if (!*s || strlen(s) > 200) return 0;
    for (const char *p = s; *p; p++) if (!islower((unsigned char)*p) && !isdigit((unsigned char)*p) && !strchr("-_,", *p)) return 0;
    return 1;
}

int ipsec_import(const char *name, const char *text, char *err, size_t errn) {
    VpnKv kv;
    if (vpn_kv_parse(text, keys, pems, &kv, err, errn)) return -1;
    const char *server = vpn_kv(&kv, "server"), *auth = vpn_kv(&kv, "auth");
    int rc = 0;
    if (!server || !vpn_host_ok(server)) { lerr_set(err, errn, "vpn.missingKey", "key", "server", NULL); rc = -1; }
    else if (!auth || (strcmp(auth, "eap") && strcmp(auth, "psk") && strcmp(auth, "cert"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "auth", NULL); rc = -1; }
    else if (!strcmp(auth, "eap") && (!vpn_kv(&kv, "user") || !id_ok(vpn_kv(&kv, "user")) || !vpn_kv(&kv, "password"))) { lerr_set(err, errn, "ipsec.eapNeedsUser", NULL); rc = -1; }
    else if (!strcmp(auth, "psk") && (!vpn_kv(&kv, "psk") || strlen(vpn_kv(&kv, "psk")) < 8)) { lerr_set(err, errn, "ipsec.pskShort", NULL); rc = -1; }
    else if (!strcmp(auth, "cert") && (!vpn_kv_pem(&kv, "cert") || !vpn_kv_pem(&kv, "key"))) { lerr_set(err, errn, "ipsec.certNeedsKey", NULL); rc = -1; }
    else if (vpn_kv(&kv, "remote_id") && !id_ok(vpn_kv(&kv, "remote_id"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "remote_id", NULL); rc = -1; }
    else if (vpn_kv(&kv, "local_id") && !id_ok(vpn_kv(&kv, "local_id"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "local_id", NULL); rc = -1; }
    else if (vpn_kv(&kv, "ike") && !proposal_ok(vpn_kv(&kv, "ike"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "ike", NULL); rc = -1; }
    else if (vpn_kv(&kv, "esp") && !proposal_ok(vpn_kv(&kv, "esp"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "esp", NULL); rc = -1; }
    else if (vpn_kv(&kv, "routes") && !vpn_routes_valid(vpn_kv(&kv, "routes"))) { lerr_set(err, errn, "vpn.invalidValue", "key", "routes", NULL); rc = -1; }
    if (!rc && vpn_kv_save(name, "ipsec", &kv)) { lerr_set(err, errn, "net.saveFailed", NULL); rc = -1; }
    if (!rc) {
        char ifn[IFNAMSIZ];
        vpn_ifname("ipsec", name, ifn, sizeof ifn);
        Buf m = {0};
        buf_printf(&m, "{\"name\":");
        buf_json_str(&m, name);
        buf_printf(&m, ",\"type\":\"ipsec\",\"ifname\":\"%s\",\"server\":", ifn);
        buf_json_str(&m, server);
        buf_printf(&m, ",\"auth\":\"%s\",\"user\":", auth);
        buf_json_str(&m, vpn_kv(&kv, "user") ? vpn_kv(&kv, "user") : "");
        buf_printf(&m, ",\"fullTunnel\":%s,\"insecure\":false}\n", vpn_kv(&kv, "routes") ? "false" : "true");
        if (!m.p || vpn_meta_save(name, m.p)) { lerr_set(err, errn, "net.saveFailed", NULL); rc = -1; }
        buf_free(&m);
    }
    vpn_kv_free(&kv);
    return rc;
}

/* This VPN's strongswan.conf. It goes in /etc/strongswan.d/lordos/ (charon's AppArmor profile
 * only lets it read /etc/strongswan.d and write /run/charon.<something>); the system's
 * strongswan.conf doesn't include it (it only includes first-level *.conf and charon/). */
static int write_sconf(const char *name, const char *ifn, const char *path) {
    Buf s = {0};
    buf_printf(&s,
        "# Generated by LordOS for the VPN %s (do not edit)\n"
        "charon {\n"
        "  port = 0\n  port_nat_t = 0\n"
        "  install_routes = no\n  install_virtual_ip = yes\n  install_virtual_ip_on = %s\n"
        "  retransmit_tries = 4\n"
        "  load = random nonce openssl pem pkcs1 pkcs8 x509 revocation constraints pubkey kernel-netlink socket-default vici resolve attr eap-identity eap-mschapv2\n"
        "  plugins {\n"
        "    vici {\n      socket = unix://" VICI_PREFIX "%s.vici\n    }\n"
        "    resolve {\n      resolvconf {\n        path = /nonexistent\n      }\n    }\n"
        "    openssl {\n      load_legacy = yes\n    }\n"
        "  }\n}\n", name, ifn, name);
    int ok = !s.oom && write_file_atomic(path, s.p, s.len, 0600) == 0;
    buf_free(&s);
    return ok ? 0 : -1;
}

static int load_pem(int fd, const char *cmd, const char *type, const char *flag, const char *pem, char *err, size_t errn) {
    ViciMsg m = {0};
    vici_kv(&m, "type", type);
    if (flag) vici_kv(&m, "flag", flag);
    vici_kv(&m, "data", pem);
    ViciResult r;
    int rc = vici_simple(fd, cmd, &m, &r, 5000);
    vici_free(&m);
    if (rc && err) lerr_set(err, errn, "ipsec.viciFailed", "cmd", cmd, "detail", r.errmsg, NULL);
    return rc;
}

/* System authorities (if the import brings no ca). */
static int load_system_cas(int fd) {
    DIR *d = opendir("/etc/ssl/certs");
    struct dirent *e;
    int n = 0;
    while (d && (e = readdir(d)) && n < 500) {
        size_t l = strlen(e->d_name);
        if (l < 5 || strcmp(e->d_name + l - 4, ".pem")) continue;
        char p[400];
        snprintf(p, sizeof p, "/etc/ssl/certs/%s", e->d_name);
        char *pem = read_file(p, 65536, NULL);
        if (!pem) continue;
        if (!load_pem(fd, "load-cert", "X509", "CA", pem, NULL, 0)) n++;
        free(pem);
    }
    if (d) closedir(d);
    return n;
}

static void list_items(ViciMsg *m, const char *name, const char *csv) {
    char copy[300];
    snprintf(copy, sizeof copy, "%s", csv);
    vici_list_begin(m, name);
    for (char *t = strtok(copy, ", "); t; t = strtok(NULL, ", ")) vici_item(m, t);
    vici_list_end(m);
}

/* Loads keys, secrets and the "lordos" connection into this VPN's charon. */
static int vici_load(int fd, const VpnKv *kv, uint32_t if_id, char *err, size_t errn) {
    const char *auth = vpn_kv(kv, "auth"), *server = vpn_kv(kv, "server");
    const char *rid = vpn_kv(kv, "remote_id") ? vpn_kv(kv, "remote_id") : server;
    const char *lid = vpn_kv(kv, "local_id"), *routes = vpn_kv(kv, "routes");
    ViciResult r;
    if (!vpn_kv_pem(kv, "ca") && load_system_cas(fd) == 0 && strcmp(auth, "psk")) { lerr_set(err, errn, "ipsec.noCas", NULL); return -1; }
    if (!strcmp(auth, "cert") && load_pem(fd, "load-key", "any", NULL, vpn_kv_pem(kv, "key"), err, errn)) return -1;
    if (!strcmp(auth, "eap") || !strcmp(auth, "psk")) {
        ViciMsg m = {0};
        vici_kv(&m, "type", !strcmp(auth, "eap") ? "EAP" : "IKE");
        vici_kv(&m, "data", vpn_kv(kv, !strcmp(auth, "eap") ? "password" : "psk"));
        vici_list_begin(&m, "owners");
        if (!strcmp(auth, "eap")) vici_item(&m, vpn_kv(kv, "user"));
        else { vici_item(&m, rid); if (lid) vici_item(&m, lid); }
        vici_list_end(&m);
        int rc = vici_simple(fd, "load-shared", &m, &r, 5000);
        vici_free(&m);
        if (rc) { lerr_set(err, errn, "ipsec.viciFailed", "cmd", "load-shared", "detail", r.errmsg, NULL); return -1; }
    }
    char hex[16];
    snprintf(hex, sizeof hex, "0x%x", if_id);
    ViciMsg m = {0};
    vici_begin(&m, "lordos");
    vici_kv(&m, "version", "2");
    list_items(&m, "remote_addrs", server);
    list_items(&m, "vips", "0.0.0.0,::");
    vici_kv(&m, "if_id_in", hex);
    vici_kv(&m, "if_id_out", hex);
    vici_kv(&m, "dpd_delay", "30s");
    if (vpn_kv(kv, "ike")) list_items(&m, "proposals", vpn_kv(kv, "ike"));
    vici_begin(&m, "local");
    if (!strcmp(auth, "eap")) {
        vici_kv(&m, "auth", "eap-mschapv2");
        vici_kv(&m, "eap_id", vpn_kv(kv, "user"));
        vici_kv(&m, "id", lid ? lid : vpn_kv(kv, "user"));
    } else if (!strcmp(auth, "psk")) {
        vici_kv(&m, "auth", "psk");
        if (lid) vici_kv(&m, "id", lid);
    } else {
        vici_kv(&m, "auth", "pubkey");
        vici_list_begin(&m, "certs");
        vici_item(&m, vpn_kv_pem(kv, "cert"));
        vici_list_end(&m);
        if (lid) vici_kv(&m, "id", lid);
    }
    vici_end(&m);
    vici_begin(&m, "remote");
    vici_kv(&m, "auth", !strcmp(auth, "psk") ? "psk" : "pubkey");
    vici_kv(&m, "id", rid);
    if (vpn_kv_pem(kv, "ca")) { vici_list_begin(&m, "cacerts"); vici_item(&m, vpn_kv_pem(kv, "ca")); vici_list_end(&m); }
    vici_end(&m);
    vici_begin(&m, "children");
    vici_begin(&m, "net");
    list_items(&m, "remote_ts", routes ? routes : "0.0.0.0/0,::/0");
    vici_kv(&m, "if_id_in", hex);
    vici_kv(&m, "if_id_out", hex);
    vici_kv(&m, "dpd_action", "clear");
    if (vpn_kv(kv, "esp")) list_items(&m, "esp_proposals", vpn_kv(kv, "esp"));
    vici_end(&m);
    vici_end(&m);
    vici_end(&m);
    int rc = vici_simple(fd, "load-conn", &m, &r, 5000);
    vici_free(&m);
    if (rc) { lerr_set(err, errn, "ipsec.viciFailed", "cmd", "load-conn", "detail", r.errmsg, NULL); return -1; }
    return 0;
}

typedef struct { char log[2048]; ViciResult res; } InitCtx;

static void init_log(const char *path, const char *key, const char *val, void *ud) {
    InitCtx *c = ud;
    (void)path;
    if (!strcmp(key, "msg") && strlen(c->log) + strlen(val) + 2 < sizeof c->log)
        snprintf(c->log + strlen(c->log), sizeof c->log - strlen(c->log), "%s\n", val);
}

static void init_reply(const char *path, const char *key, const char *val, void *ud) { vici_result_cb(path, key, val, &((InitCtx *)ud)->res); }

typedef struct { char vip[64]; int established; } SaCtx;

static void sa_cb(const char *path, const char *key, const char *val, void *ud) {
    SaCtx *s = ud;
    if (strcmp(path, "lordos")) return;
    if (!strcmp(key, "state") && !strcmp(val, "ESTABLISHED")) s->established = 1;
    if (!strcmp(key, "local-vips") && !s->vip[0] && strchr(val, '.')) snprintf(s->vip, sizeof s->vip, "%s", val);
}

static int list_sas(int fd, SaCtx *s) {
    memset(s, 0, sizeof *s);
    ViciMsg m = {0};
    vici_kv(&m, "ike", "lordos");
    int rc = vici_call(fd, "list-sas", &m, "list-sa", sa_cb, NULL, s, 5000);
    vici_free(&m);
    return rc;
}

void ipsec_run(const char *name) {
    vpn_signals();
    VpnKv kv;
    if (vpn_kv_load(name, "ipsec", &kv)) { admin_event("net vpn %s error vpn.noConfig", name); admin_event("net vpn %s disconnected", name); return; }
    char ifn[IFNAMSIZ], sconf[200], sock[160], dnsf[160], err[300] = "";
    vpn_ifname("ipsec", name, ifn, sizeof ifn);
    snprintf(sconf, sizeof sconf, SCONF_DIR "/%s.conf", name);
    snprintf(sock, sizeof sock, VICI_PREFIX "%s.vici", name);
    snprintf(dnsf, sizeof dnsf, VICI_PREFIX "%s.dns", name);
    mkdir("/etc/strongswan.d", 0755);
    mkdir(SCONF_DIR, 0700);
    unlink(dnsf);
    unlink(sock);
    admin_event("net vpn %s connecting", name);
    int sfam = 0;
    uint8_t sip[16];
    if (vpn_resolve(vpn_kv(&kv, "server"), &sfam, sip)) { admin_event("net vpn %s error vpn.serverUnresolved", name); admin_event("net vpn %s disconnected", name); vpn_kv_free(&kv); return; }
    /* if_id: from the name, never 0 */
    uint32_t if_id = 0x4c000000u;
    for (const char *p = name; *p; p++) if_id = (if_id * 31 + (unsigned char)*p) | 0x4c000000u;
    if_id &= 0x4cffffffu;
    int fd = nl_open(NETLINK_ROUTE, 0);
    if (fd < 0) { vpn_kv_free(&kv); return; }
    if (rt_ifindex(ifn)) rt_link_del(fd, rt_ifindex(ifn));
    int r = rt_link_new_xfrm(fd, ifn, if_id);
    if (r) { close(fd); admin_event("net vpn %s error vpn.xfrmFailed errno=%s", name, lerr_errno(-r)); admin_event("net vpn %s disconnected", name); vpn_kv_free(&kv); return; }
    rt_link_set(fd, rt_ifindex(ifn), 1);
    close(fd);
    vpn_mark(ifn, name, 1);
    pid_t charon = -1;
    int cout = -1, vfd = -1, connected = 0;
    if (write_sconf(name, ifn, sconf)) { admin_event("net vpn %s error vpn.writeFailed", name); goto out; }
    char envs[300];
    snprintf(envs, sizeof envs, "STRONGSWAN_CONF=%s", sconf);
    const char *cargv[] = { CHARON, NULL };
    charon = vpn_spawn(cargv, &cout, NULL, envs);
    if (charon < 0) { admin_event("net vpn %s error ipsec.charonFailed", name); goto out; }
    for (int i = 0; i < 100 && (vfd = vici_connect(sock)) < 0; i++) usleep(100000);
    if (vfd < 0) { admin_event("net vpn %s error ipsec.noVici", name); goto out; }
    if (vici_load(vfd, &kv, if_id, err, sizeof err)) { admin_event("net vpn %s error %.200s", name, err); goto out; }
    InitCtx ic = { .log = "" };
    ViciMsg m = {0};
    vici_kv(&m, "child", "net");
    vici_kv(&m, "ike", "lordos");
    vici_kv(&m, "timeout", "30000");
    vici_kv(&m, "loglevel", "1");
    r = vici_call(vfd, "initiate", &m, "control-log", init_log, init_reply, &ic, 40000);
    vici_free(&m);
    if (r || !ic.res.success) {
        if (strstr(ic.log, "AUTHENTICATION_FAILED") || strstr(ic.log, "EAP_FAILURE") || strstr(ic.log, "MSCHAPV2") || strstr(ic.log, "authentication of"))
            admin_event("net vpn %s auth-failed", name);
        else {
            char *last = NULL;
            for (char *l = strtok(ic.log, "\n"); l; l = strtok(NULL, "\n")) if (strlen(l) > 8) last = l;
            admin_event("net vpn %s error %.160s", name, last ? last : ic.res.errmsg);
        }
        goto out;
    }
    SaCtx sa;
    list_sas(vfd, &sa);
    /* The DNS servers the server pushed: charon's resolve plugin wants to install them with
     * resolvconf (which we point at /nonexistent, so it touches nothing) and logs them first:
     * "installing DNS server 10.21.0.1 via resolvconf". LordOS applies them. */
    char servers[400] = "";
    {
        char lb[16384];
        ssize_t k;
        fcntl(cout, F_SETFL, fcntl(cout, F_GETFL) | O_NONBLOCK);
        while ((k = read(cout, lb, sizeof lb - 1)) > 0) {
            lb[k] = 0;
            for (char *l = strtok(lb, "\n"); l; l = strtok(NULL, "\n")) {
                const char *p = strstr(l, "installing DNS server ");
                char ip[64];
                int fam;
                uint8_t a[16];
                if (p && sscanf(p + 22, "%63s", ip) == 1 && !parse_ip(ip, &fam, a) && !strstr(servers, ip) && strlen(servers) + strlen(ip) + 2 < sizeof servers)
                    snprintf(servers + strlen(servers), sizeof servers - strlen(servers), "%s%s", servers[0] ? " " : "", ip);
            }
        }
    }
    const char *routes = vpn_kv(&kv, "routes");
    if (routes) vpn_routes_list(ifn, routes); else vpn_routes_full(ifn, sfam, sip);
    if (servers[0]) dns_set(ifn, servers, "", routes ? DNS_SPLIT : DNS_FULL);
    connected = 1;
    admin_event("net vpn %s connected %s", name, sa.vip);
    /* watch: charon alive and the SA established */
    time_t next = time(NULL) + 10;
    while (!vpn_stop) {
        struct pollfd pf = { cout, POLLIN, 0 };
        if (poll(&pf, 1, 1000) > 0) {
            char junk[4096];
            if (read(cout, junk, sizeof junk) <= 0) { admin_event("net vpn %s error ipsec.charonExited", name); break; }
        }
        if (time(NULL) >= next) {
            next = time(NULL) + 10;
            if (list_sas(vfd, &sa) || !sa.established) { admin_event("net vpn %s error vpn.lost", name); break; }
        }
    }
    {
        ViciMsg t = {0};
        vici_kv(&t, "ike", "lordos");
        vici_kv(&t, "timeout", "5000");
        ViciResult tr;
        vici_simple(vfd, "terminate", &t, &tr, 8000);
        vici_free(&t);
    }
out:
    if (connected) dns_revert(ifn);
    if (vfd >= 0) close(vfd);
    vpn_reap(charon);
    if (cout >= 0) close(cout);
    fd = nl_open(NETLINK_ROUTE, 0);
    if (fd >= 0) { if (rt_ifindex(ifn)) rt_link_del(fd, rt_ifindex(ifn)); close(fd); }
    vpn_routes_clear(sfam, sip);
    vpn_mark(ifn, name, 0);
    unlink(dnsf);
    unlink(sock);
    unlink(sconf);
    vpn_kv_free(&kv);
    admin_event("net vpn %s disconnected", name);
}
