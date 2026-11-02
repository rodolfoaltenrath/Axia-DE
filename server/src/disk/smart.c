/* smart.c — SMART parsers (pure functions, no I/O; test: tests/smart_test.c).
 *
 * ATA (ACS-3, "SMART READ DATA" 0xB0/0xD0 and "READ THRESHOLDS" 0xB0/0xD1, 512 bytes each):
 *   bytes 0-1 version, 2-361: 30 attributes of 12 bytes
 *     [0] id, [1-2] flags (bit 0 = pre-fail), [3] normalized value, [4] worst, [5-10] raw
 *     (48 bits, little endian), [11] reserved;
 *   byte 511: checksum (the 512 bytes sum to 0 modulo 256).
 *   Thresholds: same layout, [0] id, [1] threshold.
 * NVMe (1.4, log page 02h "SMART / Health Information", 512 bytes):
 *   [0] critical warning, [1-2] temperature (kelvin), [3] available spare %, [4] spare
 *   threshold, [5] % life used, [32-47] data units read, [48-63] written (units of 512 000
 *   bytes), [112-127] power cycles, [128-143] power-on hours, [144-159] unsafe shutdowns,
 *   [160-175] media errors (128-bit integers; the low 64 bits are used here). */
#include "disk.h"

#include <stdio.h>
#include <string.h>

static uint64_t le(const uint8_t *p, int n) {
    uint64_t v = 0;
    for (int i = n - 1; i >= 0; i--) v = (v << 8) | p[i];
    return v;
}

/* 128 bits → 64 (saturates if the high half is not zero) */
static uint64_t le128(const uint8_t *p) {
    if (le(p + 8, 8)) return UINT64_MAX;
    return le(p, 8);
}

static void clear(SmartReport *r, int kind) {
    memset(r, 0, sizeof *r);
    r->kind = kind;
    r->status = -1;
    r->temp_c = -1;
    r->power_on_hours = r->power_cycles = -1;
    r->reallocated = r->pending = r->uncorrectable = -1;
    r->spare = r->spare_thresh = r->used_pct = -1;
}

int smart_ata_parse(const uint8_t data[512], const uint8_t *thresh, int status, SmartReport *r) {
    clear(r, SMART_ATA);
    r->status = status;
    uint8_t sum = 0;
    for (int i = 0; i < 512; i++) sum = (uint8_t)(sum + data[i]);
    r->checksum_ok = sum == 0;
    int failing = 0;
    for (int i = 0; i < 30; i++) {
        const uint8_t *a = data + 2 + i * 12;
        if (!a[0]) continue;
        SmartAttr *x = &r->attr[r->nattr++];
        x->id = a[0];
        x->flags = (uint16_t)le(a + 1, 2);
        x->value = a[3];
        x->worst = a[4];
        x->raw = le(a + 5, 6);
        if (thresh) {
            for (int k = 0; k < 30; k++) {
                const uint8_t *t = thresh + 2 + k * 12;
                if (t[0] == x->id) { x->thresh = t[1]; break; }
            }
        }
        /* value 0 or 0xFE/0xFF = no data (some disks); threshold 0 never fails */
        x->failing = x->thresh && x->value && x->value <= x->thresh && x->value < 0xfe;
        if (x->failing && (x->flags & 1)) failing = 1;
        switch (x->id) {
        case 5: r->reallocated = (int64_t)(x->raw & 0xffffffffu); break;
        case 9: r->power_on_hours = (int64_t)(x->raw & 0xffffffffu); break;
        case 12: r->power_cycles = (int64_t)(x->raw & 0xffffffffu); break;
        case 194: r->temp_c = (int)(x->raw & 0xff); break;
        case 190: if (r->temp_c < 0) r->temp_c = (int)(x->raw & 0xff); break;
        case 197: r->pending = (int64_t)(x->raw & 0xffffffffu); break;
        case 198: r->uncorrectable = (int64_t)(x->raw & 0xffffffffu); break;
        default: break;
        }
    }
    if (status == 0 || failing) r->health = SMART_FAIL;
    else if (r->reallocated > 0 || r->pending > 0 || r->uncorrectable > 0) r->health = SMART_WARN;
    else if (status == 1 || r->nattr) r->health = r->checksum_ok ? SMART_OK : SMART_WARN;
    else r->health = SMART_UNKNOWN;
    return r->nattr || status >= 0 ? 0 : -1;
}

