/* status.h — installed packages, read from /var/lib/dpkg/status (read-only: the dpkg
 * database is written only by dpkg). */
#ifndef LORD_PKG_STATUS_H
#define LORD_PKG_STATUS_H

#include <stddef.h>
#include <stdint.h>

#define DPKG_STATUS "/var/lib/dpkg/status"

typedef struct {
    const char *name, *version, *arch, *summary, *section, *source;
    const char *depends, *predepends, *provides, *conflicts, *breaks, *replaces, *recommends;
    uint32_t isize;
    uint8_t flags;                      /* IF_ESSENTIAL, IF_PROTECTED, IF_MA_* from idx.h */
    uint8_t prio;
    uint8_t native;                     /* native architecture or all */
    uint8_t config_only;                /* removed but with its configuration ("config-files") */
} StPkg;

typedef struct {
    StPkg *p;                           /* sorted by name (native ones first) */
    int n;
    char *buf;
    long mtime, size;
} Status;

/* Loads (or reloads if it changed). arch: the native one ("amd64"). Only counts what has its
 * files on the system (installed, unpacked, half-configured, triggers-*). 0 ok. */
int  status_load(Status *s, const char *path, const char *arch);
void status_free(Status *s);
/* Native (or all) installed package with that name, or NULL. */
const StPkg *status_find(const Status *s, const char *name, size_t n);

/* dpkg's native architecture (amd64, arm64…), according to the machine. */
const char *pkg_native_arch(void);

#endif
