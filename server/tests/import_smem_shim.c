/* import_smem_shim.c — the smem.h API on top of malloc, only for the importer tests:
 * this way AddressSanitizer and LeakSanitizer see every block (the real arena uses its own mmap and
 * overflows inside a chunk would go undetected). It wipes on free, like the real one. */
#include "vault/smem.h"

#include <malloc.h>
#include <stdlib.h>
#include <string.h>

void sm_wipe(void *p, size_t n) { if (p && n) explicit_bzero(p, n); }
void sm_harden_process(void) {}
void *sm_alloc(size_t n) { return calloc(1, n ? n : 1); }
size_t sm_size(const void *p) { return p ? malloc_usable_size((void *)p) : 0; }
void sm_free(void *p) { if (p) { explicit_bzero(p, malloc_usable_size(p)); free(p); } }
void *sm_realloc(void *p, size_t n) {
    if (!p) return sm_alloc(n);
    size_t have = malloc_usable_size(p);
    if (n <= have) return p;
    void *q = sm_alloc(n);
    if (!q) return NULL;
    memcpy(q, p, have);
    sm_free(p);
    return q;
}
char *sm_strndup(const char *s, size_t n) { if (!s) return NULL; char *d = sm_alloc(n + 1); if (d) { memcpy(d, s, n); d[n] = 0; } return d; }
char *sm_strdup(const char *s) { return s ? sm_strndup(s, strlen(s)) : NULL; }
void sm_wipe_all(void) {}
void sm_stats(SmStats *st) { memset(st, 0, sizeof *st); }
