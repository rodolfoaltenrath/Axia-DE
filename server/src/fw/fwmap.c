/* fwmap.c — the firewall's internal hash table (see fwmap.h). */
#include "fw/fwmap.h"

#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static _Atomic uint64_t seed;

/* 64-bit FNV-1a with a seed and a final mix (fmix64 from MurmurHash3). The seed comes from the
 * time, the CPU clock and an address (ASLR): it isn't cryptographic, it's enough so the buckets
 * can't be predicted from outside. */
uint64_t fw_hash(const void *data, size_t len) {
    uint64_t k = atomic_load_explicit(&seed, memory_order_acquire);
    if (!k) {
        /* the first one to arrive sets it; the other threads use that same one */
        uint64_t s = (uint64_t)time(NULL) * 0x9e3779b97f4a7c15ull;
        s ^= (uint64_t)clock() << 17;
        s ^= (uint64_t)(uintptr_t)&k;
        s |= 1;
        uint64_t zero = 0;
        if (atomic_compare_exchange_strong(&seed, &zero, s)) k = s;
        else k = zero;
    }
    const unsigned char *p = data;
    uint64_t h = 0xcbf29ce484222325ull ^ k;
    for (size_t i = 0; i < len; i++) { h ^= p[i]; h *= 0x100000001b3ull; }
    h ^= h >> 33; h *= 0xff51afd7ed558ccdull;
    h ^= h >> 33; h *= 0xc4ceb9fe1a85ec53ull;
    h ^= h >> 33;
    return h;
}

static FwMapEnt **find(const FwMap *m, const void *key, size_t klen, uint64_t h) {
    if (!m->nb) return NULL;
    FwMapEnt **pp = &m->b[h & (m->nb - 1)];
    for (; *pp; pp = &(*pp)->next)
        if ((*pp)->h == h && (*pp)->klen == klen && !memcmp((*pp)->key, key, klen)) return pp;
    return NULL;
}

static int grow(FwMap *m) {
    size_t nb = m->nb ? m->nb * 2 : 16;
    FwMapEnt **b = calloc(nb, sizeof *b);
    if (!b) return -1;
    for (size_t i = 0; i < m->nb; i++) {
        FwMapEnt *e = m->b[i];
        while (e) {
            FwMapEnt *n = e->next;
            e->next = b[e->h & (nb - 1)];
            b[e->h & (nb - 1)] = e;
            e = n;
        }
    }
    free(m->b);
    m->b = b;
    m->nb = nb;
    return 0;
}

void *fwmap_get(const FwMap *m, const void *key, size_t klen) {
    FwMapEnt **pp = find(m, key, klen, fw_hash(key, klen));
    return pp ? (*pp)->val : NULL;
}

void **fwmap_slot(FwMap *m, const void *key, size_t klen, int create) {
    uint64_t h = fw_hash(key, klen);
    FwMapEnt **pp = find(m, key, klen, h);
    if (pp) return &(*pp)->val;
    if (!create) return NULL;
    if (m->n >= m->nb && grow(m)) return NULL;
    FwMapEnt *e = malloc(sizeof *e + klen + 1);
    if (!e) return NULL;
    e->h = h;
    e->val = NULL;
    e->klen = klen;
    memcpy(e->key, key, klen);
    e->key[klen] = 0;
    e->next = m->b[h & (m->nb - 1)];
    m->b[h & (m->nb - 1)] = e;
    m->n++;
    return &e->val;
}

void *fwmap_take(FwMap *m, const void *key, size_t klen) {
    FwMapEnt **pp = find(m, key, klen, fw_hash(key, klen));
    if (!pp) return NULL;
    FwMapEnt *e = *pp;
    void *v = e->val;
    *pp = e->next;
    free(e);
    m->n--;
    return v;
}

void fwmap_free(FwMap *m, void (*free_val)(void *)) {
    for (size_t i = 0; i < m->nb; i++) {
        FwMapEnt *e = m->b[i];
        while (e) {
            FwMapEnt *n = e->next;
            if (free_val) free_val(e->val);
            free(e);
            e = n;
        }
    }
    free(m->b);
    m->b = NULL;
    m->nb = m->n = 0;
}
