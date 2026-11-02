/* disktemp_test.c — which hwmon sensor belongs to which disk (src/monitor/disktemp.c), on fake
 * sysfs trees laid out like the kernel's:
 *   nvme0n1    device → controller nvme0, sensor nvme0/hwmon3 (name "nvme", with max/crit)
 *   nvme1n1    native multipath: device → nvme-subsys1, whose entry nvme1 → the controller
 *   sda        device → SCSI device 0:0:0:0, sensor in its glue folder hwmon/hwmon4 ("drivetemp")
 *   sdb        a SCSI device whose only hwmon chip is something else ("acpitz"): no sensor
 *   mmcblk0    an SD card: no hwmon at all
 *   vda        a virtual disk: no hwmon at all
 *
 *   make -C server build/disktemp-test && ./server/build/disktemp-test   (in `make test`) */
#include "../src/monitor/disktemp.h"

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int fails, checks;
#define CHECK(c, ...) do { checks++; if (!(c)) { fails++; printf("✗ " __VA_ARGS__); printf("\n"); } } while (0)

static char root[256];

static void mkdirs(const char *rel) {
    char p[1024];
    snprintf(p, sizeof p, "%s/%s", root, rel);
    for (char *s = p + strlen(root) + 1; *s; s++) if (*s == '/') { *s = 0; mkdir(p, 0755); *s = '/'; }
    mkdir(p, 0755);
}

static void put(const char *rel, const char *text) {
    char p[1024];
    snprintf(p, sizeof p, "%s/%s", root, rel);
    char d[1024];
    snprintf(d, sizeof d, "%s", rel);
    char *sl = strrchr(d, '/');
    if (sl) { *sl = 0; mkdirs(d); }
    FILE *f = fopen(p, "w");
    if (f) { fputs(text, f); fclose(f); }
}

static void link_to(const char *target, const char *rel) {
    char p[1024], d[1024];
    snprintf(d, sizeof d, "%s", rel);
    char *sl = strrchr(d, '/');
    if (sl) { *sl = 0; mkdirs(d); }
    snprintf(p, sizeof p, "%s/%s", root, rel);
    if (symlink(target, p)) perror(p);
}

