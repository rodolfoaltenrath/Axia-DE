/* ss_memback.c — in-memory keyring for the Secret Service tests. */
#include "ss_memback.h"
#include "vault/smem.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static double now_ms(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1e3 + t.tv_nsec / 1e6; }

void memback_init(MemBack *b, int locked) { memset(b, 0, sizeof *b); b->locked = locked; }

static MbItem *find(MemBack *b, const char *id) {
    for (int i = 0; i < MB_MAX; i++) if (b->it[i].used && !strcmp(b->it[i].id, id)) return &b->it[i];
    return NULL;
}
static int has(const MbItem *x, const char *k, const char *v) {
    for (int i = 0; i < x->n; i++) if (!strcmp(x->names[i], k) && !strcmp(x->values[i], v)) return 1;
    return 0;
}
static void clear_attrs(MbItem *x) {
    for (int i = 0; i < x->n; i++) { free(x->names[i]); free(x->values[i]); }
    free(x->names); free(x->values);
    x->names = x->values = NULL; x->n = 0;
}
static void set_attrs_(MbItem *x, const SsAttr *a, int n) {
    clear_attrs(x);
    x->names = calloc((size_t)n + 1, sizeof(char *));
    x->values = calloc((size_t)n + 1, sizeof(char *));
    for (int i = 0; i < n; i++) { x->names[i] = strdup(a[i].name); x->values[i] = strdup(a[i].value); }
    x->n = n;
}
static void set_sec(MbItem *x, const uint8_t *s, size_t len, const char *ct) {
    sm_free(x->sec);
    x->sec = sm_alloc(len + 1);
    memcpy(x->sec, s, len);
    x->len = len;
    snprintf(x->ctype, sizeof x->ctype, "%s", ct && ct[0] ? ct : "text/plain");
}

static int mb_locked(void *ud) { return ((MemBack *)ud)->locked; }
static int mb_request_unlock(void *ud, unsigned id) {
    MemBack *b = ud;
    if (!b->locked) return 0;
    b->pending = id;
    b->pending_at = now_ms() + 1000;
    fprintf(stderr, "memback: unlock prompt %u (completes in 1 s)\n", id);
    return 1;
}
static int mb_lock(void *ud) { ((MemBack *)ud)->locked = 1; return 0; }
static int mb_search(void *ud, const SsAttr *a, int n, char (*ids)[40], int max) {
    MemBack *b = ud;           /* also searches while locked: the attributes are a separate index */
    int k = 0;
    for (int i = 0; i < MB_MAX && k < max; i++) {
        MbItem *x = &b->it[i];
        if (!x->used) continue;
        int ok = 1;
        for (int j = 0; j < n && ok; j++) ok = has(x, a[j].name, a[j].value);
        if (ok) snprintf(ids[k++], 40, "%s", x->id);
    }
    return k;
}
static int mb_info(void *ud, const char *id, SsItemInfo *out) {
    MemBack *b = ud;
    MbItem *x = b->locked ? NULL : find(b, id);
    if (!x) return -1;
    snprintf(out->id, sizeof out->id, "%s", x->id);
    out->label = strdup(x->label ? x->label : "");
    out->attrs = calloc((size_t)x->n + 1, sizeof(SsAttr));
    for (int i = 0; i < x->n; i++) { out->attrs[i].name = x->names[i]; out->attrs[i].value = x->values[i]; }
    out->nattrs = x->n;
    out->created = x->created;
    out->modified = x->modified;
    return 0;
}
static void mb_free_info(void *ud, SsItemInfo *in) { (void)ud; free(in->label); free(in->attrs); }
static int mb_get_secret(void *ud, const char *id, uint8_t **s, size_t *len, char ct[64]) {
    MemBack *b = ud;
    MbItem *x = b->locked ? NULL : find(b, id);
    if (!x) return -1;
    *s = sm_alloc(x->len + 1);
    memcpy(*s, x->sec, x->len);
    *len = x->len;
    snprintf(ct, 64, "%s", x->ctype);
    return 0;
}
static void mb_free_secret(void *ud, uint8_t *s, size_t len) { (void)ud; (void)len; sm_free(s); }
static int mb_create(void *ud, const char *label, const SsAttr *a, int n, const uint8_t *s, size_t len, const char *ct, int replace, char out[40]) {
    MemBack *b = ud;
    if (b->locked) return -1;
    MbItem *x = NULL;
    if (replace)
        for (int i = 0; i < MB_MAX && !x; i++) {
            MbItem *y = &b->it[i];
            if (!y->used || y->n != n) continue;
            int ok = 1;
            for (int j = 0; j < n && ok; j++) ok = has(y, a[j].name, a[j].value);
            if (ok) x = y;
        }
    if (!x) {
        for (int i = 0; i < MB_MAX && !x; i++) if (!b->it[i].used) x = &b->it[i];
        if (!x) return -1;
        memset(x, 0, sizeof *x);
        x->used = 1;
        snprintf(x->id, sizeof x->id, "i%04x", ++b->counter);
        x->created = (uint64_t)time(NULL);
    }
    free(x->label);
    x->label = strdup(label ? label : "");
    set_attrs_(x, a, n);
    set_sec(x, s, len, ct);
    x->modified = (uint64_t)time(NULL);
    snprintf(out, 40, "%s", x->id);
    return 0;
}
static int mb_set_secret(void *ud, const char *id, const uint8_t *s, size_t len, const char *ct) {
    MbItem *x = find(ud, id);
    if (!x) return -1;
    set_sec(x, s, len, ct);
    x->modified = (uint64_t)time(NULL);
    return 0;
}
static int mb_set_label(void *ud, const char *id, const char *l) { MbItem *x = find(ud, id); if (!x) return -1; free(x->label); x->label = strdup(l); return 0; }
static int mb_set_attrs(void *ud, const char *id, const SsAttr *a, int n) { MbItem *x = find(ud, id); if (!x) return -1; set_attrs_(x, a, n); return 0; }
static int mb_remove(void *ud, const char *id) {
    MbItem *x = find(ud, id);
    if (!x) return -1;
    clear_attrs(x); free(x->label); sm_free(x->sec);
    memset(x, 0, sizeof *x);
    return 0;
}
static void mb_caller(void *ud, const char *sender, uint32_t pid) {
    MemBack *b = ud;
    snprintf(b->last_caller, sizeof b->last_caller, "%s", sender);
    b->last_pid = pid;
}

const SsBackend memback_ops = {
    .locked = mb_locked, .request_unlock = mb_request_unlock, .lock = mb_lock, .search = mb_search,
    .info = mb_info, .free_info = mb_free_info, .get_secret = mb_get_secret, .free_secret = mb_free_secret,
    .create = mb_create, .set_secret = mb_set_secret, .set_label = mb_set_label, .set_attrs = mb_set_attrs,
    .remove = mb_remove, .caller = mb_caller,
};

void memback_tick(MemBack *b) {
    if (b->pending && now_ms() >= b->pending_at) {
        unsigned id = b->pending;
        b->pending = 0;
        b->locked = 0;
        fprintf(stderr, "memback: prompt %u accepted, unlocked\n", id);
        ss_prompt_done(id, 1);
    }
}
