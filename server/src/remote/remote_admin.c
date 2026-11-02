/* remote_admin.c — root actions of the remote desktop, in the privileged helper (admin.c).
 *
 *   remote-enable            turns the remote desktop on (enabled=1 and lord-sessiond rereads)
 *   remote-disable           turns it off; sessions stay open (they show on the monitor)
 *   remote-config <lines>    allow=all|group, port=1024..65535, net=lan|list|any
 *   remote-block <cidr>      blocks an address or network until removed (/etc/lordos/remote/access)
 *   remote-unblock <cidr>    removes manual blocks inside it and asks lord-sessiond to lift the
 *                            failed-login blocks inside it (/var/lib/lordos/remote/requests)
 *   remote-allow add|del <cidr>   the "only these addresses" list (net=list)
 *   remote-member <u>\n1|0   adds or removes a user from the lordos-remote group
 *   remote-kick <pid>        cuts a connection (only a pid from lord-sessiond's list)
 *   remote-ice <lines>       the WebRTC connection (docs/REMOTE.md, "Video (WebRTC)"):
 *                            stun=<host:port>[,…] (up to 4), turn=<host:port> (empty: none),
 *                            turn_transport=udp|tcp|tls, turn_fp=<sha-256> (TLS: a pinned certificate;
 *                            empty: the system's CAs), turn_user=<name>, turn_pass=<password> (kept in
 *                            turn.key, root 0600; never written to the config, the logs or an answer)
 *
 * The remote desktop is part of lord-sessiond (lordos.service, docs/SESSIONS.md): these
 * actions only change /etc/lordos/remote/config and notify it with SIGHUP. */
#include "remote_admin.h"
#include "ipnet.h"
#include "lockout.h"
#include "rtcwire.h"

#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <limits.h>
#include <pwd.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <unistd.h>

#define CONF_DIR "/etc/lordos/remote"
#define RD_ACCESS_PATH CONF_DIR "/access"
#define RD_TURN_KEY_PATH CONF_DIR "/turn.key"

static int write_file(const char *path, const char *text, mode_t mode) {
    char tmp[PATH_MAX];
    snprintf(tmp, sizeof tmp, "%s.lordos", path);
    int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC | O_NOFOLLOW | O_CLOEXEC, mode);
    if (fd < 0) return -1;
    size_t n = strlen(text);
    int ok = write(fd, text, n) == (ssize_t)n;
    if (close(fd)) ok = 0;
    return ok && !rename(tmp, path) ? 0 : -1;
}

typedef struct {
    int enabled, port, group_only, insecure_dev, net;
    int nstun, turn_key, turn_transport;
    char stun[4][96], turn[96], turn_user[129], turn_fp[96];
} Conf;

static void conf_read(Conf *c) {
    c->port = 5673;
    c->enabled = c->group_only = c->insecure_dev = 0;
    c->net = RS_NET_LAN;
    c->nstun = c->turn_key = 0;
    c->turn_transport = RW_TURN_UDP;
    c->turn[0] = c->turn_user[0] = c->turn_fp[0] = 0;
    FILE *f = fopen(CONF_DIR "/config", "r");
    char line[256], s[32], h[160];
    int v;
    while (f && fgets(line, sizeof line, f)) {
        if (sscanf(line, "turn_transport=%31s", s) == 1) { if (rw_turn_transport(s) >= 0) c->turn_transport = rw_turn_transport(s); continue; }
        if (sscanf(line, "turn_fp=%159s", h) == 1) { uint8_t fp[32]; if (!rw_fp_parse(h, fp)) snprintf(c->turn_fp, sizeof c->turn_fp, "%.95s", h); continue; }
        if (sscanf(line, "turn_key=%d", &v) == 1) { c->turn_key = v == 1; continue; }
        if (sscanf(line, "stun=%159s", h) == 1) { if (rw_ice_server_valid(h) && c->nstun < 4) snprintf(c->stun[c->nstun++], 96, "%.95s", h); continue; }
        if (sscanf(line, "turn=%159s", h) == 1) { if (rw_ice_server_valid(h)) snprintf(c->turn, sizeof c->turn, "%.95s", h); continue; }
        if (sscanf(line, "turn_user=%159s", h) == 1) { if (rw_turn_user_valid(h)) snprintf(c->turn_user, sizeof c->turn_user, "%.128s", h); continue; }
        if (sscanf(line, "enabled=%d", &v) == 1) c->enabled = v == 1;
        else if (sscanf(line, "port=%d", &v) == 1 && v >= 1024 && v < 65536) c->port = v;
        else if (sscanf(line, "allow=%31s", s) == 1) c->group_only = !strcmp(s, "group");
        else if (sscanf(line, "net=%31s", s) == 1 && rs_net_mode(s) >= 0) c->net = rs_net_mode(s);
        else if (sscanf(line, "insecure_dev=%d", &v) == 1) c->insecure_dev = v == 1;
    }
    if (f) fclose(f);
}

