/* smartctl_admin.c — the "smart-*" actions of the privileged helper: the Disks app's advanced
 * SMART section and the Monitor's health summary, through Debian's smartmontools.
 *
 *   smart-info <disk>               smartctl -j -x            (everything: attributes, logs, tests)
 *   smart-health <disk>             smartctl -j -n standby -i -H -A -c -l selftest
 *                                   (the summary; a resting disk is not woken up)
 *   smart-test <disk>\nshort|long   smartctl -j -t short|long (starts a self-test)
 *   smart-abort <disk>              smartctl -j -X            (stops a running self-test)
 *
 * Rules, all on the root side:
 *   - <disk> is a whole disk that smartctl understands: sd<letters>, hd<letters> or nvme<N>n<M>
 *     (smartctl_valid_disk), it exists in /sys/block and /dev/<disk> is a block device with the same
 *     number (the same target() check as the disk-* actions). Partitions, loop, dm, md, zram,
 *     cards and path tricks are refused before anything runs;
 *   - one program at a fixed path, /usr/sbin/smartctl, with a fixed argument list built here; the
 *     only variable parts are the validated /dev/<disk> and one of two words. No shell;
 *   - reading is open to any session user (like disk-smart); starting or stopping a self-test on an
 *     internal disk needs an administrator, on a removable one any session user (like udisks);
 *   - a USB disk whose bridge smartctl does not know is retried once with "-d sat".
 * The JSON is compacted (smartctl_compact) and sent as LOG lines of at most 900 bytes, cut on
 * UTF-8 boundaries; OK carries "<lines> <smartctl exit status>" (its bit mask: the app reads it).
 * Missing smartctl is the error disk.noSmartctl: the app offers to install smartmontools with the
 * package engine (pkg-install), never from here. */
#include "disk.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define SMARTCTL "/usr/sbin/smartctl"
#define OUT_MAX (1024 * 1024)
#define CHUNK 900
#define MAX_LINES 1200

/* runs smartctl with argv (argv[0] is SMARTCTL), output in out; exit status, -1 if it did not run */
static int run(const char *const argv[], char *out, size_t cap, size_t *len) {
    int rc = admin_run_capture(argv, out, cap);
    *len = strlen(out);
    return rc;
}

static int unknown_bridge(const char *out) { return strstr(out, "Unknown USB bridge") != NULL; }

static void send_json(char *out, size_t n, int rc) {
    n = smartctl_compact(out, n);
    if (!n || out[0] != '{') { admin_err("disk.smartFailed", NULL); return; }
    int lines = 0;
    for (size_t i = 0; i < n && lines < MAX_LINES;) {
        size_t k = smartctl_chunk(out + i, n - i, CHUNK);
        admin_say("LOG", "%.*s", (int)k, out + i);   /* i18n-ok */
        i += k;
        lines++;
    }
    admin_say("OK", "%d %d", lines, rc);   /* i18n-ok */
}

/* mode: 'i' info, 'h' health, 't' test, 'x' abort */
static void act(const AdminReq *r, const char *arg, size_t len, int mode) {
    char a[64], *word = NULL;
    if (!len || len >= sizeof a || memchr(arg, 0, len)) { admin_err("admin.invalidArgument", NULL); return; }
    memcpy(a, arg, len);
    a[len] = 0;
    char *nl = strchr(a, '\n');
    if (nl) { *nl = 0; word = nl + 1; char *e = strchr(word, '\n'); if (e) { if (e[1]) { admin_err("admin.invalidArgument", NULL); return; } *e = 0; } }
    if (mode == 't' ? !word || (strcmp(word, "short") && strcmp(word, "long")) : word && *word) {
        admin_err("admin.invalidArgument", NULL);
        return;
    }
    if (!smartctl_valid_disk(a)) { admin_err("disk.badDevice", NULL); return; }
    char dev[48];
    int usb = 0;
    if (disk_admin_target(r, a, mode == 't' || mode == 'x', dev, sizeof dev, &usb)) return;   /* it said why */
    if (access(SMARTCTL, X_OK)) { admin_err("disk.noSmartctl", NULL); return; }
    char *out = malloc(OUT_MAX);
    if (!out) { admin_err("disk.smartFailed", NULL); return; }
    for (int sat = 0; sat < 2; sat++) {
        const char *v[16];
        int k = 0;
        v[k++] = SMARTCTL;
        v[k++] = "-j";
        if (sat) { v[k++] = "-d"; v[k++] = "sat"; }
        if (mode == 'i') v[k++] = "-x";
        else if (mode == 'h') { v[k++] = "-n"; v[k++] = "standby"; v[k++] = "-i"; v[k++] = "-H"; v[k++] = "-A"; v[k++] = "-c"; v[k++] = "-l"; v[k++] = "selftest"; }
        else if (mode == 't') { v[k++] = "-t"; v[k++] = !strcmp(word, "long") ? "long" : "short"; }
        else v[k++] = "-X";
        v[k++] = dev;
        v[k] = NULL;
        size_t n = 0;
        int rc = run(v, out, OUT_MAX, &n);
        /* smartctl's exit status is a bit mask up to 255 (bit 7: errors in the self-test log), so
         * any status counts as long as JSON came out; send_json refuses anything else */
        if (rc < 0 || !n) { free(out); admin_err("disk.smartFailed", NULL); return; }
        if (!sat && usb && unknown_bridge(out)) continue;   /* try SAT pass-through once */
        send_json(out, n, rc);
        free(out);
        return;
    }
    free(out);
    admin_err("disk.smartFailed", NULL);
}

void smart_act_info(const AdminReq *r, const char *arg, size_t len) { act(r, arg, len, 'i'); }
void smart_act_health(const AdminReq *r, const char *arg, size_t len) { act(r, arg, len, 'h'); }
void smart_act_test(const AdminReq *r, const char *arg, size_t len) { act(r, arg, len, 't'); }
void smart_act_abort(const AdminReq *r, const char *arg, size_t len) { act(r, arg, len, 'x'); }
