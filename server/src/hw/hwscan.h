/* hwscan.h — read-only hardware facts for the Store's "Suggested for your computer"
 * (apps/store, sdk/suggestengine.js, assets/store/suggestions.json; see docs/PACKAGES.md).
 *
 * Everything here reads world-readable kernel files without root: DMI strings
 * (/sys/class/dmi/id), the device tree model (/proc/device-tree), PCI and USB IDs and their drivers
 * (/sys/bus/pci, /sys/bus/usb), the virtio serial port names (/sys/class/virtio-ports), the
 * "libsane_matched" flag udev keeps for scanners (/run/udev/data) and, when the kernel allows it
 * (kernel.dmesg_restrict=0), the firmware files a driver asked for and didn't find (/dev/kmsg).
 * Nothing is written, nothing is executed and nothing goes over the network.
 *
 * With dmesg_restrict=1 (Debian's default) the session can't read the kernel log; then the root
 * helper's fixed action `hw-firmware-missing` (no arguments) reads it and returns only the
 * validated names of missing firmware files.
 *
 * `root` prefixes every path ("" for the real system); tests and the fixture mode
 * (LORD_HWSCAN_ROOT, development only) point it at a fake tree, where the kernel log is the
 * plain file <root>/kmsg. */
#ifndef LORD_HW_HWSCAN_H
#define LORD_HW_HWSCAN_H

#include <stddef.h>

#define HWSCAN_FW_MAX 64                /* missing firmware files reported */

typedef struct {
    char file[200];                     /* relative to /lib/firmware */
    char driver[33];                    /* the driver or subsystem that asked ("iwlwifi", "bluetooth"), or "" */
} HwFwMiss;

/* The facts as JSON (malloc'd), or NULL on out-of-memory:
 *   { fixture, arch, dmi: { sysVendor, productName, boardVendor, boardName, chassisType },
 *     model, compatible: [], pci: [{ vendor, device, class, driver }],
 *     usb: [{ vendor, product, class, ifaces: ["070104", …], driver, sane }], virtio: [names],
 *     firmware: { readable, missing: [{ file, driver }] } }
 * IDs are lowercase hex without "0x". firmware.readable is false when the kernel log can't be
 * read without root (the Store then asks hw-firmware-missing). */
char *hwscan_json(const char *root);

/* Parses kernel log text (kmsg records "pri,seq,ts,flags;message" or dmesg lines) for the
 * firmware loader's "Direct firmware load for X failed" and "firmware: failed to load X" lines.
 * Fills up to max unique, valid names; returns how many. */
int hwscan_fw_parse(const char *text, size_t len, HwFwMiss *out, int max);

/* 1 if name is a safe relative firmware path: [A-Za-z0-9._+,-/], no "..", no leading '/', ≤ 199. */
int hwscan_fw_name_valid(const char *name);

/* 1 if <root>/lib/firmware[/updates]/<name>[.xz|.zst] exists (installed since the miss). */
int hwscan_fw_present(const char *root, const char *name);

/* Reads the kernel log ring: <root>/kmsg when root is non-empty, /dev/kmsg otherwise
 * (non-blocking, every record still in the ring, at most 4 MB). NULL with *denied = 1 when the
 * kernel refuses (dmesg_restrict). */
char *hwscan_read_kmsg(const char *root, size_t *len, int *denied);

/* Missing firmware files: parse the log and drop the ones present now. */
int hwscan_fw_missing(const char *root, const char *text, size_t len, HwFwMiss *out, int max);

#endif
