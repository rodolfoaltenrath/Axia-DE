/* smem.c — memory arena for secrets (see smem.h). */
#include "smem.h"

#include <pthread.h>
#include <stdint.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <unistd.h>

#ifndef MADV_WIPEONFORK
#define MADV_WIPEONFORK 18
#endif

#define NCLASS 9                      /* 16, 32, …, 4096 usable bytes */
#define CHUNK  (256u * 1024)          /* small blocks come from 256 KB chunks */
#define MAGIC_SMALL 0x534d454du
#define MAGIC_BIG   0x534d4542u

typedef struct Hdr { uint32_t magic, cls; size_t size; } Hdr;     /* 16 bytes before the block */
typedef struct Free { struct Free *next; } Free;
typedef struct Chunk { struct Chunk *next; size_t size, used; int locked; } Chunk;
typedef struct Big { struct Big *next, *prev; size_t map; int locked; } Big;   /* + Hdr + data */

static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static Chunk *chunks;
static Free *freel[NCLASS];
static Big *bigs;
static size_t in_use, mapped, locked_bytes;
static int unlocked_maps;

void sm_wipe(void *p, size_t n) { if (p && n) explicit_bzero(p, n); }

void sm_harden_process(void) {
    struct rlimit rl = { 0, 0 };
    setrlimit(RLIMIT_CORE, &rl);
    prctl(PR_SET_DUMPABLE, 0, 0, 0, 0);
    /* use as much mlock as the system allows (the soft limit is raised up to the hard one) */
    if (getrlimit(RLIMIT_MEMLOCK, &rl) == 0 && rl.rlim_cur < rl.rlim_max) { rl.rlim_cur = rl.rlim_max; setrlimit(RLIMIT_MEMLOCK, &rl); }
}

static void *map_secure(size_t n, int *locked) {
    void *p = mmap(NULL, n, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) return NULL;
    madvise(p, n, MADV_DONTDUMP);
    madvise(p, n, MADV_WIPEONFORK);
    *locked = mlock(p, n) == 0;
    mapped += n;
    if (*locked) locked_bytes += n; else unlocked_maps++;
    return p;
}

static void unmap_secure(void *p, size_t n, int locked) {
    explicit_bzero(p, n);
    if (locked) { munlock(p, n); locked_bytes -= n; } else unlocked_maps--;
    munmap(p, n);
    mapped -= n;
}

static int class_of(size_t n) {
    size_t c = 16;
    for (int i = 0; i < NCLASS; i++, c <<= 1) if (n <= c) return i;
    return -1;
}

static void *alloc_small(int cls) {
    size_t sz = (size_t)16 << cls;
    if (freel[cls]) {
        Free *f = freel[cls];
        freel[cls] = f->next;
        f->next = NULL;                       /* already zero except for the link */
        return f;
    }
    size_t need = sizeof(Hdr) + sz;
    Chunk *c = chunks;
    if (!c || c->size - c->used < need) {
        int lk;
        Chunk *n = map_secure(CHUNK, &lk);
        if (!n) return NULL;
        n->size = CHUNK;
        n->used = (sizeof(Chunk) + 15) & ~(size_t)15;
        n->locked = lk;
        n->next = chunks;
        chunks = c = n;
    }
    Hdr *h = (Hdr *)((char *)c + c->used);
    c->used += need;
    h->magic = MAGIC_SMALL;
    h->cls = (uint32_t)cls;
    h->size = sz;
    return h + 1;
}

void *sm_alloc(size_t n) {
    if (n == 0) n = 1;
    if (n > ((size_t)1 << 40)) return NULL;
    pthread_mutex_lock(&mu);
    void *r;
    int cls = class_of(n);
    if (cls >= 0) {
        r = alloc_small(cls);
        if (r) in_use += (size_t)16 << cls;
    } else {
        long pg = sysconf(_SC_PAGESIZE);
        size_t ps = pg > 0 ? (size_t)pg : 4096;
        size_t map = (sizeof(Big) + sizeof(Hdr) + n + ps - 1) / ps * ps;
        int lk;
        Big *b = map_secure(map, &lk);
        r = NULL;
        if (b) {
            b->map = map;
            b->locked = lk;
            b->prev = NULL;
            b->next = bigs;
            if (bigs) bigs->prev = b;
            bigs = b;
            Hdr *h = (Hdr *)(b + 1);
            h->magic = MAGIC_BIG;
            h->cls = 0;
            h->size = map - sizeof(Big) - sizeof(Hdr);
            r = h + 1;
            in_use += h->size;
        }
    }
    pthread_mutex_unlock(&mu);
    return r;
}

size_t sm_size(const void *p) {
    if (!p) return 0;
    const Hdr *h = (const Hdr *)p - 1;
    return h->size;
}

void sm_free(void *p) {
    if (!p) return;
    Hdr *h = (Hdr *)p - 1;
    pthread_mutex_lock(&mu);
    if (h->magic == MAGIC_SMALL && h->cls < NCLASS) {
        explicit_bzero(p, h->size);
        in_use -= h->size;
        Free *f = p;
        f->next = freel[h->cls];
        freel[h->cls] = f;
    } else if (h->magic == MAGIC_BIG) {
        Big *b = (Big *)h - 1;
        in_use -= h->size;
        if (b->prev) b->prev->next = b->next; else bigs = b->next;
        if (b->next) b->next->prev = b->prev;
        unmap_secure(b, b->map, b->locked);
    }
    pthread_mutex_unlock(&mu);
}

void *sm_realloc(void *p, size_t n) {
    if (!p) return sm_alloc(n);
    size_t have = sm_size(p);
    if (n <= have) return p;
    void *q = sm_alloc(n);
    if (!q) return NULL;
    memcpy(q, p, have);
    sm_free(p);
    return q;
}

char *sm_strndup(const char *s, size_t n) {
    if (!s) return NULL;
    char *d = sm_alloc(n + 1);
    if (d) { memcpy(d, s, n); d[n] = 0; }
    return d;
}

char *sm_strdup(const char *s) { return s ? sm_strndup(s, strlen(s)) : NULL; }

void sm_wipe_all(void) {
    pthread_mutex_lock(&mu);
    while (chunks) { Chunk *c = chunks; chunks = c->next; size_t sz = c->size; int lk = c->locked; unmap_secure(c, sz, lk); }
    while (bigs) { Big *b = bigs; bigs = b->next; unmap_secure(b, b->map, b->locked); }
    memset(freel, 0, sizeof freel);
    in_use = 0;
    pthread_mutex_unlock(&mu);
}

void sm_stats(SmStats *st) {
    pthread_mutex_lock(&mu);
    st->in_use = in_use;
    st->mapped = mapped;
    st->locked = locked_bytes;
    st->all_locked = unlocked_maps == 0;
    pthread_mutex_unlock(&mu);
}
