/* seat_cycle.c — plays lord-sessiond's monitor part against one session, to test what happens
 * when a session leaves the monitor and comes back (docs/GPU.md, "When the GPU context is
 * re-created"; run by tests/gpu_cycle.sh).
 *
 *   sudo build/seat-cycle <remote.sock> <card> <cycles> <on_ms> <off_ms> [snapshot-prefix [hook]]
 *
 * It connects to the session's remote.sock as root, says RP_SEAT and, <cycles> times, opens the
 * card (DRM master), lends it with RP_DISPLAY, waits <on_ms>, saves what the CRTC shows as
 * <prefix>-NN.bmp (GETFB2 + MAP_DUMB: works with dumb-backed buffers such as vkms with Mesa's
 * kms_swrast), sends RP_UNDISPLAY, waits <off_ms>, drops master and closes the card: the same
 * steps lord-sessiond takes on a session switch. No input devices are lent. After each cycle it runs
 * <hook> (a shell command, with the cycle number in $CYCLE), and at the end it lends the card once more
 * and leaves the session on the monitor (snapshot <prefix>-final.bmp).
 * A test tool only: it is never installed. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>
#include <drm/drm.h>
#include <drm/drm_mode.h>

enum { RP_SEAT = 18, RP_DISPLAY = 19, RP_UNDISPLAY = 22 };

static int sock = -1;

static int send_msg(int type, const void *d, size_t n, int fd) {
    unsigned char h[5];
    uint32_t len = (uint32_t)(1 + n);
    memcpy(h, &len, 4);
    h[4] = (unsigned char)type;
    struct iovec iov[2] = { { h, 5 }, { (void *)d, n } };
    char cbuf[CMSG_SPACE(sizeof(int))];
    memset(cbuf, 0, sizeof cbuf);
    struct msghdr mh = { .msg_iov = iov, .msg_iovlen = n ? 2 : 1 };
    if (fd >= 0) {
        mh.msg_control = cbuf;
        mh.msg_controllen = sizeof cbuf;
        struct cmsghdr *cm = CMSG_FIRSTHDR(&mh);
        cm->cmsg_level = SOL_SOCKET;
        cm->cmsg_type = SCM_RIGHTS;
        cm->cmsg_len = CMSG_LEN(sizeof(int));
        memcpy(CMSG_DATA(cm), &fd, sizeof(int));
    }
    return sendmsg(sock, &mh, MSG_NOSIGNAL) == (ssize_t)(5 + n) ? 0 : -1;
}

/* Waits ms while reading (and ignoring) whatever the session sends. */
static void wait_ms(int ms) {
    struct timespec t0;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (;;) {
        struct timespec t;
        clock_gettime(CLOCK_MONOTONIC, &t);
        long left = ms - ((t.tv_sec - t0.tv_sec) * 1000 + (t.tv_nsec - t0.tv_nsec) / 1000000);
        if (left <= 0) return;
        struct pollfd p = { sock, POLLIN, 0 };
        if (poll(&p, 1, (int)left) > 0) {
            char buf[4096];
            if (read(sock, buf, sizeof buf) <= 0) { fprintf(stderr, "seat-cycle: the session closed the socket\n"); exit(1); }
        }
    }
}

static void put16(FILE *f, unsigned v) { fputc(v & 255, f); fputc(v >> 8 & 255, f); }
static void put32(FILE *f, unsigned v) { put16(f, v & 0xffff); put16(f, v >> 16); }

