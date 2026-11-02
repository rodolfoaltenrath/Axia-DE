/* disk_cli.c — runs a disk-* action like the helper does, for manual testing on the VM (not part of
 * `make test`: it needs root and real devices). See docs/SYSTEM.md, "Disks".
 *
 *   build/disk-cli list                         the list lord.disks.list() sees
 *   build/disk-cli system <disk>                is it the system disk?
 *   sudo build/disk-cli [-u user] [-n] <action> '<arg with \n>'
 *        -n: as a user who is not an administrator
 *
 * The LordOS Imager's imager-* actions too (docs/IMAGER.md), with the real release key.
 *
 * admin_say/admin_run are minimal versions of the ones in admin.c (they print to stdout). */
#include "disk/disk.h"
#include "imager/imager_admin.h"

#include <errno.h>
#include <fcntl.h>
#include <pwd.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/wait.h>
#include <unistd.h>

void admin_say(const char *kind, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    printf("%s ", kind);
    vprintf(fmt, ap);
    printf("\n");
    fflush(stdout);
    va_end(ap);
}
void admin_event(const char *fmt, ...) { (void)fmt; }

static int run(const char *const argv[], char *out, size_t cap) {
    int p[2];
    if (pipe(p)) return -1;
    pid_t pid = fork();
    if (pid == 0) {
        dup2(p[1], 1);
        dup2(p[1], 2);
        close(p[0]);
        char *const env[] = { "PATH=/usr/sbin:/usr/bin:/sbin:/bin", "LANG=C.UTF-8", NULL };
        execve(argv[0], (char *const *)argv, env);
        _exit(127);
    }
    close(p[1]);
    size_t n = 0;
    char buf[512];
    for (ssize_t r; (r = read(p[0], buf, sizeof buf)) > 0;) {
        if (out && n + (size_t)r < cap) { memcpy(out + n, buf, (size_t)r); n += (size_t)r; }
        else if (!out) fwrite(buf, 1, (size_t)r, stdout);
    }
    if (out) out[n] = 0;
    close(p[0]);
    int st = 0;
    waitpid(pid, &st, 0);
    return WIFEXITED(st) ? WEXITSTATUS(st) : 128;
}
int admin_run(const char *const argv[]) { return run(argv, NULL, 0); }
int admin_run_input(const char *const argv[], const char *input) { (void)input; return run(argv, NULL, 0); }
int admin_run_capture(const char *const argv[], char *out, size_t cap) { return run(argv, out, cap); }

static void unescape(char *s) {
    char *o = s;
    for (char *p = s; *p; p++) {
        if (p[0] == '\\' && p[1] == 'n') { *o++ = '\n'; p++; }
        else *o++ = *p;
    }
    *o = 0;
}

int main(int argc, char **argv) {
    prctl(PR_SET_NAME, "lord-admin");      /* like the helper (disk-cancel checks it) */
    const char *user = getenv("SUDO_USER");
    int admin = 1, i = 1;
    for (; i < argc && argv[i][0] == '-'; i++) {
        if (!strcmp(argv[i], "-u") && i + 1 < argc) user = argv[++i];
        else if (!strcmp(argv[i], "-n")) admin = 0;
    }
    if (i >= argc) { fprintf(stderr, "usage: disk-cli list | system <disk> | [-u user] [-n] <action> [arg]\n"); return 2; }
    if (!strcmp(argv[i], "list")) { char *j = disk_list_json(); puts(j ? j : "null"); free(j); return 0; }
    if (!strcmp(argv[i], "system") && i + 1 < argc) { printf("%d\n", disk_is_system(argv[i + 1])); return 0; }
    struct passwd *pw = getpwnam(user ? user : "root");
    if (!pw) { fprintf(stderr, "unknown user\n"); return 2; }
    AdminReq r = { .id = 1, .user = pw->pw_name, .home = pw->pw_dir, .uid = pw->pw_uid, .gid = pw->pw_gid, .admin = admin };
    char *arg = strdup(i + 1 < argc ? argv[i + 1] : "");
    unescape(arg);
    static const struct { const char *n; void (*fn)(const AdminReq *, const char *, size_t); } acts[] = {
        { "disk-smart", disk_act_smart }, { "disk-mount", disk_act_mount }, { "disk-unmount", disk_act_unmount },
        { "disk-eject", disk_act_eject }, { "disk-format", disk_act_format }, { "disk-label", disk_act_label },
        { "disk-erase", disk_act_erase }, { "disk-write-image", disk_act_write_image }, { "disk-cancel", disk_act_cancel },
        { "smart-info", smart_act_info }, { "smart-health", smart_act_health }, { "smart-test", smart_act_test }, { "smart-abort", smart_act_abort },
        { "imager-fetch", imager_act_fetch }, { "imager-fetch-cancel", imager_act_fetch_cancel },
        { "imager-cache-clear", imager_act_cache_clear }, { "imager-write", imager_act_write },
    };
    for (size_t k = 0; k < sizeof acts / sizeof *acts; k++)
        if (!strcmp(acts[k].n, argv[i])) { acts[k].fn(&r, arg, strlen(arg)); free(arg); return 0; }
    fprintf(stderr, "unknown action\n");
    return 2;
}
