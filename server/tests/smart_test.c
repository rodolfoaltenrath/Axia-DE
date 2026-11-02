/* smart_test.c — the SMART parsers (src/disk/smart.c) with hand-built data: healthy ATA, ATA
 * with reallocated sectors, ATA with a pre-fail attribute below its threshold, a broken checksum,
 * RETURN STATUS "threshold exceeded", and the NVMe health log. */
#include "disk/disk.h"

#include <stdio.h>
#include <string.h>

static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #c); fails++; } } while (0)

/* attribute i: id, flags, value, worst, raw */
static void put_attr(uint8_t *d, int i, uint8_t id, uint16_t flags, uint8_t val, uint8_t worst, uint64_t raw) {
    uint8_t *a = d + 2 + i * 12;
    a[0] = id;
    a[1] = (uint8_t)flags;
    a[2] = (uint8_t)(flags >> 8);
    a[3] = val;
    a[4] = worst;
    for (int k = 0; k < 6; k++) a[5 + k] = (uint8_t)(raw >> (8 * k));
}

static void put_thresh(uint8_t *t, int i, uint8_t id, uint8_t th) {
    t[2 + i * 12] = id;
    t[3 + i * 12] = th;
}

static void checksum(uint8_t *d) {
    uint8_t s = 0;
    for (int i = 0; i < 511; i++) s = (uint8_t)(s + d[i]);
    d[511] = (uint8_t)(0x100 - s);
}

static void base(uint8_t *d, uint8_t *t) {
    memset(d, 0, 512);
    memset(t, 0, 512);
    d[0] = 0x10;                                    /* version */
    put_attr(d, 0, 1, 0x000b, 100, 100, 0);         /* Raw_Read_Error_Rate, pre-fail */
    put_attr(d, 1, 5, 0x0033, 100, 100, 0);         /* Reallocated_Sector_Ct */
    put_attr(d, 2, 9, 0x0032, 99, 99, 12345);       /* Power_On_Hours */
    put_attr(d, 3, 12, 0x0032, 100, 100, 678);      /* Power_Cycle_Count */
    put_attr(d, 4, 194, 0x0022, 64, 50, 0x0000002a00140024ull);   /* 36 °C (+ min/max in the upper bytes) */
    put_attr(d, 5, 197, 0x0012, 100, 100, 0);
    put_attr(d, 6, 198, 0x0010, 100, 100, 0);
    put_thresh(t, 0, 1, 6);
    put_thresh(t, 1, 5, 36);
    put_thresh(t, 2, 9, 0);
    put_thresh(t, 3, 12, 0);
    put_thresh(t, 4, 194, 0);
    checksum(d);
    checksum(t);
}

