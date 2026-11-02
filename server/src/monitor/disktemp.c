/* disktemp.c — which hwmon sensor belongs to which disk (see disktemp.h).
 *
 * Kernel layouts (all world-readable, no root):
 *   NVMe:      /sys/block/nvme0n1/device → the controller (class nvme), and its sensor is a class
 *              device directly under it: .../nvme0/hwmonN (name "nvme"). With native multipath the
 *              block device's `device` is the nvme-subsystem, which lists its controllers (nvme0 …).
 *   SATA:      /sys/block/sda/device → the SCSI device (a bus device), so the sensor sits in a glue
 *              folder: .../0:0:0:0/hwmon/hwmonN (name "drivetemp", from the drivetemp module).
 * temp1 is the drive's main ("Composite" on NVMe) temperature; temp1_max / temp1_crit are the
 * drive's own warning and critical limits when it reports them. */
#include "disktemp.h"

#include <ctype.h>
#include <dirent.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static long read_small(const char *path, char *out, size_t n) {
    int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return -1;
    ssize_t r = read(fd, out, n - 1);
    close(fd);
    if (r < 0) return -1;
    out[r] = 0;
    return r;
}

/* millidegrees in a file → whole °C within a sane range, or -1 */
static int read_limit(const char *dir, const char *file) {
    char p[PATH_MAX + 32], t[32];
    snprintf(p, sizeof p, "%s/%s", dir, file);
    if (read_small(p, t, sizeof t) <= 0) return -1;
    char *end;
    long v = strtol(t, &end, 10);
    if (end == t) return -1;
    v /= 1000;
    return v >= 30 && v <= 150 ? (int)v : -1;
}

static int is_hwmon_name(const char *n) {
    if (strncmp(n, "hwmon", 5) || !n[5]) return 0;
    for (const char *p = n + 5; *p; p++) if (!isdigit((unsigned char)*p)) return 0;
    return 1;
}

/* A hwmon folder whose chip is one we trust for a disk. */
static int try_chip(const char *dir, DiskSensor *s) {
    char p[PATH_MAX + 32], name[32];
    snprintf(p, sizeof p, "%s/name", dir);
    if (read_small(p, name, sizeof name) <= 0) return 0;
    name[strcspn(name, "\n")] = 0;
    int kind = !strcmp(name, "nvme") ? DT_NVME : !strcmp(name, "drivetemp") ? DT_DRIVETEMP : DT_NONE;
    if (!kind) return 0;
    struct stat st;
    snprintf(p, sizeof p, "%s/temp1_input", dir);
    size_t pl = strlen(p);
    if (pl >= sizeof s->input || stat(p, &st) || !S_ISREG(st.st_mode)) return 0;
    s->kind = kind;
    memcpy(s->input, p, pl + 1);
    s->max_c = read_limit(dir, "temp1_max");
    s->crit_c = read_limit(dir, "temp1_crit");
    if (s->max_c >= 0 && s->crit_c >= 0 && s->crit_c < s->max_c) s->crit_c = -1;   /* nonsense pair */
    return kind;
}

/* hwmonN directly in dev, or in dev/hwmon/ */
static int scan_dev(const char *dev, DiskSensor *s) {
    char sub[PATH_MAX];
    for (int pass = 0; pass < 2; pass++) {
        if (pass) snprintf(sub, sizeof sub, "%s/hwmon", dev); else snprintf(sub, sizeof sub, "%s", dev);
        DIR *d = opendir(sub);
        if (!d) continue;
        struct dirent *e;
        int found = 0;
        while (!found && (e = readdir(d))) {
            if (!is_hwmon_name(e->d_name)) continue;
            char p[PATH_MAX + 300];
            snprintf(p, sizeof p, "%s/%s", sub, e->d_name);
            if (strlen(p) < PATH_MAX) found = try_chip(p, s);
        }
        closedir(d);
        if (found) return found;
    }
    return 0;
}

static int valid_disk(const char *n) {
    size_t l = strlen(n);
    if (!l || l > 31 || !islower((unsigned char)n[0])) return 0;
    for (size_t i = 0; i < l; i++) if (!islower((unsigned char)n[i]) && !isdigit((unsigned char)n[i])) return 0;
    return 1;
}

int disktemp_find(const char *sys, const char *disk, DiskSensor *s) {
    memset(s, 0, sizeof *s);
    s->max_c = s->crit_c = -1;
    if (!sys || !valid_disk(disk)) return DT_NONE;
    char p[PATH_MAX], dev[PATH_MAX];
    snprintf(p, sizeof p, "%s/block/%s/device", sys, disk);
    if (!realpath(p, dev)) return DT_NONE;
    if (scan_dev(dev, s)) return s->kind;
    if (strncmp(disk, "nvme", 4)) return DT_NONE;
    /* native multipath: device is nvme-subsysN, whose entries nvme0, nvme1… are its controllers */
    DIR *d = opendir(dev);
    if (d) {
        struct dirent *e;
        while ((e = readdir(d))) {
            if (strncmp(e->d_name, "nvme", 4) || !isdigit((unsigned char)e->d_name[4])) continue;
            int digits = 1;
            for (const char *c = e->d_name + 4; *c; c++) if (!isdigit((unsigned char)*c)) digits = 0;
            if (!digits) continue;
            char q[PATH_MAX + 300], r[PATH_MAX];
            snprintf(q, sizeof q, "%s/%s", dev, e->d_name);
            if (strlen(q) < PATH_MAX && realpath(q, r) && scan_dev(r, s)) break;
        }
        closedir(d);
        if (s->kind) return s->kind;
    }
    /* last resort: nvmeXnY → the class device of controller nvmeX */
    char ctrl[32];
    size_t k = 4;
    while (isdigit((unsigned char)disk[k])) k++;
    if (k == 4 || disk[k] != 'n') return DT_NONE;
    snprintf(ctrl, sizeof ctrl, "%.*s", (int)k, disk);
    snprintf(p, sizeof p, "%s/class/nvme/%s", sys, ctrl);
    if (realpath(p, dev) && scan_dev(dev, s)) return s->kind;
    return DT_NONE;
}

int disktemp_read(const DiskSensor *s, double *celsius) {
    if (!s || !s->kind) return -1;
    char t[32];
    if (read_small(s->input, t, sizeof t) <= 0) return -1;
    char *end;
    long v = strtol(t, &end, 10);
    if (end == t) return -1;
    double c = v / 1000.0;
    if (c <= -40 || c >= 150) return -1;            /* a bogus reading is "no value", not a value */
    *celsius = c;
    return 0;
}

const char *disktemp_kind_name(int kind) {
    return kind == DT_NVME ? "nvme" : kind == DT_DRIVETEMP ? "drivetemp" : NULL;
}
