/* imager_admin.c — the LordOS Imager's actions in the privileged helper (admin.c). See
 * docs/IMAGER.md, "Security model".
 *
 *   imager-fetch meta                                the release list (GitHub API) into the cache
 *   imager-fetch file\n<tag>\n<name>\n<size>         one release file into the cache, resumable
 *   imager-fetch-cancel                              stops this user's download
 *   imager-cache-clear                               deletes the imager's cache of this user
 *   imager-write <disk>\n<size>\n<model>\n<serial>\nofficial|custom\n<image>[\n<SHA256SUMS>\n<SHA256SUMS.sig>]
 *                                                    writes a verified image to a whole removable disk
 *                                                    (cancel: disk-cancel <disk>)
 *
 * Downloads: only two fixed URL shapes (the release list of lordbasex/LordOS, and a file of one of
 * its releases), fetched by /usr/bin/curl running as the session user (setresuid, no new privileges,
 * no curlrc, https only, also on redirects) into ~/.cache/lordos/imager. Root never parses what
 * arrives; the UI reads the list as the user, and the image is verified by imager-write.
 *
 * Writing, all checked here (the UI is not trusted):
 *   - a whole disk, never the system disk, and only a removable one (USB, card): this action
 *     refuses internal disks even for an administrator;
 *   - its size, model and serial must be the ones the user picked; re-checked right before the
 *     device is opened, and the opened descriptor must be that very device (major:minor);
 *   - nothing of it may be mounted outside /media or /run/media (those mounts are unmounted), and
 *     it is opened O_EXCL, so the kernel refuses a disk still in use;
 *   - the image is opened with the user's permissions, from their home or their mounts only, never
 *     from the disk being written; its uncompressed size must fit;
 *   - official: SHA256SUMS.sig must verify with the release key compiled into LordOS
 *     (codesig_release_key, the key of docs/SIGNING.md) and the image's SHA-256 must be the listed
 *     one, BEFORE anything is erased; the bytes are hashed again while writing and the disk is
 *     wiped if they changed in between;
 *   - .img.xz is decompressed by /usr/bin/xz running as the user with no privileges (stdin and
 *     stdout pipes only): root never decodes the compressed data;
 *   - after writing, the disk is read back (O_DIRECT) and its SHA-256 compared with what was written;
 *   - on a failure or a cancel after the first write, the start and end of the disk are wiped, so a
 *     half-written card never looks bootable. */
#include "../admin.h"
#include "../disk/disk.h"
#include "../pkg/crypto.h"
#include "../sec/codesig.h"
#include "imgverify.h"

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <limits.h>
#include <linux/fs.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/fsuid.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include "imager_admin.h"

#define CURL "/usr/bin/curl"
#define XZ   "/usr/bin/xz"
#define REPO "lordbasex/LordOS"
#define MIB (1024u * 1024u)
#define MAX_IMAGE (64ull << 30)          /* 64 GiB: far above any LordOS image */

/* snprintf for paths and short lines; truncation (never expected: the parts are validated and
 * short) leaves a shorter string that the following open/stat simply fails on. */
static void pathf(char *out, size_t n, const char *fmt, ...) __attribute__((format(printf, 3, 4)));
static void pathf(char *out, size_t n, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(out, n, fmt, ap);
    va_end(ap);
}

static double now_s(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + t.tv_nsec / 1e9;
}

/* Splits arg into at most max lines (no empty argument, no control characters but \n). */
static int lines(char *buf, char **f, int max) {
    int n = 0;
    for (char *s = buf; s && n < max;) {
        f[n++] = s;
        char *nl = strchr(s, '\n');
        if (nl) *nl = 0;
        s = nl ? nl + 1 : NULL;
        if (s && n == max) return -1;
    }
    for (int i = 0; i < n; i++)
        for (const unsigned char *p = (const unsigned char *)f[i]; *p; p++) if (*p < 0x20 || *p == 0x7f) return -1;
    return n;
}

static void as_user(const AdminReq *r) { setfsgid(r->gid); setfsuid(r->uid); }
static void as_root(void) { setfsuid(0); setfsgid(0); }