int main(void) {
    snprintf(root, sizeof root, "/tmp/disktemp-test-XXXXXX");
    if (!mkdtemp(root)) { perror("mkdtemp"); return 1; }

    /* NVMe: the sensor is a class device right under the controller */
    put("devices/pci0000:00/0000:00:1d.0/0000:3d:00.0/nvme/nvme0/hwmon3/name", "nvme\n");
    put("devices/pci0000:00/0000:00:1d.0/0000:3d:00.0/nvme/nvme0/hwmon3/temp1_input", "38850\n");
    put("devices/pci0000:00/0000:00:1d.0/0000:3d:00.0/nvme/nvme0/hwmon3/temp1_max", "81850\n");
    put("devices/pci0000:00/0000:00:1d.0/0000:3d:00.0/nvme/nvme0/hwmon3/temp1_crit", "84850\n");
    put("devices/pci0000:00/0000:00:1d.0/0000:3d:00.0/nvme/nvme0/hwmon3/temp2_input", "41850\n");
    link_to("../../devices/pci0000:00/0000:00:1d.0/0000:3d:00.0/nvme/nvme0", "block/nvme0n1/device");

    /* NVMe with native multipath: device is the subsystem; the controller is listed inside */
    put("devices/pci0000:00/0000:00:1c.0/0000:04:00.0/nvme/nvme1/hwmon5/name", "nvme\n");
    put("devices/pci0000:00/0000:00:1c.0/0000:04:00.0/nvme/nvme1/hwmon5/temp1_input", "45000\n");
    mkdirs("devices/virtual/nvme-subsystem/nvme-subsys1");
    link_to("../../../pci0000:00/0000:00:1c.0/0000:04:00.0/nvme/nvme1", "devices/virtual/nvme-subsystem/nvme-subsys1/nvme1");
    link_to("../../devices/virtual/nvme-subsystem/nvme-subsys1", "block/nvme1n1/device");

    /* SATA with drivetemp: hwmon/hwmonN under the SCSI device; no limits exposed */
    put("devices/pci0000:00/0000:00:1f.2/ata1/host0/target0:0:0/0:0:0:0/hwmon/hwmon4/name", "drivetemp\n");
    put("devices/pci0000:00/0000:00:1f.2/ata1/host0/target0:0:0/0:0:0:0/hwmon/hwmon4/temp1_input", "31000\n");
    link_to("../../devices/pci0000:00/0000:00:1f.2/ata1/host0/target0:0:0/0:0:0:0", "block/sda/device");

    /* a chip that is not a disk sensor must not count */
    put("devices/platform/x/host1/target1:0:0/1:0:0:0/hwmon/hwmon6/name", "acpitz\n");
    put("devices/platform/x/host1/target1:0:0/1:0:0:0/hwmon/hwmon6/temp1_input", "50000\n");
    link_to("../../devices/platform/x/host1/target1:0:0/1:0:0:0", "block/sdb/device");

    /* card and virtual disk: no sensor */
    mkdirs("devices/platform/axi/1000fff000.mmc/mmc_host/mmc0/mmc0:aaaa");
    link_to("../../devices/platform/axi/1000fff000.mmc/mmc_host/mmc0/mmc0:aaaa", "block/mmcblk0/device");
    mkdirs("devices/pci0000:00/0000:00:05.0/virtio2");
    link_to("../../devices/pci0000:00/0000:00:05.0/virtio2", "block/vda/device");

    DiskSensor s;
    double c = 0;
    CHECK(disktemp_find(root, "nvme0n1", &s) == DT_NVME, "nvme0n1 has an nvme sensor");
    CHECK(s.max_c == 81 && s.crit_c == 84, "nvme0n1 limits 81/84, got %d/%d", s.max_c, s.crit_c);
    CHECK(!disktemp_read(&s, &c) && c > 38.8 && c < 38.9, "nvme0n1 reads 38.85, got %.2f", c);
    CHECK(!strcmp(disktemp_kind_name(s.kind), "nvme"), "kind name nvme");

    CHECK(disktemp_find(root, "nvme1n1", &s) == DT_NVME, "nvme1n1 (multipath) found through its controller");
    CHECK(s.max_c == -1 && s.crit_c == -1, "nvme1n1 without limits");
    CHECK(!disktemp_read(&s, &c) && c == 45.0, "nvme1n1 reads 45");

    CHECK(disktemp_find(root, "sda", &s) == DT_DRIVETEMP, "sda has a drivetemp sensor");
    CHECK(!disktemp_read(&s, &c) && c == 31.0, "sda reads 31");

    CHECK(disktemp_find(root, "sdb", &s) == DT_NONE, "sdb: an acpitz chip is not a disk sensor");
    CHECK(disktemp_find(root, "mmcblk0", &s) == DT_NONE, "mmcblk0: no sensor");
    CHECK(disktemp_read(&s, &c) == -1, "no sensor: no value");
    CHECK(disktemp_find(root, "vda", &s) == DT_NONE, "vda: no sensor");
    CHECK(disktemp_find(root, "sdx", &s) == DT_NONE, "missing disk: no sensor");
    CHECK(disktemp_find(root, "../sda", &s) == DT_NONE, "a path is not a disk name");
    CHECK(disktemp_kind_name(DT_NONE) == NULL, "no kind name without a sensor");

    /* a bogus reading is "no value", never a number */
    put("devices/pci0000:00/0000:00:1f.2/ata1/host0/target0:0:0/0:0:0:0/hwmon/hwmon4/temp1_input", "-273150\n");
    disktemp_find(root, "sda", &s);
    CHECK(disktemp_read(&s, &c) == -1, "absurd reading refused");
    put("devices/pci0000:00/0000:00:1f.2/ata1/host0/target0:0:0/0:0:0:0/hwmon/hwmon4/temp1_input", "garbage\n");
    CHECK(disktemp_read(&s, &c) == -1, "garbage refused");

    char cmd[300];
    snprintf(cmd, sizeof cmd, "rm -rf '%s'", root);
    if (system(cmd)) printf("could not remove %s\n", root);
    printf("disktemp-test: %d checks, %d failures\n", checks, fails);
    return fails ? 1 : 0;
}
