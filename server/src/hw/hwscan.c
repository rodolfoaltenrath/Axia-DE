/* hwscan.c — read-only hardware facts for the Store's suggestions (see hwscan.h). */
#include "hwscan.h"
#include "jbuf.h"

#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/utsname.h>
#include <unistd.h>

#define KMSG_MAX (4u << 20)

/* <root><path> into out. */
static void rpath(char *out, size_t size, const char *root, const char *path) {
    snprintf(out, size, "%s%s", root ? root : "", path);
}

/* A small sysfs/proc file: its first line, trimmed, printable ASCII only ('\0' inside a device
 * tree string ends it). "" when missing. */
static void read_attr(const char *path, char *out, size_t size) {
    out[0] = 0;
    int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return;
    char buf[256];
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) return;
    buf[n] = 0;
    size_t o = 0;
    for (ssize_t i = 0; i < n && buf[i] && buf[i] != '\n' && o + 1 < size; i++)
        out[o++] = (buf[i] >= 0x20 && buf[i] < 0x7f) ? buf[i] : '?';
    while (o && out[o - 1] == ' ') o--;
    out[o] = 0;
}

static void attr_at(const char *root, const char *dir, const char *name, char *out, size_t size) {
    char p[1024];
    snprintf(p, sizeof p, "%s%s/%s", root ? root : "", dir, name);
    read_attr(p, out, size);
}

/* A hex ID attribute ("0x10de" or "10de") as lowercase hex of `digits` digits; "" if not hex. */
static void hex_attr(const char *root, const char *dir, const char *name, int digits, char *out) {
    char v[32];
    attr_at(root, dir, name, v, sizeof v);
    const char *s = v;
    if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) s += 2;
    size_t n = strlen(s);
    out[0] = 0;
    if (!n || n > (size_t)digits) return;
    for (size_t i = 0; i < n; i++) if (!isxdigit((unsigned char)s[i])) return;
    size_t pad = (size_t)digits - n;
    for (size_t i = 0; i < pad; i++) out[i] = '0';
    for (size_t i = 0; i < n; i++) out[pad + i] = (char)tolower((unsigned char)s[i]);
    out[digits] = 0;
}

/* The driver bound to a device: the last component of its "driver" link. */
static void driver_of(const char *root, const char *dir, char *out, size_t size) {
    char p[1024], link[512];
    snprintf(p, sizeof p, "%s%s/driver", root ? root : "", dir);
    ssize_t n = readlink(p, link, sizeof link - 1);
    out[0] = 0;
    if (n <= 0) return;
    link[n] = 0;
    const char *b = strrchr(link, '/');
    b = b ? b + 1 : link;
    size_t o = 0;
    for (; *b && o + 1 < size; b++) if (isalnum((unsigned char)*b) || *b == '_' || *b == '-') out[o++] = *b;
    out[o] = 0;
}

static int skip_dirent(const struct dirent *e) { return e->d_name[0] == '.'; }

static int cmp_str(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }

/* The entries of a directory, sorted (so the JSON is stable). Caller frees each and the array. */
static char **list_dir(const char *path, int *count) {
    *count = 0;
    DIR *d = opendir(path);
    if (!d) return NULL;
    char **v = NULL;
    int n = 0, cap = 0;
    struct dirent *e;
    while ((e = readdir(d)) && n < 1024) {
        if (skip_dirent(e) || strlen(e->d_name) > 64) continue;
        if (n == cap) {
            int nc = cap ? cap * 2 : 32;
            char **nv = realloc(v, (size_t)nc * sizeof *v);
            if (!nv) break;
            v = nv;
            cap = nc;
        }
        if (!(v[n] = strdup(e->d_name))) break;
        n++;
    }
    closedir(d);
    if (v) qsort(v, (size_t)n, sizeof *v, cmp_str);
    *count = n;
    return v;
}

static void free_list(char **v, int n) {
    for (int i = 0; i < n; i++) free(v[i]);
    free(v);
}

/* ---- DMI and device tree ---- */

static void put_dmi(HBuf *b, const char *root) {
    static const struct { const char *key, *file; } f[] = {
        { "sysVendor", "sys_vendor" }, { "productName", "product_name" }, { "boardVendor", "board_vendor" },
        { "boardName", "board_name" }, { "chassisType", "chassis_type" },
    };
    hb_printf(b, "\"dmi\":{");
    for (size_t i = 0; i < sizeof f / sizeof f[0]; i++) {
        char v[96];
        attr_at(root, "/sys/class/dmi/id", f[i].file, v, sizeof v);
        hb_printf(b, "%s\"%s\":", i ? "," : "", f[i].key);
        hb_str(b, v);
    }
    hb_printf(b, "}");
}

