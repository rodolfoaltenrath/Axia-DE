/* smartctl_test.c — the smart-* actions' pure helpers (src/disk/smartctl.c): which device names
 * reach smartctl, the JSON compaction and the LOG pieces.
 *
 *   make -C server build/smartctl-test && ./server/build/smartctl-test   (in `make test`) */
#include "../src/disk/disk.h"

#include <stdio.h>
#include <string.h>

static int fails, checks;
#define CHECK(c, ...) do { checks++; if (!(c)) { fails++; printf("✗ " __VA_ARGS__); printf("\n"); } } while (0)

int main(void) {
    const char *good[] = { "sda", "sdb", "sdz", "sdaa", "sdzzz", "hda", "nvme0n1", "nvme1n1", "nvme12n3", "nvme0n128" };
    for (size_t i = 0; i < sizeof good / sizeof *good; i++) CHECK(smartctl_valid_disk(good[i]), "accepts %s", good[i]);
    const char *bad[] = { "", "sd", "sda1", "sdA", "sdaaaa", "nvme0", "nvme0n0", "nvme0n1p1", "nvme00n1", "nvme0n01", "nvmen1",
        "nvme1234n1", "nvme0n1234", "loop0", "dm-0", "md0", "zram0", "mmcblk0", "mmcblk0p1", "sr0", "vda", "xvda", "../sda",
        "sda/", "/dev/sda", "sda ", " sda", "sd-a", "sda\n", "nvme0n1\n", "hd1", "sdá" };
    for (size_t i = 0; i < sizeof bad / sizeof *bad; i++) CHECK(!smartctl_valid_disk(bad[i]), "refuses \"%s\"", bad[i]);
    CHECK(!smartctl_valid_disk(NULL), "refuses NULL");

    /* compaction keeps strings (spaces, escapes, an escaped quote) and drops the rest */
    char j[] = "{\n  \"model_name\": \"QEMU HARDDISK\",\n  \"x\": [\n    1,\n    2\n  ],\n  \"s\": \"a \\\"b\\\" \\\\ c\"\t}\n";
    size_t n = smartctl_compact(j, strlen(j));
    const char *want = "{\"model_name\":\"QEMU HARDDISK\",\"x\":[1,2],\"s\":\"a \\\"b\\\" \\\\ c\"}";
    CHECK(n == strlen(want) && !strcmp(j, want), "compact: got %s", j);
    char raw[] = "{\"a\":\"x\ty\"}";                      /* a raw tab inside a string becomes a space */
    smartctl_compact(raw, strlen(raw));
    CHECK(!strcmp(raw, "{\"a\":\"x y\"}"), "raw control: got %s", raw);

    /* pieces never cut a UTF-8 sequence */
    const char *u = "ab\xc3\xa9" "cd";                    /* "abécd" */
    CHECK(smartctl_chunk(u, 6, 3) == 2, "chunk before é: %zu", smartctl_chunk(u, 6, 3));
    CHECK(smartctl_chunk(u, 6, 4) == 4, "chunk after é: %zu", smartctl_chunk(u, 6, 4));
    CHECK(smartctl_chunk(u, 6, 900) == 6, "short text in one piece");

    printf("smartctl-test: %d checks, %d failures\n", checks, fails);
    return fails ? 1 : 0;
}
