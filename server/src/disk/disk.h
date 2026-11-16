/* disk.h — disks and partitions for the Disks app (see docs/SYSTEM.md, "Disks").
 *
 * rust/axia-codecs/src/disk.rs implements the readers declared here, without root: /sys/block, the
 * udev database (/run/udev/data/b<major>:<minor>) and /proc/self/mountinfo. axia-server uses them
 * (lord.disks, js_disk.c; the main loop for the change notifications). Returned strings are malloc'd:
 * free() them. docs/RUST-DISK.md says what is Rust and what is not.
 * rust/axia-codecs/src/smart.rs holds the parsers (pure functions) for ATA SMART and the NVMe health
 * log, and smartctl.rs the pure helpers of the smart-* actions.
 */
#ifndef LORD_DISK_H
#define LORD_DISK_H

#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
#include <sys/stat.h>


/* Block device name as the kernel gives it: sda, sda1, nvme0n1p2, mmcblk0p1, vdb, loop3
 * (lowercase letters and digits, starts with a letter, up to 31). */
int  disk_valid_name(const char *name);
/* 1 if it is a partition (/sys/class/block/<n>/partition exists); its disk goes in disk. */
int  disk_parent(const char *name, char *disk, size_t n);
/* 1 if the (whole) disk holds something of the system: /, /usr, /var, /home, /boot, /boot/efi, the
 * live medium, an active swap, or a dm/md on top of it that holds any of those. */
int  disk_is_system(const char *disk);
/* 1 if removable: removable=1 in sysfs, connected over USB, or a card (mmc). */
int  disk_is_removable(const char *disk);
/* udev database property for that device ("ID_FS_TYPE"…); "" if missing. */
int  disk_udev_prop(const char *name, const char *key, char *out, size_t n);
/* File system label as is (ID_FS_LABEL_ENC with the \xNN decoded); "" if none. */
void disk_fs_label(const char *name, char *out, size_t n);
/* Size in bytes (/sys/class/block/<n>/size × 512). */
uint64_t disk_size(const char *name);
/* Model and short serial, exactly as disk_list_json() reports them (the imager's identity check). */
int  disk_identity(const char *name, char *model, size_t mn, char *serial, size_t sn);
/* Mount points of the device (separated by \n) from mountinfo; returns how many. */
int  disk_mounts(const char *name, char *out, size_t n);

/* List for JS: [{ name, model, vendor, serial, size, removable, transport, rotational, ro,
 * system, table, fstype, label, uuid, mountpoint, used, avail, partitions: [{ name, number, size,
 * fstype, label, uuid, partlabel, mountpoint, used, avail, start }] }]. malloc'd; NULL on failure. */
char *disk_list_json(void);
/* Format and rename programs present on this machine: {"fat32":1,"exfat":1,"ext4":1,"ntfs":0} */
char *disk_tools_json(void);

/* Change notifications while the app is open: one fd (epoll over mountinfo and udev events) for
 * the main loop. disk_watch(1) opens it, disk_watch(0) closes it; -1 when not open. */
int  disk_watch(int on);
int  disk_watch_fd(void);
/* Reads what arrived; 1 if disks or mounts changed (JS must be told). */
int  disk_watch_dispatch(void);

/* ---- SMART (axia-codecs smart.rs, pure functions: tests/smart_test.c) ---- */

enum { SMART_UNKNOWN = 0, SMART_OK = 1, SMART_WARN = 2, SMART_FAIL = 3 };
enum { SMART_ATA = 1, SMART_NVME = 2 };

typedef struct {
    uint8_t id, value, worst, thresh;
    uint16_t flags;                     /* bit 0: pre-fail */
    uint64_t raw;                       /* 48 bits */
    int failing;                        /* value <= threshold (threshold != 0) */
} SmartAttr;

typedef struct {
    int kind;                           /* SMART_ATA / SMART_NVME */
    int health;                         /* SMART_OK… */
    int checksum_ok;                    /* ATA: the 512 bytes sum to 0 */
    int status;                         /* ATA RETURN STATUS: 1 good, 0 threshold exceeded, -1 unknown */
    int temp_c;                         /* -1 if unknown */
    int64_t power_on_hours, power_cycles;           /* -1 if unknown */
    int64_t reallocated, pending, uncorrectable;    /* ATA 5, 197, 198 (-1 if absent) */
    int nattr;
    SmartAttr attr[30];
    /* NVMe */
    int crit_warning, spare, spare_thresh, used_pct;
    uint64_t data_read, data_written, media_errors, unsafe_shutdowns;   /* data_*: bytes */
} SmartReport;

/* data = SMART READ DATA (512 bytes); thresh = READ THRESHOLDS (512) or NULL; status = 1/0/-1. */
int  smart_ata_parse(const uint8_t data[512], const uint8_t *thresh, int status, SmartReport *r);
/* log = NVMe log page 02h (SMART / Health Information, 512 bytes). */
int  smart_nvme_parse(const uint8_t log[512], SmartReport *r);
/* Summary as JSON (without the attributes): {kind, health, temp, hours, cycles, ...}. */
int  smart_summary_json(const SmartReport *r, char *out, size_t n);
/* One ATA attribute as JSON: {id, value, worst, thresh, raw, prefail, failing}. */
int  smart_attr_json(const SmartAttr *a, char *out, size_t n);

/* ---- smartctl (smartctl.c, pure: tests/smartctl_test.c) ---- */

/* sd<letters>, hd<letters> or nvme<N>n<M>: the whole disks the smart-* actions accept. */
int    smartctl_valid_disk(const char *name);
/* Drops the whitespace outside JSON strings, in place; returns the new length. */
size_t smartctl_compact(char *s, size_t n);
/* Length of the next LOG piece (at most max bytes, never inside a UTF-8 sequence). */
size_t smartctl_chunk(const char *s, size_t n, size_t max);

#endif
