/* uevent.c — kernel uevents (see uevent.h). */
#include "uevent.h"

#include <errno.h>
#include <linux/netlink.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int fd = -1;

int uevent_open(void) {
    if (fd >= 0) return 0;
    fd = socket(AF_NETLINK, SOCK_DGRAM | SOCK_CLOEXEC | SOCK_NONBLOCK, NETLINK_KOBJECT_UEVENT);
    if (fd < 0) return -1;
    struct sockaddr_nl a = { .nl_family = AF_NETLINK, .nl_groups = 1 };      /* group 1: the kernel's */
    if (bind(fd, (struct sockaddr *)&a, sizeof a)) { close(fd); fd = -1; return -1; }
    return 0;
}

int uevent_fd(void) { return fd; }

int uevent_parse(const char *buf, int len) {
    if (len <= 0 || !memchr(buf, '@', (size_t)len)) return 0;      /* udev's own (libudev) messages have no '@' header */
    for (int off = 0; off < len;) {
        const char *s = buf + off;
        size_t n = strnlen(s, (size_t)(len - off));
        if (n > 10 && !strncmp(s, "SUBSYSTEM=", 10)) {
            const char *v = s + 10;
            if (!strcmp(v, "drm")) return UEV_DRM;
            if (!strcmp(v, "power_supply")) return UEV_POWER;
            if (!strcmp(v, "backlight")) return UEV_BACKLIGHT;
            return 0;
        }
        off += (int)n + 1;
    }
    return 0;
}

int uevent_dispatch(void) {
    int mask = 0;
    char buf[8192];
    for (;;) {
        struct sockaddr_nl from;
        struct iovec iov = { buf, sizeof buf - 1 };
        struct msghdr m = { .msg_name = &from, .msg_namelen = sizeof from, .msg_iov = &iov, .msg_iovlen = 1 };
        ssize_t r = recvmsg(fd, &m, 0);
        if (r < 0) { if (errno == EINTR) continue; break; }
        if (from.nl_pid != 0) continue;                             /* only the kernel */
        buf[r] = 0;
        mask |= uevent_parse(buf, (int)r);
    }
    return mask;
}