int main(void) {
    uint8_t d[512], t[512];
    SmartReport r;
    char j[1024];

    /* healthy */
    base(d, t);
    CHECK(smart_ata_parse(d, t, 1, &r) == 0);
    CHECK(r.kind == SMART_ATA && r.nattr == 7 && r.checksum_ok);
    CHECK(r.health == SMART_OK);
    CHECK(r.temp_c == 36);
    CHECK(r.power_on_hours == 12345 && r.power_cycles == 678);
    CHECK(r.reallocated == 0 && r.pending == 0 && r.uncorrectable == 0);
    CHECK(r.attr[1].id == 5 && r.attr[1].thresh == 36 && !r.attr[1].failing && (r.attr[1].flags & 1));
    CHECK(smart_summary_json(&r, j, sizeof j) == 0 && strstr(j, "\"health\":\"ok\"") && strstr(j, "\"temp\":36"));
    CHECK(smart_attr_json(&r.attr[2], j, sizeof j) == 0 && strstr(j, "\"raw\":12345") && strstr(j, "\"prefail\":false"));

    /* no thresholds and no status: still parsed */
    CHECK(smart_ata_parse(d, NULL, -1, &r) == 0 && r.health == SMART_OK && r.attr[1].thresh == 0);

    /* reallocated and pending sectors: warning */
    base(d, t);
    put_attr(d, 1, 5, 0x0033, 98, 98, 24);
    put_attr(d, 5, 197, 0x0012, 100, 100, 3);
    checksum(d);
    CHECK(smart_ata_parse(d, t, 1, &r) == 0 && r.health == SMART_WARN && r.reallocated == 24 && r.pending == 3);

    /* pre-fail attribute below its threshold: failing, even if RETURN STATUS says good */
    base(d, t);
    put_attr(d, 1, 5, 0x0033, 30, 30, 4000);
    checksum(d);
    CHECK(smart_ata_parse(d, t, 1, &r) == 0 && r.health == SMART_FAIL && r.attr[1].failing);

    /* a non-pre-fail (old-age) attribute below its threshold does not make it fail */
    base(d, t);
    put_attr(d, 3, 12, 0x0032, 1, 1, 9);
    put_thresh(t, 3, 12, 20);
    checksum(d);
    checksum(t);
    CHECK(smart_ata_parse(d, t, 1, &r) == 0 && r.attr[3].failing && r.health == SMART_OK);

    /* RETURN STATUS: threshold exceeded */
    base(d, t);
    CHECK(smart_ata_parse(d, t, 0, &r) == 0 && r.health == SMART_FAIL);

    /* broken checksum: warning */
    base(d, t);
    d[100] ^= 0x55;
    CHECK(smart_ata_parse(d, t, -1, &r) == 0 && !r.checksum_ok && r.health == SMART_WARN);

    /* temperature from 190 when 194 is missing */
    base(d, t);
    put_attr(d, 4, 190, 0x0022, 60, 50, 40);
    checksum(d);
    CHECK(smart_ata_parse(d, t, 1, &r) == 0 && r.temp_c == 40);

    /* nothing: no data and no status */
    memset(d, 0, sizeof d);
    CHECK(smart_ata_parse(d, NULL, -1, &r) == -1 && r.health == SMART_UNKNOWN);

    /* healthy NVMe */
    uint8_t n[512] = { 0 };
    n[1] = (uint8_t)(310 & 0xff); n[2] = (uint8_t)(310 >> 8);      /* 310 K = 37 °C */
    n[3] = 100; n[4] = 10; n[5] = 3;
    n[32] = 0x10; n[33] = 0x27;                                     /* 10000 units read */
    n[48] = 0x88; n[49] = 0x13;                                     /* 5000 written */
    n[112] = 42;                                                    /* cycles */
    n[128] = 0x39; n[129] = 0x05;                                   /* 1337 hours */
    n[144] = 7;                                                     /* unsafe shutdowns */
    CHECK(smart_nvme_parse(n, &r) == 0);
    CHECK(r.kind == SMART_NVME && r.health == SMART_OK && r.temp_c == 37 && r.spare == 100 && r.used_pct == 3);
    CHECK(r.data_read == 10000ull * 512000 && r.data_written == 5000ull * 512000);
    CHECK(r.power_cycles == 42 && r.power_on_hours == 1337 && r.unsafe_shutdowns == 7 && r.media_errors == 0);
    CHECK(smart_summary_json(&r, j, sizeof j) == 0 && strstr(j, "\"kind\":\"nvme\"") && strstr(j, "\"hours\":1337"));
    /* spare below threshold: warning; degraded reliability (bit 2): failing */
    n[3] = 5;
    CHECK(smart_nvme_parse(n, &r) == 0 && r.health == SMART_WARN);
    n[3] = 100; n[0] = 0x04;
    CHECK(smart_nvme_parse(n, &r) == 0 && r.health == SMART_FAIL);
    /* temperature only (bit 1): warning */
    n[0] = 0x02;
    CHECK(smart_nvme_parse(n, &r) == 0 && r.health == SMART_WARN);
    /* media errors */
    n[0] = 0; n[160] = 1;
    CHECK(smart_nvme_parse(n, &r) == 0 && r.health == SMART_WARN && r.media_errors == 1);
    /* huge 128-bit counter: saturates, does not wrap */
    n[160] = 0; n[32 + 9] = 1;
    CHECK(smart_nvme_parse(n, &r) == 0 && r.data_read == UINT64_MAX);

    if (fails) { fprintf(stderr, "smart-test: %d failures\n", fails); return 1; }
    printf("smart-test: ok\n");
    return 0;
}