int smart_nvme_parse(const uint8_t log[512], SmartReport *r) {
    clear(r, SMART_NVME);
    r->checksum_ok = 1;
    r->crit_warning = log[0];
    unsigned k = (unsigned)le(log + 1, 2);
    r->temp_c = k ? (int)k - 273 : -1;
    r->spare = log[3];
    r->spare_thresh = log[4];
    r->used_pct = log[5];
    uint64_t ur = le128(log + 32), uw = le128(log + 48);
    r->data_read = ur > UINT64_MAX / 512000 ? UINT64_MAX : ur * 512000;
    r->data_written = uw > UINT64_MAX / 512000 ? UINT64_MAX : uw * 512000;
    uint64_t cyc = le128(log + 112), hrs = le128(log + 128);
    r->power_cycles = cyc > INT64_MAX ? INT64_MAX : (int64_t)cyc;
    r->power_on_hours = hrs > INT64_MAX ? INT64_MAX : (int64_t)hrs;
    r->unsafe_shutdowns = le128(log + 144);
    r->media_errors = le128(log + 160);
    /* warning bits: 0 low spare, 1 temperature, 2 degraded reliability, 3 read-only,
     * 4 volatile backup failed */
    if (r->crit_warning & 0x0d) r->health = SMART_FAIL;
    else if (r->crit_warning || r->used_pct >= 100 || r->spare < r->spare_thresh || r->media_errors) r->health = SMART_WARN;
    else r->health = SMART_OK;
    return 0;
}

int smart_summary_json(const SmartReport *r, char *out, size_t n) {
    static const char *const h[] = { "unknown", "ok", "warning", "failing" };
    int k = snprintf(out, n, "{\"kind\":\"%s\",\"health\":\"%s\",\"checksum\":%s,\"status\":%d,\"temp\":%d,\"hours\":%lld,\"cycles\":%lld",
                     r->kind == SMART_NVME ? "nvme" : "ata", h[r->health & 3], r->checksum_ok ? "true" : "false", r->status, r->temp_c,
                     (long long)r->power_on_hours, (long long)r->power_cycles);
    if (k < 0 || (size_t)k >= n) return -1;
    size_t o = (size_t)k;
    if (r->kind == SMART_ATA)
        k = snprintf(out + o, n - o, ",\"reallocated\":%lld,\"pending\":%lld,\"uncorrectable\":%lld,\"attributes\":%d}",
                     (long long)r->reallocated, (long long)r->pending, (long long)r->uncorrectable, r->nattr);
    else
        k = snprintf(out + o, n - o, ",\"critical\":%d,\"spare\":%d,\"spareThreshold\":%d,\"used\":%d,\"read\":%llu,\"written\":%llu,"
                     "\"mediaErrors\":%llu,\"unsafeShutdowns\":%llu}", r->crit_warning, r->spare, r->spare_thresh, r->used_pct,
                     (unsigned long long)r->data_read, (unsigned long long)r->data_written, (unsigned long long)r->media_errors,
                     (unsigned long long)r->unsafe_shutdowns);
    return k < 0 || (size_t)k >= n - o ? -1 : 0;
}

int smart_attr_json(const SmartAttr *a, char *out, size_t n) {
    int k = snprintf(out, n, "{\"id\":%u,\"value\":%u,\"worst\":%u,\"thresh\":%u,\"raw\":%llu,\"prefail\":%s,\"failing\":%s}",
                     a->id, a->value, a->worst, a->thresh, (unsigned long long)a->raw, (a->flags & 1) ? "true" : "false",
                     a->failing ? "true" : "false");
    return k < 0 || (size_t)k >= n ? -1 : 0;
}
