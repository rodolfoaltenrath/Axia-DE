/* mmdb_test.c — test of the MaxMind DB reader (src/monitor/mmdb.c).
 *
 *   build/mmdb-test [file.mmdb[.xz]] [original.mmdb]
 *   (default ../assets/monitor/geoip-country.mmdb.xz; with a second file, for example the full
 *   dbip-country-lite.mmdb, it compares 200 000 random addresses and shows how many match)
 *
 * Looks up known addresses, then breaks the file in several ways (truncated, random
 * bytes, bogus pointers and sizes) to check that the reader never reads outside the buffer: it is
 * also run with -fsanitize=address during development. Exits with 1 if anything fails. */
#include "monitor/mmdb.h"

#include <arpa/inet.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails;

static void expect(const Mmdb *m, const char *ip, const char *want) {
    uint8_t a[16];
    int fam = strchr(ip, ':') ? 6 : 4;
    if (inet_pton(fam == 4 ? AF_INET : AF_INET6, ip, a) != 1) { printf("✗ %s: invalid address\n", ip); fails++; return; }
    char cc[3];
    int r = mmdb_country(m, a, fam, cc);
    const char *got = r == 1 ? cc : "";
    if (strcmp(got, want)) { printf("✗ %s → '%s' (esperado '%s', r=%d)\n", ip, got, want, r); fails++; }
    else printf("✓ %s → %s\n", ip, want[0] ? want : "(no data)");
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "../assets/monitor/geoip-country.mmdb.xz";
    Mmdb m;
    if (mmdb_open(&m, path)) { printf("✗ could not open %s\n", path); return 1; }
    printf("%s: %s, %u nodes, %u-bit records, IPv%u\n", path, m.type, m.node_count, m.record_size, m.ip_version);
    expect(&m, "8.8.8.8", "US");                  /* Google DNS */
    expect(&m, "1.1.1.1", "AU");                  /* APNIC/Cloudflare: AU in DB-IP */
    expect(&m, "200.45.0.1", "AR");               /* Telecom Argentina */
    expect(&m, "177.10.0.1", "BR");
    expect(&m, "193.0.6.139", "NL");              /* RIPE NCC, Amsterdam */
    expect(&m, "210.130.0.1", "JP");
    expect(&m, "2800:810::1", "AR");              /* LACNIC → Argentina */
    expect(&m, "2a01:4f8::1", "DE");              /* Hetzner */
    expect(&m, "10.0.0.1", "");                   /* private: no data */
    expect(&m, "127.0.0.1", "");
    expect(&m, "::1", "");

    /* broken files: no lookup may read outside the buffer */
    uint8_t *copy = malloc(m.len);
    const char *probe[] = { "8.8.8.8", "200.45.0.1", "2800:810::1", "193.0.6.139" };
    srand(1234);
    int opened = 0;
    for (int round = 0; round < 400; round++) {
        size_t len = m.len;
        memcpy(copy, m.buf, len);
        if (round % 4 == 0) len = (size_t)rand() % len;                         /* cortado */
        else for (int k = 0; k < 64; k++) copy[(size_t)rand() % len] = (uint8_t)rand();
        if (round % 4 == 3) {                                                     /* metadatos falsos */
            for (size_t k = len - 200; k < len; k++) copy[k] = (uint8_t)rand();
        }
        uint8_t *exact = malloc(len ? len : 1);                                  /* no slack: ASan reports */
        memcpy(exact, copy, len);
        Mmdb b;
        if (!mmdb_open_mem(&b, exact, len)) {
            opened++;
            for (int k = 0; k < 4; k++) {
                uint8_t a[16];
                int fam = strchr(probe[k], ':') ? 6 : 4;
                inet_pton(fam == 4 ? AF_INET : AF_INET6, probe[k], a);
                char cc[3];
                mmdb_country(&b, a, fam, cc);
            }
            for (int k = 0; k < 200; k++) {                                       /* random addresses */
                uint8_t a[16];
                for (int j = 0; j < 16; j++) a[j] = (uint8_t)rand();
                char cc[3];
                mmdb_country(&b, a, k & 1 ? 6 : 4, cc);
            }
        }
        free(exact);
    }
    printf("✓ 400 broken files with no reads outside the buffer (%d still opened)\n", opened);
    /* bad parameters */
    char cc[3];
    uint8_t a[16] = { 0 };
    if (mmdb_country(&m, a, 5, cc) != -1 || mmdb_country(NULL, a, 4, cc) != -1) { printf("✗ bad parameters accepted\n"); fails++; }
    free(copy);
    if (argc > 2) {                                   /* accuracy against the full file */
        Mmdb full;
        if (mmdb_open(&full, argv[2])) { printf("✗ could not open %s\n", argv[2]); return 1; }
        long same[2] = { 0 }, total[2] = { 0 };
        for (int k = 0; k < 200000; k++) {
            uint8_t a[16];
            int fam = k & 1 ? 6 : 4;
            for (int j = 0; j < 16; j++) a[j] = (uint8_t)rand();
            if (fam == 6) { a[0] = 0x20 | (a[0] & 0x0f); a[1] &= 0x0f; }  /* 2000::/12: the allocated part */
            char x[3], y[3];
            int rf = mmdb_country(&full, a, fam, y);
            if (rf != 1) continue;
            total[fam == 6]++;
            if (mmdb_country(&m, a, fam, x) == 1 && !strcmp(x, y)) same[fam == 6]++;
        }
        printf("matches with %s: IPv4 %.2f %% of %ld, IPv6 %.2f %% of %ld\n", argv[2],
               100.0 * same[0] / (total[0] ? total[0] : 1), total[0], 100.0 * same[1] / (total[1] ? total[1] : 1), total[1]);
        mmdb_close(&full);
    }
    mmdb_close(&m);
    printf(fails ? "mmdb: %d errors\n" : "mmdb: all good\n", fails);
    return fails ? 1 : 0;
}