/* ~/.cache/lordos/imager[/<tag>] created as the user; returns 0 and the path. */
static int cache_dir(const AdminReq *r, const char *tag, char *out, size_t n) {
    char p[PATH_MAX];
    static const char *const parts[] = { "/.cache", "/.cache/lordos", "/.cache/lordos/imager" };
    as_user(r);
    int ok = 1;
    for (int i = 0; i < 3 && ok; i++) {
        pathf(p, sizeof p, "%s%s", r->home, parts[i]);
        if (mkdir(p, 0755) && errno != EEXIST) ok = 0;
    }
    if (ok && tag) {
        pathf(p, sizeof p, "%s/.cache/lordos/imager/%s", r->home, tag);
        if (mkdir(p, 0755) && errno != EEXIST) ok = 0;
    }
    struct stat st;
    if (ok && (lstat(p, &st) || !S_ISDIR(st.st_mode) || st.st_uid != r->uid)) ok = 0;   /* no symlinked cache */
    as_root();
    if (!ok) return -1;
    pathf(out, n, "%s", p);
    return 0;
}

/* A child that becomes the session user for good (groups, gid, uid, no new privileges). */
static int drop_to_user(const AdminReq *r) {
    if (initgroups(r->user, r->gid) && setgroups(0, NULL)) return -1;
    if (setresgid(r->gid, r->gid, r->gid) || setresuid(r->uid, r->uid, r->uid)) return -1;
    if (getuid() == 0 || geteuid() == 0) return -1;
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)) return -1;
    prctl(PR_SET_PDEATHSIG, SIGTERM);
    return 0;
}

/* ---- downloads ---- */

static volatile sig_atomic_t fetch_stop;
static void on_fetch_stop(int s) { (void)s; fetch_stop = 1; }

static void fetch_job(const AdminReq *r, char *p, size_t n) { pathf(p, n, "/run/lordos/imager-fetch-%u.job", (unsigned)r->uid); }