static void put_devicetree(HBuf *b, const char *root) {
    char p[1024], model[128];
    rpath(p, sizeof p, root, "/proc/device-tree/model");
    read_attr(p, model, sizeof model);
    hb_printf(b, ",\"model\":");
    hb_str(b, model);
    /* compatible: NUL-separated strings */
    hb_printf(b, ",\"compatible\":[");
    rpath(p, sizeof p, root, "/proc/device-tree/compatible");
    int fd = open(p, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd >= 0) {
        char buf[512];
        ssize_t n = read(fd, buf, sizeof buf - 1);
        close(fd);
        int first = 1;
        for (ssize_t i = 0; n > 0 && i < n;) {
            char s[96];
            size_t o = 0;
            while (i < n && buf[i]) {
                char ch = buf[i++];
                if (o + 1 < sizeof s && (isalnum((unsigned char)ch) || strchr(",.-_+", ch))) s[o++] = ch;
            }
            i++;
            s[o] = 0;
            if (o) { hb_printf(b, "%s", first ? "" : ","); hb_str(b, s); first = 0; }
        }
    }
    hb_printf(b, "]");
}

/* ---- PCI ---- */

static void put_pci(HBuf *b, const char *root) {
    char p[1024];
    rpath(p, sizeof p, root, "/sys/bus/pci/devices");
    int n;
    char **v = list_dir(p, &n);
    hb_printf(b, ",\"pci\":[");
    int out = 0;
    for (int i = 0; i < n && out < 256; i++) {
        char dir[256], vendor[8], device[8], cls[8], drv[33];
        snprintf(dir, sizeof dir, "/sys/bus/pci/devices/%s", v[i]);
        hex_attr(root, dir, "vendor", 4, vendor);
        hex_attr(root, dir, "device", 4, device);
        hex_attr(root, dir, "class", 6, cls);
        if (!vendor[0] || !device[0]) continue;
        driver_of(root, dir, drv, sizeof drv);
        hb_printf(b, "%s{\"vendor\":\"%s\",\"device\":\"%s\",\"class\":\"%s\",\"driver\":", out++ ? "," : "", vendor, device, cls);
        hb_str(b, drv);
        hb_printf(b, "}");
    }
    hb_printf(b, "]");
    free_list(v, n);
}

/* ---- USB ---- */

/* udev's database entry for a character device ("189:3" → /run/udev/data/c189:3) has
 * "E:libsane_matched=yes" when libsane's udev rules know the device as a scanner. */
static int sane_matched(const char *root, const char *dir) {
    char dev[32];
    attr_at(root, dir, "dev", dev, sizeof dev);
    unsigned ma, mi;
    if (sscanf(dev, "%u:%u", &ma, &mi) != 2) return 0;
    char p[1024];
    snprintf(p, sizeof p, "%s/run/udev/data/c%u:%u", root ? root : "", ma, mi);
    int fd = open(p, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return 0;
    char buf[8192];
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) return 0;
    buf[n] = 0;
    for (char *l = buf; l && *l; l = strchr(l, '\n') ? strchr(l, '\n') + 1 : NULL)
        if (!strncmp(l, "E:libsane_matched=yes", 21) && (l[21] == '\n' || !l[21])) return 1;
    return 0;
}

static void put_usb(HBuf *b, const char *root) {
    char p[1024];
    rpath(p, sizeof p, root, "/sys/bus/usb/devices");
    int n;
    char **v = list_dir(p, &n);
    hb_printf(b, ",\"usb\":[");
    int out = 0;
    for (int i = 0; i < n && out < 256; i++) {
        if (strchr(v[i], ':')) continue;                     /* an interface, listed under its device */
        char dir[256], vendor[8], product[8], cls[4], drv[33];
        snprintf(dir, sizeof dir, "/sys/bus/usb/devices/%s", v[i]);
        hex_attr(root, dir, "idVendor", 4, vendor);
        hex_attr(root, dir, "idProduct", 4, product);
        if (!vendor[0] || !product[0] || !strcmp(vendor, "1d6b")) continue;   /* no IDs, or a root hub */
        hex_attr(root, dir, "bDeviceClass", 2, cls);
        hb_printf(b, "%s{\"vendor\":\"%s\",\"product\":\"%s\",\"class\":\"%s\",\"ifaces\":[", out++ ? "," : "", vendor, product, cls);
        /* its interfaces: "<device>:<config>.<n>"; the first bound driver names the device */
        drv[0] = 0;
        size_t dl = strlen(v[i]);
        int fi = 0;
        for (int k = 0; k < n; k++) {
            if (strncmp(v[k], v[i], dl) || v[k][dl] != ':') continue;
            char idir[256], ic[4], is[4], ip[4];
            snprintf(idir, sizeof idir, "/sys/bus/usb/devices/%s", v[k]);
            hex_attr(root, idir, "bInterfaceClass", 2, ic);
            hex_attr(root, idir, "bInterfaceSubClass", 2, is);
            hex_attr(root, idir, "bInterfaceProtocol", 2, ip);
            if (!ic[0]) continue;
            hb_printf(b, "%s\"%s%s%s\"", fi++ ? "," : "", ic, is[0] ? is : "00", ip[0] ? ip : "00");
            if (!drv[0]) driver_of(root, idir, drv, sizeof drv);
        }
        hb_printf(b, "],\"driver\":");
        hb_str(b, drv);
        hb_printf(b, ",\"sane\":%s}", sane_matched(root, dir) ? "true" : "false");
    }
    hb_printf(b, "]");
    free_list(v, n);
}