static int conf_write(const Conf *c) {
    mkdir("/etc/lordos", 0755);
    mkdir(CONF_DIR, 0755);
    char text[1600];
    int n = snprintf(text, sizeof text, "# LordOS · remote desktop (Settings → Sharing). See docs/REMOTE.md\nenabled=%d\nport=%d\nallow=%s\nnet=%s\ninsecure_dev=%d\n",
                     c->enabled, c->port, c->group_only ? "group" : "all", rs_net_name(c->net), c->insecure_dev);
    for (int i = 0; i < c->nstun; i++) n += snprintf(text + n, sizeof text - (size_t)n, "stun=%s\n", c->stun[i]);
    if (c->turn[0]) {
        n += snprintf(text + n, sizeof text - (size_t)n, "turn=%s\nturn_user=%s\nturn_key=%d\nturn_transport=%s\n", c->turn, c->turn_user, c->turn_key,
                      rw_turn_transport_name(c->turn_transport));
        if (c->turn_fp[0] && c->turn_transport == RW_TURN_TLS) n += snprintf(text + n, sizeof text - (size_t)n, "turn_fp=%s\n", c->turn_fp);
    }
    return write_file(CONF_DIR "/config", text, 0644);
}

/* the running lord-sessiond (the one from lordos.service): rereads the configuration on SIGHUP */
static pid_t sessiond_pid(void) {
    const char *const a[] = { "/usr/bin/systemctl", "show", "--property=MainPID", "--value", "lordos.service", NULL };
    char out[64] = "";
    if (admin_run_capture(a, out, sizeof out)) return -1;
    long pid = strtol(out, NULL, 10);
    if (pid <= 1) return -1;
    char link[64], exe[PATH_MAX];
    snprintf(link, sizeof link, "/proc/%ld/exe", pid);
    ssize_t l = readlink(link, exe, sizeof exe - 1);
    if (l <= 0) return -1;
    exe[l] = 0;
    /* after a package upgrade the running binary was replaced: the link ends in " (deleted)" */
    static const char deleted[] = " (deleted)";
    if ((size_t)l > sizeof deleted - 1 && !strcmp(exe + l - (sizeof deleted - 1), deleted)) exe[l - (sizeof deleted - 1)] = 0;
    const char *base = strrchr(exe, '/');
    return base && !strcmp(base + 1, "lord-sessiond") ? (pid_t)pid : -1;
}