void imager_act_fetch(const AdminReq *r, const char *arg, size_t len) {
    char *buf = strndup(arg, len), *f[4];
    int nf = buf ? lines(buf, f, 4) : -1;
    char url[512], dir[PATH_MAX], dest[PATH_MAX], part[PATH_MAX];
    uint64_t want = 0, max;
    int meta = nf == 1 && !strcmp(f[0], "meta");
    if (meta) {
        pathf(url, sizeof url, "https://api.github.com/repos/" REPO "/releases?per_page=10");
        max = 4 * MIB;
        if (cache_dir(r, NULL, dir, sizeof dir)) { free(buf); admin_err("imager.cacheFailed", NULL); return; }
        pathf(dest, sizeof dest, "%s/releases.json", dir);
    } else if (nf == 4 && !strcmp(f[0], "file")) {
        char *end;
        errno = 0;
        unsigned long long sz = strtoull(f[3], &end, 10);
        if (!img_valid_tag(f[1]) || !img_valid_name(f[2]) || errno || *end || !f[3][0] || sz == 0 || sz > MAX_IMAGE ||
            !(img_kind(f[2]) || !strcmp(f[2], "SHA256SUMS") || !strcmp(f[2], "SHA256SUMS.sig"))) {
            free(buf);
            admin_err("admin.invalidArgument", NULL);
            return;
        }
        want = sz;
        max = sz;
        pathf(url, sizeof url, "https://github.com/" REPO "/releases/download/%s/%s", f[1], f[2]);
        if (cache_dir(r, f[1], dir, sizeof dir)) { free(buf); admin_err("imager.cacheFailed", NULL); return; }
        pathf(dest, sizeof dest, "%s/%s", dir, f[2]);
    } else {
        free(buf);
        admin_err("admin.invalidArgument", NULL);
        return;
    }
    free(buf);
    if (access(CURL, X_OK)) { admin_err("imager.noCurl", NULL); return; }
    pathf(part, sizeof part, "%s.part", dest);
    struct stat st;
    as_user(r);
    int have = !lstat(dest, &st) && S_ISREG(st.st_mode) && want && (uint64_t)st.st_size == want;
    if (meta) unlink(part);                                  /* the list is never resumed */
    as_root();
    if (have) { admin_ok("imager.cached", "size", lerr_int((long long)want), NULL); return; }

    char jp[96];
    fetch_job(r, jp, sizeof jp);
    mkdir("/run/lordos", 0755);
    FILE *jf = fopen(jp, "we");
    if (jf) { fprintf(jf, "%d %u\n", (int)getpid(), (unsigned)r->uid); fclose(jf); }
    struct sigaction sa = { .sa_handler = on_fetch_stop };
    sigaction(SIGTERM, &sa, NULL);

    char maxs[24];
    pathf(maxs, sizeof maxs, "%llu", (unsigned long long)max);
    pid_t pid = fork();
    if (pid == 0) {
        int null = open("/dev/null", O_RDWR);
        if (null >= 0) { dup2(null, 0); dup2(null, 1); }
        int errp = open("/dev/null", O_WRONLY);
        if (errp >= 0) dup2(errp, 2);
        syscall(SYS_close_range, 3, ~0u, 0);
        if (drop_to_user(r) || chdir(dir)) _exit(126);
        char *const env[] = { "PATH=/usr/bin:/bin", "LANG=C.UTF-8", NULL };
        /* -q first: no ~/.curlrc. https only, also after a redirect (GitHub sends the file from
         * its own download host). -C -: resume the .part file. */
        const char *const resume[] = { CURL, "-q", "-sS", "-f", "-L", "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "5",
                                       "--connect-timeout", "20", "--speed-limit", "1024", "--speed-time", "60", "--max-filesize", maxs,
                                       "-C", "-", "-o", part, "--", url, NULL };
        const char *const fresh[] = { CURL, "-q", "-sS", "-f", "-L", "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "5",
                                      "--connect-timeout", "20", "--speed-limit", "1024", "--speed-time", "60", "--max-filesize", maxs,
                                      "-H", "Accept: application/vnd.github+json", "-o", part, "--", url, NULL };
        execve(CURL, (char *const *)(meta ? fresh : resume), env);
        _exit(127);
    }
    int code = -1;
    double last = 0;
    while (pid > 0) {
        int stc;
        pid_t w = waitpid(pid, &stc, WNOHANG);
        if (w == pid) { code = WIFEXITED(stc) ? WEXITSTATUS(stc) : 128; break; }
        if (w < 0 && errno != EINTR) break;
        if (fetch_stop) { kill(pid, SIGTERM); waitpid(pid, &stc, 0); code = -2; break; }
        if (now_s() - last >= 0.5) {
            last = now_s();
            as_user(r);
            uint64_t done = !stat(part, &st) ? (uint64_t)st.st_size : 0;
            as_root();
            admin_say("LOG", "progress phase=download done=%llu total=%llu", (unsigned long long)done, (unsigned long long)want);   /* i18n-ok */
        }
        usleep(100000);
    }
    unlink(jp);
    int ok = 0;
    as_user(r);
    if (code == 0 && !lstat(part, &st) && S_ISREG(st.st_mode) && (!want || (uint64_t)st.st_size == want) && (uint64_t)st.st_size <= max)
        ok = rename(part, dest) == 0;
    else if (code == 0) unlink(part);                         /* wrong size: not something we asked for */
    as_root();
    if (code == -2) admin_err("imager.fetchCancelled", NULL);
    else if (code == 127 || code == 126) admin_err("imager.noCurl", NULL);
    else if (code == 63) admin_err("imager.fetchTooLarge", NULL);
    else if (code == 22) admin_err("imager.fetchNotFound", NULL);
    else if (code == 6 || code == 7 || code == 28 || code == 35 || code == 56) admin_err("imager.fetchNetwork", "code", lerr_int(code), NULL);
    else if (code != 0 || !ok) admin_err("imager.fetchFailed", "code", lerr_int(code), NULL);
    else admin_ok("imager.fetched", "size", lerr_int((long long)st.st_size), NULL);
}

void imager_act_fetch_cancel(const AdminReq *r, const char *arg, size_t len) {
    (void)arg; (void)len;
    char jp[96];
    fetch_job(r, jp, sizeof jp);
    FILE *f = fopen(jp, "re");
    int pid = 0;
    unsigned uid = 0;
    int ok = f && fscanf(f, "%d %u", &pid, &uid) == 2;
    if (f) fclose(f);
    if (!ok || pid <= 1 || uid != r->uid) { admin_err("imager.nothingRunning", NULL); return; }
    char cp[64], comm[32] = "";
    pathf(cp, sizeof cp, "/proc/%d/comm", pid);
    FILE *c = fopen(cp, "re");
    if (c) { if (!fgets(comm, sizeof comm, c)) comm[0] = 0; fclose(c); }
    if (strncmp(comm, "lord-admin", 10)) { unlink(jp); admin_err("imager.nothingRunning", NULL); return; }
    kill(pid, SIGTERM);
    admin_ok("imager.cancelling", NULL);
}