/* ---- virtio serial ports (a QEMU guest agent channel is named "org.qemu.guest_agent.0") ---- */

static void put_virtio(HBuf *b, const char *root) {
    char p[1024];
    rpath(p, sizeof p, root, "/sys/class/virtio-ports");
    int n;
    char **v = list_dir(p, &n);
    hb_printf(b, ",\"virtio\":[");
    int out = 0;
    for (int i = 0; i < n && out < 32; i++) {
        char dir[256], name[96];
        snprintf(dir, sizeof dir, "/sys/class/virtio-ports/%s", v[i]);
        attr_at(root, dir, "name", name, sizeof name);
        int ok = name[0] != 0;
        for (const char *s = name; *s && ok; s++) ok = isalnum((unsigned char)*s) || strchr("._-", *s);
        if (!ok) continue;
        hb_printf(b, "%s", out++ ? "," : "");
        hb_str(b, name);
    }
    hb_printf(b, "]");
    free_list(v, n);
}

/* ---- firmware misses ---- */

int hwscan_fw_name_valid(const char *s) {
    size_t n = strlen(s);
    if (!n || n >= sizeof ((HwFwMiss *)0)->file || s[0] == '/' || s[0] == '.' || strstr(s, "..") || strstr(s, "//")) return 0;
    for (; *s; s++) if (!isalnum((unsigned char)*s) && !strchr("._+,-/", *s)) return 0;
    return 1;
}

int hwscan_fw_present(const char *root, const char *name) {
    static const char *const dirs[] = { "/lib/firmware/updates/", "/lib/firmware/", "/usr/lib/firmware/" };
    static const char *const ext[] = { "", ".xz", ".zst" };
    if (!hwscan_fw_name_valid(name)) return 0;
    for (size_t d = 0; d < sizeof dirs / sizeof dirs[0]; d++)
        for (size_t e = 0; e < sizeof ext / sizeof ext[0]; e++) {
            char p[1024];
            struct stat st;
            snprintf(p, sizeof p, "%s%s%s%s", root ? root : "", dirs[d], name, ext[e]);
            if (!stat(p, &st) && S_ISREG(st.st_mode)) return 1;
        }
    return 0;
}

/* The driver name at the start of a message: "iwlwifi 0000:00:14.3: …" → "iwlwifi". */
static void msg_driver(const char *msg, const char *end, char *out, size_t size) {
    out[0] = 0;
    const char *sp = memchr(msg, ' ', (size_t)(end - msg));
    if (!sp || sp == msg || (size_t)(sp - msg) >= size) return;
    const char *sp2 = memchr(sp + 1, ' ', (size_t)(end - sp - 1));      /* "<driver> <device>: " */
    if (!sp2 || sp2 - sp < 3 || sp2[-1] != ':') return;
    for (const char *p = msg; p < sp; p++) if (!isalnum((unsigned char)*p) && *p != '_' && *p != '-') return;
    memcpy(out, msg, (size_t)(sp - msg));
    out[sp - msg] = 0;
}

int hwscan_fw_parse(const char *text, size_t len, HwFwMiss *out, int max) {
    static const struct { const char *pre, *post; } pat[] = {
        { "Direct firmware load for ", " failed with error " },
        { "firmware: failed to load ", " (" },
    };
    int count = 0;
    const char *end = text + len;
    for (const char *l = text; l < end && count < max;) {
        const char *nl = memchr(l, '\n', (size_t)(end - l));
        const char *le = nl ? nl : end;
        const char *msg = l;
        /* kmsg: "6,1234,5678,-;message"; continuation lines start with a space */
        if (*l == ' ') { l = nl ? nl + 1 : end; continue; }
        const char *semi = memchr(l, ';', (size_t)(le - l));
        if (semi && isdigit((unsigned char)*l)) msg = semi + 1;
        else if (*l == '[') {                                              /* dmesg: "[    5.1] message" */
            const char *rb = memchr(l, ']', (size_t)(le - l));
            if (rb) msg = rb + 1 + (rb + 1 < le && rb[1] == ' ');
        }
        for (size_t k = 0; k < sizeof pat / sizeof pat[0]; k++) {
            size_t pl = strlen(pat[k].pre);
            const char *a = NULL;
            for (const char *s = msg; s + pl <= le; s++) if (!memcmp(s, pat[k].pre, pl)) { a = s + pl; break; }
            if (!a) continue;
            const char *z = NULL, *post = pat[k].post;
            size_t ql = strlen(post);
            for (const char *s = a; s + ql <= le; s++) if (!memcmp(s, post, ql)) { z = s; break; }
            if (!z || z == a || (size_t)(z - a) >= sizeof out->file) break;
            HwFwMiss m;
            memcpy(m.file, a, (size_t)(z - a));
            m.file[z - a] = 0;
            if (!hwscan_fw_name_valid(m.file)) break;
            msg_driver(msg, le, m.driver, sizeof m.driver);
            int dup = 0;
            for (int i = 0; i < count && !dup; i++) dup = !strcmp(out[i].file, m.file);
            if (!dup) out[count++] = m;
            break;
        }
        l = nl ? nl + 1 : end;
    }
    return count;
}

