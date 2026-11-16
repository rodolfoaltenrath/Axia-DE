/* disk_test.c — the C ABI of src/disk/disk.h (Rust: rust/axia-codecs/src/disk.rs) against this
 * machine's /sys, udev database and mountinfo: what lord.disks.list() and the main loop rely on.
 *
 * It asserts relations that hold on any machine (a listed disk has the size and the system flag its
 * own accessors report, partitions belong to their disk, the JSON parses strictly), so it passes on a
 * build container with no disks too. The parsers themselves, on hostile synthetic trees, are tested
 * by `cargo test -p axia-codecs` (disk::tests).
 *
 *   make -C server build/disk-test && ./server/build/disk-test   (in `make test`) */
#include "../src/disk/disk.h"
#include "../src/sec/json.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails, checks;
#define CHECK(c, ...) do { checks++; if (!(c)) { fails++; printf("✗ " __VA_ARGS__); printf("\n"); } } while (0)

static const JVal *member(const JVal *o, const char *k, JType t) {
    const JVal *v = json_get(o, k);
    return v && v->type == t ? v : NULL;
}

int main(void) {
    /* names as the kernel gives them */
    const char *good[] = { "sda", "sda1", "nvme0n1", "nvme0n1p2", "mmcblk0p1", "vdb", "loop3", "a", "abcdefghijklmnopqrstuvwxyzabcde" };
    for (size_t i = 0; i < sizeof good / sizeof *good; i++) CHECK(disk_valid_name(good[i]), "accepts %s", good[i]);
    const char *bad[] = { "", "1sda", "SDA", "sd-a", "dm-0", "sda/", "../sda", "sda ", "sda\n", "abcdefghijklmnopqrstuvwxyzabcdef", "sdá" };
    for (size_t i = 0; i < sizeof bad / sizeof *bad; i++) CHECK(!disk_valid_name(bad[i]), "refuses \"%s\"", bad[i]);
    CHECK(!disk_valid_name(NULL), "refuses NULL");
    CHECK(disk_is_system("dm-0") == 1 && disk_is_system("") == 1, "an invalid name counts as the system disk (refuse to touch it)");
    CHECK(disk_size("nonexistent9") == 0 && disk_mounts("nonexistent9", NULL, 0) == 0, "an unknown device has no size or mounts");

    /* tools: five keys, each 0 or 1 */
    char *tools = disk_tools_json();
    JDoc td;
    char err[160] = "";
    CHECK(tools && !json_parse(tools, strlen(tools), &td, err, sizeof err), "tools JSON parses: %s", err);
    if (tools && td.root) {
        const char *keys[] = { "fat32", "exfat", "ext4", "ntfs", "smartctl" };
        for (size_t i = 0; i < 5; i++) {
            const JVal *v = member(td.root, keys[i], J_NUM);
            CHECK(v && (v->num == 0 || v->num == 1), "tools.%s is 0 or 1", keys[i]);
        }
        json_free(&td);
    }
    free(tools);

    /* the list */
    char *list = disk_list_json();
    JDoc ld;
    CHECK(list && !json_parse(list, strlen(list), &ld, err, sizeof err), "list JSON parses: %s", err);
    int ndisks = 0, nparts = 0;
    char first[32] = "", first_model[256] = "", first_serial[256] = "";
    if (list && ld.root) {
        CHECK(ld.root->type == J_ARR, "the list is an array");
        for (const JVal *d = ld.root->kid; d; d = d->next) {
            const JVal *name = member(d, "name", J_STR), *size = member(d, "size", J_NUM), *parts = member(d, "partitions", J_ARR);
            const JVal *system = member(d, "system", J_BOOL), *removable = member(d, "removable", J_BOOL);
            CHECK(name && size && parts && system && removable, "a disk has name, size, partitions, system and removable");
            if (!name || !size || !parts || !system || !removable) continue;
            ndisks++;
            CHECK(disk_valid_name(name->str), "listed disk %s has a valid name", name->str);
            CHECK(size->num > 0 && (uint64_t)size->num == disk_size(name->str), "%s: size matches disk_size()", name->str);
            CHECK((int)system->num == disk_is_system(name->str), "%s: system flag matches disk_is_system()", name->str);
            CHECK((int)removable->num == disk_is_removable(name->str), "%s: removable flag matches disk_is_removable()", name->str);
            CHECK(member(d, "model", J_STR) && member(d, "vendor", J_STR) && member(d, "serial", J_STR) && member(d, "transport", J_STR) &&
                  member(d, "table", J_STR) && member(d, "fstype", J_STR) && member(d, "label", J_STR) && member(d, "uuid", J_STR) &&
                  member(d, "mountpoint", J_STR) && member(d, "ro", J_BOOL) && member(d, "rotational", J_BOOL), "%s: every disk field is present", name->str);
            if (!first[0]) {
                snprintf(first, sizeof first, "%s", name->str);
                snprintf(first_model, sizeof first_model, "%s", member(d, "model", J_STR) ? member(d, "model", J_STR)->str : "");
                snprintf(first_serial, sizeof first_serial, "%s", member(d, "serial", J_STR) ? member(d, "serial", J_STR)->str : "");
            }
            double last = 0;
            for (const JVal *p = parts->kid; p; p = p->next) {
                const JVal *pn = member(p, "name", J_STR), *num = member(p, "number", J_NUM), *psz = member(p, "size", J_NUM), *st = member(p, "start", J_NUM);
                CHECK(pn && num && psz && st && member(p, "partlabel", J_STR) && member(p, "parttype", J_STR) && member(p, "mountpoint", J_STR),
                      "a partition of %s has its fields", name->str);
                if (!pn || !num || !psz || !st) continue;
                nparts++;
                char parent[32];
                CHECK(disk_parent(pn->str, parent, sizeof parent) == 1 && !strcmp(parent, name->str), "%s belongs to %s", pn->str, name->str);
                CHECK(num->num >= 1 && num->num > last, "%s: partition numbers ascend", pn->str);
                last = num->num;
                CHECK((uint64_t)psz->num == disk_size(pn->str), "%s: size matches disk_size()", pn->str);
                /* the first mount point is the one listed */
                char mounts[2048];
                int n = disk_mounts(pn->str, mounts, sizeof mounts);
                char *nl = strchr(mounts, '\n');
                if (nl) *nl = 0;
                const JVal *mp = member(p, "mountpoint", J_STR);
                CHECK(mp && !strcmp(mp->str, n ? mounts : ""), "%s: mountpoint %s matches disk_mounts()", pn->str, mp ? mp->str : "?");
                /* used/avail/fssize are all numbers or all null, and only a mounted file system has them */
                const JVal *used = json_get(p, "used"), *avail = json_get(p, "avail"), *fssize = json_get(p, "fssize");
                CHECK(used && avail && fssize && used->type == avail->type && avail->type == fssize->type && (used->type == J_NUM || used->type == J_NULL),
                      "%s: used/avail/fssize agree", pn->str);
                if (used && used->type == J_NUM) CHECK(mp && mp->str[0], "%s: space is only reported for a mounted partition", pn->str);
            }
        }
        json_free(&ld);
    }
    free(list);
    printf("disk-test: %d disks, %d partitions on this machine\n", ndisks, nparts);

    /* identity as the list shows it */
    if (first[0]) {
        char model[256], serial[256];
        CHECK(disk_identity(first, model, sizeof model, serial, sizeof serial) == 0, "identity of %s", first);
        CHECK(!strcmp(model, first_model) && !strcmp(serial, first_serial), "%s: identity matches the list (model \"%s\")", first, model);
    }
    CHECK(disk_identity("../x", (char[8]){ 0 }, 8, (char[8]){ 0 }, 8) == -1, "identity refuses an invalid name");

    /* the label and a property of a device that does not exist */
    char buf[64] = "keep";
    CHECK(disk_udev_prop("nonexistent9", "ID_FS_TYPE", buf, sizeof buf) == -1 && !buf[0], "a missing udev entry clears the output");
    memset(buf, 'x', sizeof buf);
    disk_fs_label("nonexistent9", buf, sizeof buf);
    CHECK(!buf[0], "a missing label is empty");
    char tiny[1] = { 'x' };
    disk_fs_label("nonexistent9", tiny, 1);
    CHECK(!tiny[0], "a one-byte buffer holds just the terminator");

    /* notifications: open, nothing to report right away, close (twice is fine) */
    CHECK(disk_watch_fd() < 0, "no watch before it is opened");
    CHECK(disk_watch(1) == 0 && disk_watch_fd() >= 0, "watch opens");
    CHECK(disk_watch(1) == 0, "opening twice is a no-op");
    int ch = disk_watch_dispatch();
    CHECK(ch == 0 || ch == 1, "dispatch answers 0 or 1");
    CHECK(disk_watch(0) == 0 && disk_watch_fd() < 0, "watch closes");
    CHECK(disk_watch(0) == 0 && disk_watch_dispatch() == 0, "closing twice is fine and nothing is dispatched after");

    printf("disk-test: %d checks, %d failures\n", checks, fails);
    return fails ? 1 : 0;
}