/* What the first active CRTC shows, as a 24-bit BMP. 0 if saved. */
static int snapshot(int card, const char *path) {
    uint32_t crtcs[16];
    struct drm_mode_card_res r = {0};
    if (ioctl(card, DRM_IOCTL_MODE_GETRESOURCES, &r)) return -1;
    r = (struct drm_mode_card_res){ .crtc_id_ptr = (uint64_t)(uintptr_t)crtcs, .count_crtcs = r.count_crtcs < 16 ? r.count_crtcs : 16 };
    if (ioctl(card, DRM_IOCTL_MODE_GETRESOURCES, &r)) return -1;
    for (uint32_t i = 0; i < r.count_crtcs && i < 16; i++) {
        struct drm_mode_crtc c = { .crtc_id = crtcs[i] };
        if (ioctl(card, DRM_IOCTL_MODE_GETCRTC, &c) || !c.fb_id) continue;
        struct drm_mode_fb_cmd2 fb = { .fb_id = c.fb_id };
        if (ioctl(card, DRM_IOCTL_MODE_GETFB2, &fb) || !fb.handles[0]) return -1;
        struct drm_mode_map_dumb md = { .handle = fb.handles[0] };
        int ret = -1;
        size_t size = (size_t)fb.pitches[0] * fb.height;
        if (!ioctl(card, DRM_IOCTL_MODE_MAP_DUMB, &md)) {
            uint8_t *m = mmap(NULL, size + fb.offsets[0], PROT_READ, MAP_SHARED, card, (off_t)md.offset);
            FILE *f = m != MAP_FAILED ? fopen(path, "wb") : NULL;
            if (f) {
                unsigned w = fb.width, h = fb.height, row = (w * 3 + 3) & ~3u;
                fputc('B', f); fputc('M', f); put32(f, 54 + row * h); put32(f, 0); put32(f, 54);
                put32(f, 40); put32(f, w); put32(f, h); put16(f, 1); put16(f, 24); put32(f, 0);
                put32(f, row * h); put32(f, 2835); put32(f, 2835); put32(f, 0); put32(f, 0);
                for (unsigned y = h; y-- > 0;) {
                    const uint32_t *px = (const uint32_t *)(m + fb.offsets[0] + (size_t)y * fb.pitches[0]);
                    for (unsigned x = 0; x < w; x++) { fputc(px[x] & 255, f); fputc(px[x] >> 8 & 255, f); fputc(px[x] >> 16 & 255, f); }
                    for (unsigned x = w * 3; x < row; x++) fputc(0, f);
                }
                fclose(f);
                ret = 0;
            }
            if (m != MAP_FAILED) munmap(m, size + fb.offsets[0]);
        }
        struct drm_gem_close gc = { .handle = fb.handles[0] };
        ioctl(card, DRM_IOCTL_GEM_CLOSE, &gc);
        return ret;
    }
    return -1;
}

int main(int argc, char **argv) {
    if (argc < 6) {
        fprintf(stderr, "usage: %s <remote.sock> <card> <cycles> <on_ms> <off_ms> [snapshot-prefix [hook]]\n", argv[0]);
        return 2;
    }
    int cycles = atoi(argv[3]), on_ms = atoi(argv[4]), off_ms = atoi(argv[5]);
    const char *prefix = argc > 6 && argv[6][0] ? argv[6] : NULL, *hook = argc > 7 ? argv[7] : NULL;
    sock = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    struct sockaddr_un a = { .sun_family = AF_UNIX };
    snprintf(a.sun_path, sizeof a.sun_path, "%s", argv[1]);
    if (sock < 0 || connect(sock, (struct sockaddr *)&a, sizeof a)) { perror("seat-cycle: connect"); return 1; }
    if (send_msg(RP_SEAT, NULL, 0, -1)) { perror("seat-cycle: RP_SEAT"); return 1; }
    for (int i = 0; i < cycles; i++) {
        int card = open(argv[2], O_RDWR | O_CLOEXEC);
        if (card < 0) { perror("seat-cycle: card"); return 1; }
        ioctl(card, DRM_IOCTL_SET_MASTER, 0);
        unsigned char d[2] = { 1, 0 };
        if (send_msg(RP_DISPLAY, d, sizeof d, card)) { perror("seat-cycle: RP_DISPLAY"); return 1; }
        wait_ms(on_ms);
        if (prefix) {
            char path[512];
            snprintf(path, sizeof path, "%s-%02d.bmp", prefix, i + 1);
            if (snapshot(card, path)) fprintf(stderr, "seat-cycle: no snapshot for cycle %d\n", i + 1);
        }
        printf("cycle %d: shown\n", i + 1);
        fflush(stdout);
        if (send_msg(RP_UNDISPLAY, NULL, 0, -1)) { perror("seat-cycle: RP_UNDISPLAY"); return 1; }
        wait_ms(off_ms);
        ioctl(card, DRM_IOCTL_DROP_MASTER, 0);
        close(card);
        if (hook) {
            char n[16];
            snprintf(n, sizeof n, "%d", i + 1);
            setenv("CYCLE", n, 1);
            fflush(stdout);
            if (system(hook) != 0) fprintf(stderr, "seat-cycle: the hook failed\n");
        }
    }
    /* back on the monitor at the end, so the last state can be looked at */
    int card = open(argv[2], O_RDWR | O_CLOEXEC);
    if (card >= 0) {
        ioctl(card, DRM_IOCTL_SET_MASTER, 0);
        unsigned char d[2] = { 1, 0 };
        send_msg(RP_DISPLAY, d, sizeof d, card);
        wait_ms(on_ms);
        if (prefix) {
            char path[512];
            snprintf(path, sizeof path, "%s-final.bmp", prefix);
            snapshot(card, path);
        }
        close(card);
    }
    return 0;
}
