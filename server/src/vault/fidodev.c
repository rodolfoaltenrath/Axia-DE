/* fidodev.c — FIDO2 keys over hidraw (see fidodev.h). */
#include "fidodev.h"

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static const char *sysroot = "/sys/class/hidraw";

void fidodev_set_sysroot(const char *dir) { sysroot = dir ? dir : "/sys/class/hidraw"; }

/* Short items of the HID descriptor: prefix (tag 4 bits, type 2, size 2) + 0/1/2/4 bytes;
 * long item 0xFE: size, tag, data. We track Usage Page (global, with push/pop), Usage
 * (local, 16 or 32 bits) and Collection (main). */
int fidodev_desc_is_fido(const uint8_t *d, size_t len) {
    uint32_t page = 0, stack[8];
    int sp = 0;
    uint32_t usage[16];
    int nusage = 0;
    size_t p = 0;
    while (p < len) {
        uint8_t b = d[p++];
        if (b == 0xfe) {                                  /* long item */
            if (p + 2 > len) return 0;
            size_t sz = d[p];
            p += 2 + sz;
            continue;
        }
        size_t sz = (b & 3) == 3 ? 4 : (b & 3);
        if (p + sz > len) return 0;
        uint32_t v = 0;
        for (size_t i = 0; i < sz; i++) v |= (uint32_t)d[p + i] << (8 * i);
        p += sz;
        int type = (b >> 2) & 3, tag = b >> 4;
        if (type == 1) {                                  /* global */
            if (tag == 0) page = v;
            else if (tag == 10 && sp < 8) stack[sp++] = page;      /* push */
            else if (tag == 11 && sp > 0) page = stack[--sp];      /* pop */
        } else if (type == 2) {                           /* local */
            if (tag == 0 && nusage < 16) usage[nusage++] = sz == 4 ? v : (page << 16 | v);
        } else if (type == 0) {                           /* main */
            if (tag == 10 && v == 1) {                    /* Collection (Application) */
                for (int i = 0; i < nusage; i++) if (usage[i] == 0xf1d00001u) return 1;
            }
            nusage = 0;                                   /* locals last until the next main */
        }
    }
    return 0;
}

static int read_file(const char *path, uint8_t *buf, size_t cap) {
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    size_t n = 0;
    while (n < cap) {
        ssize_t r = read(fd, buf + n, cap - n);
        if (r <= 0) break;
        n += (size_t)r;
    }
    close(fd);
    return (int)n;
}

static int valid_name(const char *n) {
    if (strncmp(n, "hidraw", 6) || !n[6] || strlen(n) > 12) return 0;
    for (const char *p = n + 6; *p; p++) if (*p < '0' || *p > '9') return 0;
    return 1;
}

static int is_fido(const char *name) {
    char path[512];
    uint8_t desc[4096];
    snprintf(path, sizeof path, "%.400s/%.12s/device/report_descriptor", sysroot, name);
    int n = read_file(path, desc, sizeof desc);
    return n > 0 && fidodev_desc_is_fido(desc, (size_t)n);
}

int fidodev_list(FidoDev *out, int max) {
    DIR *dir = opendir(sysroot);
    if (!dir) return 0;
    int count = 0;
    struct dirent *e;
    while (count < max && (e = readdir(dir))) {
        if (!valid_name(e->d_name) || !is_fido(e->d_name)) continue;
        FidoDev *f = &out[count];
        memset(f, 0, sizeof *f);
        snprintf(f->path, sizeof f->path, "/dev/%.12s", e->d_name);
        char path[512], ue[2048];
        snprintf(path, sizeof path, "%.400s/%.12s/device/uevent", sysroot, e->d_name);
        int n = read_file(path, (uint8_t *)ue, sizeof ue - 1);
        if (n > 0) {
            ue[n] = 0;
            for (char *line = strtok(ue, "\n"); line; line = strtok(NULL, "\n")) {
                if (!strncmp(line, "HID_NAME=", 9)) snprintf(f->name, sizeof f->name, "%s", line + 9);
                else if (!strncmp(line, "HID_ID=", 7)) {
                    unsigned bus, v, p;
                    if (sscanf(line + 7, "%x:%x:%x", &bus, &v, &p) == 3) { f->vid = v; f->pid = p; }
                }
            }
        }
        if (!f->name[0]) snprintf(f->name, sizeof f->name, "%.12s", e->d_name);
        count++;
    }
    closedir(dir);
    return count;
}

int fidodev_open(const char *path) {
    if (!path || strncmp(path, "/dev/", 5) || !valid_name(path + 5)) { errno = EINVAL; return -1; }
    if (!is_fido(path + 5)) { errno = ENODEV; return -1; }
    int fd = open(path, O_RDWR | O_CLOEXEC | O_NONBLOCK | O_NOFOLLOW);
    if (fd < 0) return -1;
    struct stat st;
    if (fstat(fd, &st) || !S_ISCHR(st.st_mode)) { close(fd); errno = ENODEV; return -1; }
    return fd;
}
