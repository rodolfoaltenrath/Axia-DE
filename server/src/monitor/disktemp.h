/* disktemp.h — disk temperatures from the kernel's hwmon sensors, without root (see disktemp.c and
 * docs/MONITOR.md, "Disk temperature and health").
 *
 * NVMe (also an M.2 drive on a Raspberry Pi 5 HAT) has a sensor from the nvme driver itself; SATA
 * disks get one from the drivetemp module (loaded at boot by the lordos package). microSD/eMMC cards,
 * virtual disks and anything else have none: the caller shows "No sensor", never a made-up value. */
#ifndef LORD_DISKTEMP_H
#define LORD_DISKTEMP_H

#include <limits.h>
#include <stddef.h>

enum { DT_NONE = 0, DT_NVME = 1, DT_DRIVETEMP = 2 };

typedef struct {
    int kind;                           /* DT_NONE: no sensor */
    char input[PATH_MAX];               /* .../hwmonN/temp1_input */
    int max_c, crit_c;                  /* the drive's own limits (temp1_max / temp1_crit), -1 if not exposed */
} DiskSensor;

/* Finds the sensor of whole disk `disk` (sda, nvme0n1) under sysfs root `sys` ("/sys"; a fake tree
 * in tests). Looks in the disk's device folder (hwmonN directly, as for an NVMe controller, or
 * hwmon/hwmonN, as for a SCSI device), then, for NVMe with native multipath (the device is an
 * nvme-subsystem), in its controllers. Only chips named "nvme" or "drivetemp" count. Returns kind. */
int disktemp_find(const char *sys, const char *disk, DiskSensor *s);
/* Reads the current temperature in °C (one small sysfs read). 0 on success. */
int disktemp_read(const DiskSensor *s, double *celsius);
/* Short name for the JSON: "nvme", "drivetemp" or NULL. */
const char *disktemp_kind_name(int kind);

#endif
