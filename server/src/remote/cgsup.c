/* cgsup.c — lord-sessiond's cgroup placement service (see sec/cgroup.h, docs/PRIVACY.md).
 *
 * A small root child of lord-sessiond listens on /run/lordos/cgroup.sock. lord-server (and the
 * children it forks right before exec: Linux apps, .lpk apps, lord-media, lord-doc…) ask it to be
 * placed into their app scope. The scopes and their cgroup.procs are root-owned, so a process of
 * the user can never move itself out of its scope. The child runs serially with short timeouts
 * and dies with its parent (PR_SET_PDEATHSIG): a lord-sessiond restart starts a fresh one. */
#include "sessiond.h"
#include "../sec/cgroup.h"

#include <errno.h>
#include <limits.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

static pid_t cg_pid = -1;
static double cg_restart_at;

static void serve(void) {
    syscall(SYS_close_range, 3, ~0u, 0);        /* no monitor fds, no worker sockets */
    prctl(PR_SET_PDEATHSIG, SIGTERM);
    signal(SIGTERM, SIG_DFL);
    signal(SIGHUP, SIG_DFL);
    if (getppid() == 1) _exit(0);
    char bindir[512];
    snprintf(bindir, sizeof bindir, "%s", rd_conf.server);
    char *slash = strrchr(bindir, '/');
    if (!slash) _exit(1);
    *slash = 0;
    char real[PATH_MAX];
    if (realpath(bindir, real) && strlen(real) < sizeof bindir) memcpy(bindir, real, strlen(real) + 1);
    int lfd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
    struct sockaddr_un a = { .sun_family = AF_UNIX };
    snprintf(a.sun_path, sizeof a.sun_path, "%s", CGROUP_SOCK);
    unlink(CGROUP_SOCK);
    mode_t old = umask(0);
    int ok = lfd >= 0 && !bind(lfd, (struct sockaddr *)&a, sizeof a) && !listen(lfd, 32);
    umask(old);
    if (!ok) { sd_log("cgroup: could not listen on %s: %s", CGROUP_SOCK, strerror(errno)); _exit(1); }
    chmod(CGROUP_SOCK, 0666);                   /* anyone may ask; SO_PEERCRED decides */
    sd_log("cgroup: root-owned scopes (service on %s, binaries from %s)", CGROUP_SOCK, bindir);
    for (;;) {
        int c = accept4(lfd, NULL, NULL, SOCK_CLOEXEC);
        if (c < 0) { if (errno == EINTR) continue; _exit(1); }
        cgroup_place_serve(c, bindir, rd_conf.insecure_dev);
        close(c);
    }
}

void sd_cgroup_start(void) {
    pid_t p = fork();
    if (p == 0) { serve(); _exit(0); }
    cg_pid = p;
}

int sd_cgroup_reaped(pid_t pid, int status) {
    (void)status;
    if (pid != cg_pid) return 0;
    cg_pid = -1;
    cg_restart_at = rd_now() + 1;
    return 1;
}

void sd_cgroup_tick(void) {
    if (cg_pid < 0 && cg_restart_at > 0 && rd_now() >= cg_restart_at) { cg_restart_at = 0; sd_cgroup_start(); }
}
