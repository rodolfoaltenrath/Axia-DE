/* fwmap.h — chained hash table, internal to the firewall (rules.c, dnsmap.c). Byte keys with a
 * length, one void* value per key and a hash seeded per process (so nobody can deliberately
 * craft keys that all land in the same bucket). Grows by doubling. */
#ifndef LORD_FW_FWMAP_H
#define LORD_FW_FWMAP_H

#include <stddef.h>
#include <stdint.h>

typedef struct FwMapEnt {
    struct FwMapEnt *next;
    uint64_t h;
    void *val;
    size_t klen;
    unsigned char key[];
} FwMapEnt;

typedef struct {
    FwMapEnt **b;
    size_t nb, n;
} FwMap;

uint64_t fw_hash(const void *data, size_t len);

/* Value of the key, or NULL. */
void  *fwmap_get(const FwMap *m, const void *key, size_t klen);
/* Slot of the value; with create, creates the key (value NULL) if missing. NULL if missing or out of memory. */
void **fwmap_slot(FwMap *m, const void *key, size_t klen, int create);
/* Removes the key and returns its value (NULL if it wasn't there). */
void  *fwmap_take(FwMap *m, const void *key, size_t klen);
/* Frees the table; free_val (optional) receives each value. */
void   fwmap_free(FwMap *m, void (*free_val)(void *));

#define FWMAP_EACH(m, e) \
    for (size_t fwm_i_ = 0; fwm_i_ < (m)->nb; fwm_i_++) \
        for (FwMapEnt *e = (m)->b[fwm_i_], *fwm_n_ = e ? e->next : NULL; e; e = fwm_n_, fwm_n_ = e ? e->next : NULL)

#endif
