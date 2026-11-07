/* facts.c — the running session's facts for the Checks tab (see facts.h). */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include "facts.h"

#include <dirent.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static char *slurp(const char *root, const char *path, size_t max) {
    char p[1024];
    snprintf(p, sizeof p, "%s%s", root, path);
    int fd = open(p, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return NULL;
    char *b = malloc(max + 1);
    size_t got = 0;
    while (b && got < max) {
        ssize_t n = read(fd, b + got, max - got);
        if (n <= 0) break;
        got += (size_t)n;
    }
    close(fd);
    if (b) b[got] = 0;
    return b;
}

/* 1 root-owned, 0 not, -1 missing */
static int root_owned(const char *root, const char *path) {
    char p[1024];
    struct stat st;
    snprintf(p, sizeof p, "%s/sys/fs/cgroup%s", root, path);
    if (lstat(p, &st) || !S_ISDIR(st.st_mode)) return -1;
    return st.st_uid == 0;
}

void sr_facts(SrFacts *f, const char *root) {
    if (!root) root = "";
    memset(f, 0, sizeof *f);
    f->uid = f->euid = -1;
    f->leaf_root = f->parent_root = -1;
    char *st = slurp(root, "/proc/self/status", 16384);
    const char *u = st ? strstr(st, "\nUid:") : NULL;
    if (u) {
        int a, b;
        if (sscanf(u + 5, "%d %d", &a, &b) == 2) { f->uid = a; f->euid = b; }
    }
    free(st);

    /* "0::/system.slice/lordos.service/lordos-session-1000/server" (cgroup v2) */
    char *cg = slurp(root, "/proc/self/cgroup", 4096);
    const char *z = cg ? strstr(cg, "0::") : NULL;
    if (z && (z == cg || z[-1] == '\n')) {
        z += 3;
        size_t n = strcspn(z, "\n");
        if (n && n < sizeof f->cgroup && z[0] == '/' && !memmem(z, n, "..", 2)) {
            memcpy(f->cgroup, z, n);
            f->cgroup[n] = 0;
        }
    }
    free(cg);
    if (f->cgroup[0]) {
        f->leaf_root = root_owned(root, f->cgroup);
        char parent[256];
        snprintf(parent, sizeof parent, "%s", f->cgroup);
        char *sl = strrchr(parent, '/');
        if (sl && sl != parent) {
            *sl = 0;
            f->parent_root = root_owned(root, parent);
            char p[1024];
            snprintf(p, sizeof p, "%s/sys/fs/cgroup%s", root, parent);
            DIR *d = opendir(p);
            struct dirent *e;
            while (d && (e = readdir(d))) {
                size_t l = strlen(e->d_name);
                if (!strncmp(e->d_name, "app-", 4) && l > 10 && !strcmp(e->d_name + l - 6, ".scope")) f->app_scopes++;
            }
            if (d) closedir(d);
        }
    }
}