char *hwscan_read_kmsg(const char *root, size_t *len, int *denied) {
    *len = 0;
    *denied = 0;
    char p[1024];
    int fixture = root && root[0];
    if (fixture) rpath(p, sizeof p, root, "/kmsg");
    else snprintf(p, sizeof p, "/dev/kmsg");
    int fd = open(p, O_RDONLY | O_CLOEXEC | O_NONBLOCK | (fixture ? O_NOFOLLOW : 0));
    if (fd < 0) { *denied = errno == EPERM || errno == EACCES; return NULL; }
    size_t cap = 65536, n = 0;
    char *buf = malloc(cap);
    if (!buf) { close(fd); return NULL; }
    for (;;) {
        if (cap - n < 8193) {
            if (cap >= KMSG_MAX) break;
            char *nb = realloc(buf, cap * 2);
            if (!nb) break;
            buf = nb;
            cap *= 2;
        }
        ssize_t r = read(fd, buf + n, cap - n - 1);
        if (r < 0 && errno == EINTR) continue;
        if (r < 0 && errno == EPIPE) continue;               /* a record was overwritten: go on */
        if (r < 0 && (errno == EPERM || errno == EACCES) && !n) { *denied = 1; free(buf); close(fd); return NULL; }
        if (r <= 0) break;                                   /* EAGAIN: end of the ring */
        n += (size_t)r;
    }
    close(fd);
    buf[n] = 0;
    *len = n;
    return buf;
}

int hwscan_fw_missing(const char *root, const char *text, size_t len, HwFwMiss *out, int max) {
    HwFwMiss all[HWSCAN_FW_MAX];
    int n = hwscan_fw_parse(text, len, all, max < HWSCAN_FW_MAX ? max : HWSCAN_FW_MAX), k = 0;
    for (int i = 0; i < n; i++) if (!hwscan_fw_present(root, all[i].file)) out[k++] = all[i];
    return k;
}

static void put_firmware(HBuf *b, const char *root) {
    size_t len;
    int denied;
    char *log = hwscan_read_kmsg(root, &len, &denied);
    hb_printf(b, ",\"firmware\":{\"readable\":%s,\"missing\":[", log ? "true" : "false");
    if (log) {
        HwFwMiss m[HWSCAN_FW_MAX];
        int n = hwscan_fw_missing(root, log, len, m, HWSCAN_FW_MAX);
        for (int i = 0; i < n; i++) {
            hb_printf(b, "%s{\"file\":", i ? "," : "");
            hb_str(b, m[i].file);
            hb_printf(b, ",\"driver\":");
            hb_str(b, m[i].driver);
            hb_printf(b, "}");
        }
        free(log);
    }
    hb_printf(b, "]}");
}

char *hwscan_json(const char *root) {
    HBuf b = { 0 };
    struct utsname u;
    if (uname(&u)) snprintf(u.machine, sizeof u.machine, "unknown");
    char arch[72];
    if (root && root[0]) {                                   /* a fixture may name its architecture */
        char p[1024];
        rpath(p, sizeof p, root, "/arch");
        read_attr(p, arch, sizeof arch);
        if (!arch[0]) snprintf(arch, sizeof arch, "%s", u.machine);
    } else snprintf(arch, sizeof arch, "%s", u.machine);
    hb_printf(&b, "{\"fixture\":%s,\"arch\":", root && root[0] ? "true" : "false");
    hb_str(&b, arch);
    hb_printf(&b, ",");
    put_dmi(&b, root);
    put_devicetree(&b, root);
    put_pci(&b, root);
    put_usb(&b, root);
    put_virtio(&b, root);
    put_firmware(&b, root);
    hb_printf(&b, "}");
    return hb_take(&b);
}