static void set_enabled(int on) {
    pid_t pid = sessiond_pid();
    if (pid < 0) { admin_err("remote.needSessiond", NULL); return; }
    Conf c;
    conf_read(&c);
    c.enabled = on;
    if (conf_write(&c)) { admin_err("sys.writeFailed", "path", CONF_DIR "/config", NULL); return; }
    kill(pid, SIGHUP);
    if (!on) { admin_ok("remote.off", NULL); return; }
    /* waits for it to listen (status.json says running) */
    for (int i = 0; i < 40; i++) {
        char st[512] = "";
        int fd = open("/run/lordos/status.json", O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
        if (fd >= 0) { ssize_t n = read(fd, st, sizeof st - 1); if (n > 0) st[n] = 0; close(fd); }
        if (strstr(st, "\"running\":true")) { admin_ok("remote.on", NULL); return; }
        usleep(100000);
    }
    admin_err("remote.listenFailed", "port", lerr_int(c.port), NULL);
}

void remote_act_enable(const AdminReq *r, const char *arg, size_t len) {
    (void)r; (void)arg; (void)len;
    set_enabled(1);
}

void remote_act_disable(const AdminReq *r, const char *arg, size_t len) {
    (void)r; (void)arg; (void)len;
    set_enabled(0);
}

void remote_act_config(const AdminReq *r, const char *arg, size_t len) {
    (void)r; (void)len;
    Conf c;
    conf_read(&c);
    char buf[256];
    snprintf(buf, sizeof buf, "%s", arg);
    for (char *line = strtok(buf, "\n"); line; line = strtok(NULL, "\n")) {
        int v;
        char s[16];
        if (sscanf(line, "port=%d", &v) == 1) {
            if (v < 1024 || v > 65535) { admin_err("remote.badPort", NULL); return; }
            c.port = v;
        } else if (sscanf(line, "allow=%15s", s) == 1) {
            if (strcmp(s, "all") && strcmp(s, "group")) { admin_err("admin.invalidArgument", NULL); return; }
            c.group_only = !strcmp(s, "group");
        } else if (sscanf(line, "net=%15s", s) == 1) {
            if (rs_net_mode(s) < 0) { admin_err("admin.invalidArgument", NULL); return; }
            c.net = rs_net_mode(s);
        } else { admin_err("admin.invalidArgument", NULL); return; }
    }
    if (c.group_only && !getgrnam("lordos-remote")) {
        const char *const g[] = { "/usr/sbin/groupadd", "--system", "lordos-remote", NULL };
        if (admin_run(g)) { admin_err("remote.groupFailed", NULL); return; }
    }
    if (conf_write(&c)) { admin_err("remote.configFailed", NULL); return; }
    pid_t pid = sessiond_pid();
    if (pid > 0) kill(pid, SIGHUP);
    admin_ok("remote.saved", NULL);
}

void remote_act_member(const AdminReq *r, const char *arg, size_t len) {
    (void)r; (void)len;
    char user[40] = "";
    int on = -1;
    if (sscanf(arg, "%39[a-z0-9_-]\n%d", user, &on) != 2 || (on != 0 && on != 1)) { admin_err("admin.invalidArgument", NULL); return; }
    struct passwd *pw = getpwnam(user);
    if (!pw || pw->pw_uid < 1000 || pw->pw_uid >= 60000) { admin_err("users.invalid", NULL); return; }
    if (!getgrnam("lordos-remote")) {
        const char *const g[] = { "/usr/sbin/groupadd", "--system", "lordos-remote", NULL };
        if (admin_run(g)) { admin_err("remote.groupFailed", NULL); return; }
    }
    const char *const a[] = { "/usr/bin/gpasswd", on ? "-a" : "-d", user, "lordos-remote", NULL };
    if (admin_run(a)) admin_err("sys.programError", "program", "gpasswd", NULL); else admin_ok(on ? "remote.userAdded" : "remote.userRemoved", "name", user, NULL);
}

void remote_act_kick(const AdminReq *r, const char *arg, size_t len) {
    (void)r; (void)len;
    char *end;
    long pid = strtol(arg, &end, 10);
    if (pid <= 1 || *end) { admin_err("admin.invalidArgument", NULL); return; }
    /* only a connection that lord-sessiond (root) recorded in its list */
    char status[16384] = "", want[40];
    int fd = open("/run/lordos/status.json", O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    struct stat st;
    if (fd < 0 || fstat(fd, &st) || st.st_uid != 0) { if (fd >= 0) close(fd); admin_err("remote.notRunning", NULL); return; }
    ssize_t n = read(fd, status, sizeof status - 1);
    close(fd);
    if (n > 0) status[n] = 0;
    snprintf(want, sizeof want, "{\"pid\":%ld,", pid);
    char exe[PATH_MAX], link[64];
    snprintf(link, sizeof link, "/proc/%ld/exe", pid);
    ssize_t l = readlink(link, exe, sizeof exe - 1);
    if (l > 0) exe[l] = 0;
    const char *base = l > 0 ? strrchr(exe, '/') : NULL;
    if (!strstr(status, want) || !base || strcmp(base + 1, "lord-sessiond")) { admin_err("remote.noSuchConnection", NULL); return; }
    kill((pid_t)pid, SIGTERM);
    admin_ok("remote.disconnected", NULL);
}

/* ---- address lists (docs/REMOTE.md, "Blocked and allowed addresses") ---- */

/* One argument: an address or network, strictly (ipnet.c), nothing else on the line. */
static int one_cidr(const char *arg, size_t len, RsCidr *c, char canon[64]) {
    if (!len || len > 60 || memchr(arg, '\n', len) || memchr(arg, ' ', len)) return -1;
    char buf[64];
    memcpy(buf, arg, len);
    buf[len] = 0;
    if (rs_cidr_parse(buf, c)) return -1;
    rs_cidr_format(c, canon, 64);
    return 0;
}

static void access_read(RsAccess *acc) {
    char text[16384] = "";
    int fd = open(RD_ACCESS_PATH, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    if (fd >= 0) {
        struct stat st;
        if (!fstat(fd, &st) && S_ISREG(st.st_mode) && st.st_uid == 0) {
            ssize_t n = read(fd, text, sizeof text - 1);
            if (n > 0) text[n] = 0;
        }
        close(fd);
    }
    rs_access_parse(text, acc, NULL);
}

static int access_write(const RsAccess *acc) {
    char text[16384];
    if (rs_access_format(acc, text, sizeof text) < 0) return -1;
    mkdir("/etc/lordos", 0755);
    mkdir(CONF_DIR, 0755);
    return write_file(RD_ACCESS_PATH, text, 0600);
}

static void reload_sessiond(void) {
    pid_t pid = sessiond_pid();
    if (pid > 0) kill(pid, SIGHUP);
}

void remote_act_block(const AdminReq *r, const char *arg, size_t len) {
    (void)r;
    RsCidr c;
    char canon[64];
    if (one_cidr(arg, len, &c, canon)) { admin_err("remote.badAddress", NULL); return; }
    RsAccess acc;
    access_read(&acc);
    int k = rs_access_add(acc.block, &acc.nblock, &c);
    if (k < 0) { admin_err("remote.listFull", NULL); return; }
    if (k == 0 && access_write(&acc)) { admin_err("remote.configFailed", NULL); return; }
    reload_sessiond();
    admin_ok("remote.blocked", "address", canon, NULL);
}

void remote_act_unblock(const AdminReq *r, const char *arg, size_t len) {
    (void)r;
    RsCidr c;
    char canon[64];
    /* also an IPv6 /64 lockout key, which one_cidr accepts as a network */
    if (one_cidr(arg, len, &c, canon)) { admin_err("remote.badAddress", NULL); return; }
    RsAccess acc;
    access_read(&acc);
    if (rs_access_del(acc.block, &acc.nblock, &c) && access_write(&acc)) { admin_err("remote.configFailed", NULL); return; }
    /* the failed-login blocks belong to lord-sessiond: a request it reads on SIGHUP */
    mkdir("/var/lib/lordos", 0755);
    mkdir(RL_FILE_DIR, 0700);
    struct stat st;
    if (lstat(RL_FILE_DIR, &st) || !S_ISDIR(st.st_mode) || st.st_uid != 0) { admin_err("remote.configFailed", NULL); return; }
    int fd = open(RL_REQUESTS, O_WRONLY | O_APPEND | O_CREAT | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (fd < 0) { admin_err("remote.configFailed", NULL); return; }
    flock(fd, LOCK_EX);
    char line[96];
    int n = snprintf(line, sizeof line, "unblock %s\n", canon);
    int ok = write(fd, line, (size_t)n) == n;
    close(fd);
    if (!ok) { admin_err("remote.configFailed", NULL); return; }
    reload_sessiond();
    admin_ok("remote.unblocked", "address", canon, NULL);
}

void remote_act_allow(const AdminReq *r, const char *arg, size_t len) {
    (void)r;
    int add;
    if (len > 4 && !strncmp(arg, "add ", 4)) add = 1;
    else if (len > 4 && !strncmp(arg, "del ", 4)) add = 0;
    else { admin_err("admin.invalidArgument", NULL); return; }
    RsCidr c;
    char canon[64];
    if (one_cidr(arg + 4, len - 4, &c, canon)) { admin_err("remote.badAddress", NULL); return; }
    RsAccess acc;
    access_read(&acc);
    if (add) {
        int k = rs_access_add(acc.allow, &acc.nallow, &c);
        if (k < 0) { admin_err("remote.listFull", NULL); return; }
    } else rs_access_del(acc.allow, &acc.nallow, &c);
    if (access_write(&acc)) { admin_err("remote.configFailed", NULL); return; }
    reload_sessiond();
    admin_ok("remote.saved", NULL);
}

/* remote-failures: the recent failed sign-ins lord-sessiond keeps (Monitor → Network), read-only.
 * One LOG code line per row, re-validated here: remote.failure key= count= first= last= user=. The user is
 * what was typed in the name field, already cleaned to printable ASCII; passwords are never kept. */
void remote_act_failures(const AdminReq *r, const char *arg, size_t len) {
    (void)r; (void)arg;
    if (len) { admin_err("admin.invalidArgument", NULL); return; }
    char text[16384] = "";
    int fd = open("/run/lordos/remote-failures", O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    struct stat st;
    if (fd >= 0 && !fstat(fd, &st) && S_ISREG(st.st_mode) && st.st_uid == 0 && !(st.st_mode & 077)) {
        ssize_t n = read(fd, text, sizeof text - 1);
        if (n > 0) text[n] = 0;
    }
    if (fd >= 0) close(fd);
    int rows = 0;
    for (char *line = strtok(text, "\n"); line && rows < 64; line = strtok(NULL, "\n")) {
        char key[48], user[40] = "";
        int count;
        double first, last;
        int used = 0;
        RsCidr c;
        if (line[0] == '#' || sscanf(line, "%47s %d %lf %lf %n", key, &count, &first, &last, &used) != 4 || !used) continue;
        snprintf(user, sizeof user, "%.32s", line + used);
        int ok = !rs_key_parse(key, &c) && count > 0 && count < 1000000 && first > 0 && last >= first && user[0];
        for (const char *p = user; ok && *p; p++) if (*p < 0x20 || *p > 0x7e || *p == '"' || *p == '\\') ok = 0;
        if (!ok) continue;
        char f1[24], f2[24];
        snprintf(f1, sizeof f1, "%.0f", first);
        snprintf(f2, sizeof f2, "%.0f", last);
        admin_log("remote.failure", "key", key, "count", lerr_int(count), "first", f1, "last", f2, "user", user, NULL);
        rows++;
    }
    admin_ok("remote.failures", "count", lerr_int(rows), NULL);
}

/* remote-ice: the WebRTC connection's settings (Settings → Sharing → Connection). Every line is checked before
 * anything is written; the TURN password goes only to turn.key (root 0600), which lord-sessiond reads
 * for each video connection and hands to the bridge in memory. */
void remote_act_ice(const AdminReq *r, const char *arg, size_t len) {
    (void)r;
    if (len > 2048) { admin_err("admin.invalidArgument", NULL); return; }
    Conf c;
    conf_read(&c);
    char buf[2050], pass[130] = "";
    int set_turn = 0, set_pass = 0;
    snprintf(buf, sizeof buf, "%.*s", (int)len, arg);
    for (char *save = NULL, *line = strtok_r(buf, "\n", &save); line; line = strtok_r(NULL, "\n", &save)) {
        if (!strncmp(line, "stun=", 5)) {
            c.nstun = 0;
            char list[600];
            snprintf(list, sizeof list, "%s", line + 5);
            for (char *s2 = NULL, *one = strtok_r(list, ",", &s2); one; one = strtok_r(NULL, ",", &s2)) {
                if (c.nstun >= 4 || !rw_ice_server_valid(one)) { admin_err("remote.badIceServer", NULL); goto out; }
                snprintf(c.stun[c.nstun++], 96, "%.95s", one);
            }
        } else if (!strncmp(line, "turn=", 5)) {
            if (line[5] && !rw_ice_server_valid(line + 5)) { admin_err("remote.badIceServer", NULL); goto out; }
            snprintf(c.turn, sizeof c.turn, "%.95s", line + 5);
            set_turn = 1;
        } else if (!strncmp(line, "turn_transport=", 15)) {
            if (rw_turn_transport(line + 15) < 0) { admin_err("admin.invalidArgument", NULL); goto out; }
            c.turn_transport = rw_turn_transport(line + 15);
        } else if (!strncmp(line, "turn_fp=", 8)) {
            uint8_t fp[32];
            if (line[8] && rw_fp_parse(line + 8, fp)) { admin_err("remote.badTurnFingerprint", NULL); goto out; }
            snprintf(c.turn_fp, sizeof c.turn_fp, "%.95s", line + 8);
        } else if (!strncmp(line, "turn_user=", 10)) {
            if (!rw_turn_user_valid(line + 10)) { admin_err("remote.badTurnUser", NULL); goto out; }
            snprintf(c.turn_user, sizeof c.turn_user, "%.128s", line + 10);
        } else if (!strncmp(line, "turn_pass=", 10)) {
            if (!rw_turn_pass_valid(line + 10)) { admin_err("remote.badTurnPassword", NULL); goto out; }
            snprintf(pass, sizeof pass, "%s", line + 10);
            set_pass = 1;
        } else { admin_err("admin.invalidArgument", NULL); goto out; }
    }
    if (set_turn && !c.turn[0]) { c.turn_user[0] = c.turn_fp[0] = 0; c.turn_key = 0; c.turn_transport = RW_TURN_UDP; set_pass = 0; unlink(RD_TURN_KEY_PATH); }
    if (c.turn[0] && !c.turn_user[0]) { admin_err("remote.badTurnUser", NULL); goto out; }
    if (set_pass) {
        if (!c.turn[0]) { admin_err("admin.invalidArgument", NULL); goto out; }
        mkdir("/etc/lordos", 0755);
        mkdir(CONF_DIR, 0755);
        char line[140];
        snprintf(line, sizeof line, "%s\n", pass);
        int w = write_file(RD_TURN_KEY_PATH, line, 0600);
        explicit_bzero(line, sizeof line);
        if (w || chmod(RD_TURN_KEY_PATH, 0600)) { admin_err("remote.configFailed", NULL); goto out; }
        c.turn_key = 1;
    }
    if (conf_write(&c)) { admin_err("remote.configFailed", NULL); goto out; }
    pid_t pid = sessiond_pid();
    if (pid > 0) kill(pid, SIGHUP);
    admin_ok("remote.saved", NULL);
out:
    explicit_bzero(pass, sizeof pass);
    explicit_bzero(buf, sizeof buf);
}
