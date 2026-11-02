/* secretsvc_demo.c — our own Secret Service with an in-memory keyring, on the real session bus
 * (no sandbox). To test with secret-tool/libsecret: tests/secretsvc/run-pw-116.sh.
 *   build/secretsvc-demo [--locked] [--seconds N]
 * With --locked it starts locked: Unlock shows a "prompt" that accepts itself after one second.
 * Exits with 0 when done, 3 if org.freedesktop.secrets already had an owner. */
#include "vault/dbus.h"
#include "vault/secretsvc.h"
#include "ss_memback.h"

#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static volatile sig_atomic_t stop;
static void on_sig(int s) { (void)s; stop = 1; }
static MemBack mb;

int main(int argc, char **argv) {
    int locked = 0, seconds = 120;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--locked")) locked = 1;
        else if (!strcmp(argv[i], "--seconds") && i + 1 < argc) seconds = atoi(argv[++i]);
    }
    signal(SIGTERM, on_sig);
    signal(SIGINT, on_sig);
    setenv("LORD_SS_DEBUG", "1", 0);
    memback_init(&mb, locked);
    int fd = dbus_connect_session();
    if (fd < 0) { fprintf(stderr, "demo: no session bus\n"); return 1; }
    int r = ss_start(fd, &memback_ops, &mb);
    if (r == 1) { fprintf(stderr, "demo: org.freedesktop.secrets already has an owner\n"); return 3; }
    if (r) { fprintf(stderr, "demo: could not start\n"); return 1; }
    fprintf(stderr, "demo: ready as %s (%s)\n", ss_unique_name(), locked ? "locked" : "unlocked");
    time_t end = time(NULL) + seconds;
    while (!stop && time(NULL) < end) {
        struct pollfd p = { ss_fd(), POLLIN | (ss_pending_out() ? POLLOUT : 0), 0 };
        poll(&p, 1, 100);
        if (ss_dispatch() < 0) { fprintf(stderr, "demo: the bus closed\n"); return 1; }
        memback_tick(&mb);
    }
    ss_stop();
    fprintf(stderr, "demo: done\n");
    return 0;
}
