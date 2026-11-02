/* mmdb.h — minimal reader for MaxMind DB files (.mmdb) for IP → country (see mmdb.c).
 *
 * Read-only: the whole file goes into memory and every access is checked against its length, so
 * a broken or tampered file gives "not found", never a read outside the buffer. */
#ifndef LORD_MMDB_H
#define LORD_MMDB_H

#include <stddef.h>
#include <stdint.h>

#define MMDB_MAX_FILE (64u << 20)

typedef struct {
    uint8_t *buf;                       /* the file (owned if owned) */
    size_t len;
    int owned;
    uint32_t node_count;
    uint32_t record_size;               /* 24, 28 or 32 */
    uint32_t ip_version;                /* 4 or 6 */
    uint64_t build_epoch;
    size_t tree_size;                   /* tree bytes */
    size_t data_off, data_len;          /* data section */
    uint32_t ipv4_start;                /* node of ::/96 in an IPv6 tree */
    char type[64];                      /* database_type */
} Mmdb;

/* Opens a .mmdb file (max. MMDB_MAX_FILE). 0 if valid. */
int mmdb_open(Mmdb *m, const char *path);
/* The same from memory (doesn't copy or free it). */
int mmdb_open_mem(Mmdb *m, const uint8_t *buf, size_t len);
void mmdb_close(Mmdb *m);

/* Country of an address (family 4: 4 bytes, 6: 16 bytes) in cc[3] ("AR").
 * 1 = found, 0 = no data, -1 = invalid file or bad parameters. */
int mmdb_country(const Mmdb *m, const uint8_t *addr, int family, char cc[3]);

#endif