/* Deletes the files the imager put in the cache (as the user; one level of release folders). */
void imager_act_cache_clear(const AdminReq *r, const char *arg, size_t len) {
    (void)arg; (void)len;
    char dir[PATH_MAX];
    if (cache_dir(r, NULL, dir, sizeof dir)) { admin_err("imager.cacheFailed", NULL); return; }
    as_user(r);
    long long freed = 0;
    int dfd = open(dir, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    DIR *d = dfd >= 0 ? fdopendir(dfd) : NULL;
    struct dirent *e;
    while (d && (e = readdir(d))) {
        if (e->d_name[0] == '.') continue;
        struct stat st;
        if (fstatat(dfd, e->d_name, &st, AT_SYMLINK_NOFOLLOW)) continue;
        if (S_ISREG(st.st_mode)) { if (!unlinkat(dfd, e->d_name, 0)) freed += st.st_size; continue; }
        if (!S_ISDIR(st.st_mode) || !img_valid_tag(e->d_name)) continue;
        int sfd = openat(dfd, e->d_name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
        DIR *sd = sfd >= 0 ? fdopendir(sfd) : NULL;
        struct dirent *se;
        while (sd && (se = readdir(sd))) {
            struct stat ss;
            if (se->d_name[0] == '.' || fstatat(sfd, se->d_name, &ss, AT_SYMLINK_NOFOLLOW) || !S_ISREG(ss.st_mode)) continue;
            if (!unlinkat(sfd, se->d_name, 0)) freed += ss.st_size;
        }
        if (sd) closedir(sd); else if (sfd >= 0) close(sfd);
        unlinkat(dfd, e->d_name, AT_REMOVEDIR);
    }
    if (d) closedir(d); else if (dfd >= 0) close(dfd);
    as_root();
    admin_ok("imager.cacheCleared", "size", lerr_int(freed), NULL);
}

/* ---- writing ---- */

static int read_user_small(const AdminReq *r, const char *path, char *out, size_t cap, size_t *n) {
    struct stat st;
    int fd = disk_admin_open_user_file(r, path, &st);
    if (fd < 0) return -1;
    if ((size_t)st.st_size >= cap) { close(fd); return -1; }
    size_t o = 0;
    while (o < (size_t)st.st_size) {
        ssize_t k = read(fd, out + o, (size_t)st.st_size - o);
        if (k < 0 && errno == EINTR) continue;
        if (k <= 0) break;
        o += (size_t)k;
    }
    close(fd);
    if (o != (size_t)st.st_size) return -1;
    out[o] = 0;
    *n = o;
    return 0;
}

typedef struct { int fd; } FdRead;
static int fd_read(void *ud, uint64_t off, void *buf, size_t n) {
    FdRead *f = ud;
    size_t o = 0;
    while (o < n) {
        ssize_t k = pread(f->fd, (char *)buf + o, n - o, (off_t)(off + o));
        if (k < 0 && errno == EINTR) continue;
        if (k <= 0) return -1;
        o += (size_t)k;
    }
    return 0;
}

/* The disk is still the one picked: same size, model and serial, and still removable. */
static int same_disk(const DiskTarget *t, uint64_t size, const char *model, const char *serial) {
    char m[256], s[256];
    if (disk_identity(t->disk, m, sizeof m, s, sizeof s)) return 0;
    return disk_size(t->disk) == size && size == t->size && !strcmp(m, model) && !strcmp(s, serial) && disk_is_removable(t->disk) && !disk_is_system(t->disk);
}

static ssize_t read_some(int fd, uint8_t *b, size_t n) {
    for (;;) {
        ssize_t k = read(fd, b, n);
        if (k < 0 && errno == EINTR) continue;
        return k;
    }
}

/* Writes b[0..n) at off (O_DIRECT when aligned). */
static int put(int out, int *direct, const uint8_t *b, size_t n, uint64_t off, int ssz) {
    if (*direct && (n % (size_t)ssz || off % (uint64_t)ssz)) {
        fcntl(out, F_SETFL, fcntl(out, F_GETFL) & ~O_DIRECT);
        *direct = 0;
    }
    for (size_t o = 0; o < n;) {
        ssize_t w = pwrite(out, b + o, n - o, (off_t)(off + o));
        if (w < 0 && errno == EINTR) continue;
        if (w <= 0) return -1;
        o += (size_t)w;
    }
    return 0;
}

enum { E_OK = 0, E_READ, E_WRITE, E_VERIFYREAD, E_XZ, E_TOOBIG, E_CHANGED, E_SHORT };

void imager_act_write(const AdminReq *r, const char *arg, size_t len) {
    char *buf = strndup(arg, len), *f[8];
    int nf = buf ? lines(buf, f, 8) : -1;
    if (nf != 6 && nf != 8) { free(buf); admin_err("admin.invalidArgument", NULL); return; }
    int official = !strcmp(f[4], "official");
    if (!official && strcmp(f[4], "custom")) { free(buf); admin_err("admin.invalidArgument", NULL); return; }
    if (official != (nf == 8)) { free(buf); admin_err("admin.invalidArgument", NULL); return; }
    char *end;
    errno = 0;
    unsigned long long want_size = strtoull(f[1], &end, 10);
    if (errno || *end || !f[1][0] || strlen(f[2]) > 255 || strlen(f[3]) > 255) { free(buf); admin_err("admin.invalidArgument", NULL); return; }
    char model[256], serial[256], image[PATH_MAX], sums_p[PATH_MAX] = "", sig_p[PATH_MAX] = "";
    pathf(model, sizeof model, "%s", f[2]);
    pathf(serial, sizeof serial, "%s", f[3]);
    pathf(image, sizeof image, "%s", f[5]);
    if (official) { pathf(sums_p, sizeof sums_p, "%s", f[6]); pathf(sig_p, sizeof sig_p, "%s", f[7]); }

    /* 1. the disk: whole, not the system one, removable, and the one the user picked */
    DiskTarget t;
    if (disk_admin_whole(r, f[0], &t)) { free(buf); return; }
    free(buf);
    if (!t.removable) { admin_err("imager.notRemovable", "name", t.disk, NULL); return; }
    /* LordOS images use an MBR table (2 TiB at most); a larger "removable" drive is also not a card */
    if (t.size > (2ull << 40)) { admin_err("imager.driveTooLarge", "name", t.disk, NULL); return; }
    char ro[8] = "0", rop[96];
    pathf(rop, sizeof rop, "/sys/class/block/%s/ro", t.disk);
    FILE *rf = fopen(rop, "re");
    if (rf) { if (!fgets(ro, sizeof ro, rf)) ro[0] = '0'; fclose(rf); }
    if (ro[0] == '1') { admin_err("imager.readOnly", "name", t.disk, NULL); return; }
    if (!same_disk(&t, want_size, model, serial)) { admin_err("imager.diskChanged", "name", t.disk, NULL); return; }

    /* 2. the image: the user's file, a known kind, not on that disk */
    const char *base = strrchr(image, '/');
    base = base ? base + 1 : image;
    int kind = img_kind(base);
    if (!kind || !img_valid_name(base)) { admin_err("imager.badImageName", NULL); return; }
    struct stat ist;
    int in = disk_admin_open_user_file(r, image, &ist);
    if (in < 0) { admin_err("disk.imageOpenFailed", NULL); return; }
    if (disk_admin_file_on(&ist, t.disk)) { close(in); admin_err("disk.imageOnTarget", NULL); return; }
    uint64_t fsize = (uint64_t)ist.st_size, raw = fsize;
    if (kind == 2) {
        FdRead fr = { in };
        if (img_xz_size(fd_read, &fr, fsize, &raw)) { close(in); admin_err("imager.xzDamaged", NULL); return; }
        if (access(XZ, X_OK)) { close(in); admin_err("imager.noXz", NULL); return; }
    }
    if (raw == 0 || raw > MAX_IMAGE) { close(in); admin_err("imager.xzDamaged", NULL); return; }
    if (raw > t.size) { close(in); admin_err("disk.imageTooLarge", "name", t.name, "size", lerr_int((long long)raw), "capacity", lerr_int((long long)t.size), NULL); return; }

    /* 3. official: the signature and the listed hash, before anything is erased */
    uint8_t listed[32] = { 0 };
    if (official) {
        static char sums[IMG_SUMS_MAX + 1], sig[IMG_SIG_MAX + 1];
        size_t sn = 0, gn = 0;
        if (read_user_small(r, sums_p, sums, sizeof sums, &sn) || read_user_small(r, sig_p, sig, sizeof sig, &gn)) {
            close(in);
            admin_err("imager.noSignature", NULL);
            return;
        }
        if (img_sig_verify((const uint8_t *)sums, sn, sig, gn, codesig_release_key)) { close(in); admin_err("imager.badSignature", NULL); return; }
        if (img_sums_find(sums, sn, base, listed)) { close(in); admin_err("imager.notListed", "name", base, NULL); return; }
        disk_admin_job_begin(r, t.disk);
        Sha256 h;
        sha256_init(&h);
        static uint8_t cb[4 * MIB];
        uint64_t o = 0;
        double last = 0;
        posix_fadvise(in, 0, 0, POSIX_FADV_SEQUENTIAL);
        while (o < fsize && !disk_admin_cancelled()) {
            ssize_t k = pread(in, cb, sizeof cb, (off_t)o);
            if (k < 0 && errno == EINTR) continue;
            if (k <= 0) break;
            sha256_update(&h, cb, (size_t)k);
            o += (uint64_t)k;
            if (now_s() - last > 0.5 || o == fsize) {
                last = now_s();
                admin_say("LOG", "progress phase=check done=%llu total=%llu", (unsigned long long)o, (unsigned long long)fsize);   /* i18n-ok */
            }
        }
        uint8_t got[32];
        sha256_final(&h, got);
        if (disk_admin_cancelled()) { disk_admin_job_end(t.disk); close(in); admin_err("disk.cancelled", NULL); return; }
        if (o != fsize || memcmp(got, listed, 32)) { disk_admin_job_end(t.disk); close(in); admin_err("imager.hashMismatch", NULL); return; }
        admin_log("imager.verifiedImage", NULL);
    } else {
        disk_admin_job_begin(r, t.disk);
    }

    /* 4. right before opening: the same disk, nothing mounted outside /media, opened exclusively */
    if (!same_disk(&t, want_size, model, serial)) { disk_admin_job_end(t.disk); close(in); admin_err("imager.diskChanged", "name", t.disk, NULL); return; }
    if (disk_admin_unmount(&t)) { disk_admin_job_end(t.disk); close(in); return; }
    signal(SIGPIPE, SIG_IGN);                                /* xz may exit early: an error, not a signal */
    int out = disk_admin_open_excl(&t, O_DIRECT);
    if (out < 0) { disk_admin_job_end(t.disk); close(in); return; }
    struct stat ost;
    uint64_t bsz = 0;
    if (fstat(out, &ost) || !S_ISBLK(ost.st_mode) || major(ost.st_rdev) != t.major || minor(ost.st_rdev) != t.minor ||
        ioctl(out, BLKGETSIZE64, &bsz) || bsz != t.size || !same_disk(&t, want_size, model, serial)) {
        close(out);
        close(in);
        disk_admin_job_end(t.disk);
        admin_err("imager.diskChanged", "name", t.disk, NULL);
        return;
    }
    int ssz = 512;
    if (ioctl(out, BLKSSZGET, &ssz) || ssz < 512 || ssz > 65536) ssz = 512;
    int direct = 1;

    /* 5. write: raw straight from the file; xz through /usr/bin/xz as the user */
    const size_t CH = 4 * MIB;
    uint8_t *b = NULL, *ob = NULL;
    if (posix_memalign((void **)&b, 4096, CH) || posix_memalign((void **)&ob, 4096, CH)) {
        free(b); close(out); close(in); disk_admin_job_end(t.disk); admin_err("sys.outOfMemory", NULL); return;
    }
    Sha256 hin, hraw;
    sha256_init(&hin);                 /* the file's bytes (compared again with the listed hash) */
    sha256_init(&hraw);                /* what goes to the disk (compared with the read-back) */
    uint64_t off = 0, fin = 0;
    double last = 0, t0 = now_s();
    int err = E_OK;
    lseek(in, 0, SEEK_SET);
    if (kind == 1) {
        while (off < raw && !disk_admin_cancelled()) {
            ssize_t k = read_some(in, b, CH);
            if (k <= 0) { err = E_READ; break; }
            if (off + (uint64_t)k > raw) { err = E_CHANGED; break; }
            sha256_update(&hin, b, (size_t)k);
            sha256_update(&hraw, b, (size_t)k);
            if (put(out, &direct, b, (size_t)k, off, ssz)) { err = E_WRITE; break; }
            off += (uint64_t)k;
            fin += (uint64_t)k;
            if (now_s() - last > 0.5 || off == raw) {
                last = now_s();
                admin_say("LOG", "progress phase=write done=%llu total=%llu", (unsigned long long)off, (unsigned long long)raw);   /* i18n-ok */
            }
        }
    } else {
        int pin[2], pout[2];
        if (pipe2(pin, O_CLOEXEC) || pipe2(pout, O_CLOEXEC)) { err = E_XZ; goto written; }
        pid_t pid = fork();
        if (pid == 0) {
            dup2(pin[0], 0);
            dup2(pout[1], 1);
            int null = open("/dev/null", O_WRONLY);
            if (null >= 0) dup2(null, 2);
            syscall(SYS_close_range, 3, ~0u, 0);
            if (drop_to_user(r) || chdir("/")) _exit(126);
            char *const env[] = { "PATH=/usr/bin:/bin", "LANG=C.UTF-8", NULL };
            const char *const a[] = { XZ, "--decompress", "--stdout", "--threads=1", "--memlimit-decompress=512MiB", NULL };
            execve(XZ, (char *const *)a, env);
            _exit(127);
        }
        close(pin[0]);
        close(pout[1]);
        if (pid < 0) { close(pin[1]); close(pout[0]); err = E_XZ; goto written; }
        fcntl(pin[1], F_SETFL, O_NONBLOCK);
        size_t ib_len = 0, ib_off = 0, ob_len = 0;
        int in_eof = 0, out_eof = 0;
        while (!out_eof && !err && !disk_admin_cancelled()) {
            if (!in_eof && ib_off == ib_len) {
                ssize_t k = read_some(in, b, CH);
                if (k < 0) { err = E_READ; break; }
                if (k == 0) { in_eof = 1; close(pin[1]); pin[1] = -1; }
                else { sha256_update(&hin, b, (size_t)k); fin += (uint64_t)k; ib_len = (size_t)k; ib_off = 0; }
            }
            struct pollfd pf[2] = { { pout[0], POLLIN, 0 }, { pin[1], POLLOUT, 0 } };
            int np = pin[1] >= 0 && ib_off < ib_len ? 2 : 1;
            if (poll(pf, (nfds_t)np, 1000) < 0) { if (errno == EINTR) continue; err = E_XZ; break; }
            if (np == 2 && (pf[1].revents & (POLLOUT | POLLERR | POLLHUP))) {
                ssize_t w = write(pin[1], b + ib_off, ib_len - ib_off);
                if (w > 0) ib_off += (size_t)w;
                else if (w < 0 && errno != EAGAIN && errno != EINTR) { err = E_XZ; break; }
            }
            if (pf[0].revents & (POLLIN | POLLHUP | POLLERR)) {
                ssize_t k = read(pout[0], ob + ob_len, CH - ob_len);
                if (k < 0) { if (errno == EINTR || errno == EAGAIN) continue; err = E_XZ; break; }
                if (k == 0) out_eof = 1;
                else ob_len += (size_t)k;
                /* write whole 4 MiB pieces (aligned for O_DIRECT); the tail at the end */
                if (ob_len == CH || (out_eof && ob_len)) {
                    if (off + ob_len > raw) { err = E_TOOBIG; break; }
                    sha256_update(&hraw, ob, ob_len);
                    if (put(out, &direct, ob, ob_len, off, ssz)) { err = E_WRITE; break; }
                    off += ob_len;
                    ob_len = 0;
                }
                if (now_s() - last > 0.5) {
                    last = now_s();
                    admin_say("LOG", "progress phase=write done=%llu total=%llu", (unsigned long long)off, (unsigned long long)raw);   /* i18n-ok */
                }
            }
        }
        if (pin[1] >= 0) close(pin[1]);
        close(pout[0]);
        if (err || disk_admin_cancelled()) kill(pid, SIGKILL);
        int stc = 0;
        while (waitpid(pid, &stc, 0) < 0 && errno == EINTR) {}
        if (!err && !disk_admin_cancelled() && (!WIFEXITED(stc) || WEXITSTATUS(stc))) err = WIFEXITED(stc) && WEXITSTATUS(stc) >= 126 ? E_XZ : E_TOOBIG;
        if (!err && !disk_admin_cancelled() && (off != raw || fin != fsize)) err = E_SHORT;
        admin_say("LOG", "progress phase=write done=%llu total=%llu", (unsigned long long)off, (unsigned long long)raw);   /* i18n-ok */
    }
written:
    close(in);
    {
        uint8_t got[32];
        sha256_final(&hin, got);
        if (!err && !disk_admin_cancelled() && official && memcmp(got, listed, 32)) err = E_CHANGED;   /* the file changed after the check */
    }
    uint8_t want[32], back[32];
    sha256_final(&hraw, want);
    if (!err && !disk_admin_cancelled() && fsync(out)) err = E_WRITE;
    double tw = now_s() - t0;

    /* 6. verify: read the disk back, bypassing the cache */
    if (!err && !disk_admin_cancelled()) {
        ioctl(out, BLKFLSBUF);
        int rd = open(t.dev, O_RDONLY | O_DIRECT | O_CLOEXEC);
        struct stat rst;
        if (rd < 0 || fstat(rd, &rst) || major(rst.st_rdev) != t.major || minor(rst.st_rdev) != t.minor) err = E_VERIFYREAD;
        Sha256 hv;
        sha256_init(&hv);
        last = 0;
        for (uint64_t o = 0; !err && o < raw && !disk_admin_cancelled();) {
            size_t want_n = raw - o > CH ? CH : (size_t)(raw - o);
            size_t rdn = (want_n + (size_t)ssz - 1) / (size_t)ssz * (size_t)ssz;
            ssize_t k = pread(rd, b, rdn, (off_t)o);
            if (k < (ssize_t)want_n) { err = E_VERIFYREAD; break; }
            sha256_update(&hv, b, want_n);
            o += want_n;
            if (now_s() - last > 0.5 || o == raw) {
                last = now_s();
                admin_say("LOG", "progress phase=verify done=%llu total=%llu", (unsigned long long)o, (unsigned long long)raw);   /* i18n-ok */
            }
        }
        sha256_final(&hv, back);
        if (rd >= 0) close(rd);
    }
    int failed = err || disk_admin_cancelled() || memcmp(want, back, 32);
    if (failed) {
        /* never leave a half-written card that looks bootable */
        fcntl(out, F_SETFL, fcntl(out, F_GETFL) & ~O_DIRECT);
        disk_admin_wipe_ends(out, t.size);
        fsync(out);
    }
    ioctl(out, BLKRRPART);
    close(out);
    free(b);
    free(ob);
    disk_admin_job_end(t.disk);
    disk_admin_reread(t.disk);
    char hex[65];
    hex_encode(want, 32, hex);
    if (disk_admin_cancelled()) admin_err("imager.cancelledWiped", NULL);
    else if (err == E_READ) admin_err("disk.imageReadFailed", NULL);
    else if (err == E_WRITE) admin_err("disk.writeFailed", "name", t.name, "errno", lerr_errno(errno), NULL);
    else if (err == E_VERIFYREAD) admin_err("disk.verifyReadFailed", "name", t.name, NULL);
    else if (err == E_XZ) admin_err("imager.xzFailed", NULL);
    else if (err == E_TOOBIG || err == E_SHORT) admin_err("imager.xzDamaged", NULL);
    else if (err == E_CHANGED) admin_err("imager.imageChanged", NULL);
    else if (memcmp(want, back, 32)) admin_err("disk.verifyMismatch", NULL);
    else {
        char ms[24], mbps[24], sz[24];
        pathf(ms, sizeof ms, "%.0f", tw * 1000);
        pathf(mbps, sizeof mbps, "%.1f", tw > 0 ? raw / tw / 1e6 : 0);
        pathf(sz, sizeof sz, "%llu", (unsigned long long)raw);
        admin_ok("imager.written", "sha256", hex, "ms", ms, "mbps", mbps, "size", sz, "official", official ? "1" : "0", NULL);
    }
}
