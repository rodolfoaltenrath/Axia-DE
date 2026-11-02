/* dnsmap.c — IP → names per app, learned from DNS (see dnsmap.h). */
#include "fw/dnsmap.h"
#include "fw/fwmap.h"

#include <stdlib.h>
#include <string.h>

/* ---- DNS response reader ---------------------------------------------------------------------- */

#define MAX_HOPS 64

static uint16_t rd16(const uint8_t *p) { return (uint16_t)(p[0] << 8 | p[1]); }
static uint32_t rd32(const uint8_t *p) { return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; }

/* Name at off. *next: first byte after the name in place. *host: 1 if it is a host name that can
 * be normalized (otherwise out stays empty but the packet is still valid). */
static int read_name(const uint8_t *pkt, size_t len, size_t off, size_t *next, char *out, int *host) {
    size_t pos = off, seg = off, wire = 0, o = 0;
    int jumped = 0, hops = 0, ok = 1;
    char txt[FW_NAME_MAX + 4];
    for (;;) {
        if (pos >= len) return -1;
        uint8_t c = pkt[pos];
        if (c == 0) {
            if (!jumped) *next = pos + 1;
            wire++;
            break;
        }
        if ((c & 0xc0) == 0xc0) {
            if (pos + 1 >= len) return -1;
            size_t ptr = (size_t)(c & 0x3f) << 8 | pkt[pos + 1];
            /* only backwards from the current span: each jump lowers seg, so it terminates */
            if (ptr >= seg || ++hops > MAX_HOPS) return -1;
            if (!jumped) *next = pos + 2;
            jumped = 1;
            pos = seg = ptr;
            continue;
        }
        if (c & 0xc0) return -1;                    /* 0x40 and 0x80: obsolete label types */
        if (pos + 1 + c > len) return -1;
        wire += 1u + c;
        if (wire > 255) return -1;
        if (o) txt[o++] = '.';
        for (size_t i = 0; i < c; i++) {
            uint8_t ch = pkt[pos + 1 + i];
            if (ch >= 'A' && ch <= 'Z') ch = (uint8_t)(ch - 'A' + 'a');
            if (!((ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9') || ch == '-' || ch == '_')) { ok = 0; ch = '?'; }
            txt[o++] = (char)ch;
        }
        pos += 1u + c;
    }
    txt[o] = 0;
    out[0] = 0;
    *host = ok && o && !fw_norm_domain(txt, out);
    if (!*host) out[0] = 0;
    return 0;
}

int dns_parse_response(const uint8_t *pkt, size_t len, DnsResponse *out) {
    if (!pkt || !out) return -1;
    memset(out, 0, sizeof *out);
    if (len < 12 || len > 65535) return -1;
    if (!(pkt[2] & 0x80) || (pkt[2] >> 3 & 0x0f) != 0) return -1;   /* QR=1, OPCODE=QUERY */
    out->rcode = pkt[3] & 0x0f;
    uint16_t qd = rd16(pkt + 4), an = rd16(pkt + 6);
    if (qd != 1) return -1;
    size_t pos = 12, next;
    int host;
    if (read_name(pkt, len, pos, &next, out->qname, &host)) return -1;
    pos = next;
    if (pos + 4 > len) return -1;
    out->qtype = rd16(pkt + pos);
    uint16_t qclass = rd16(pkt + pos + 2);
    pos += 4;
    int usable = host && qclass == 1 && out->rcode == 0;
    for (unsigned i = 0; i < an; i++) {
        char owner[FW_NAME_MAX];
        int ohost;
        if (read_name(pkt, len, pos, &next, owner, &ohost)) return -1;
        pos = next;
        if (pos + 10 > len) return -1;
        uint16_t type = rd16(pkt + pos), cls = rd16(pkt + pos + 2), rdlen = rd16(pkt + pos + 8);
        uint32_t ttl = rd32(pkt + pos + 4);
        if (ttl & 0x80000000u) ttl = 0;             /* RFC 2181: TTL with the high bit = 0 */
        pos += 10;
        if (pos + rdlen > len) return -1;
        size_t rd = pos;
        pos += rdlen;
        if (cls != 1) continue;
        if (type == 1 || type == 28) {
            if (rdlen != (type == 1 ? 4 : 16)) return -1;
            if (!usable || !ohost || out->naddr >= DNS_MAX_ADDRS) continue;
            DnsAddrRec *a = &out->addr[out->naddr++];
            a->fam = type == 1 ? 4 : 6;
            memcpy(a->addr, pkt + rd, rdlen);
            a->ttl = ttl;
            memcpy(a->owner, owner, sizeof a->owner);
        } else if (type == 5) {
            char to[FW_NAME_MAX];
            int thost;
            size_t tnext = 0;
            /* the name has to fill rdata exactly (its pointers may go further back) */
            if (read_name(pkt, pos, rd, &tnext, to, &thost) || tnext != pos) return -1;
            if (!usable || !ohost || !thost || out->ncname >= DNS_MAX_CNAMES) continue;
            DnsCname *c = &out->cname[out->ncname++];
            memcpy(c->from, owner, sizeof c->from);
            memcpy(c->to, to, sizeof c->to);
            c->ttl = ttl;
        }
    }
    return 0;
}

/* ---- table ------------------------------------------------------------------------------------- */

typedef struct Group Group;
typedef struct Entry Entry;

struct Entry {
    Entry *prev, *next;             /* global LRU: head = most recent */
    Entry *gnext;                   /* in its group */
    Group *g;
    long long expires;
    char name[FW_NAME_MAX];
};

#define KEY_MAX (FW_APP_MAX + 1 + 16)

struct Group {
    unsigned char key[KEY_MAX];
    size_t klen;
    Entry *head;
    int n;
};

struct DnsMap {
    FwMap groups;
    Entry *head, *tail;
    size_t n, max;
};

DnsMap *dnsmap_new(size_t max_entries) {
    DnsMap *m = calloc(1, sizeof *m);
    if (!m) return NULL;
    m->max = max_entries ? max_entries : 4096;
    if (m->max > (1u << 20)) m->max = 1u << 20;
    return m;
}

static void group_free(void *v) {
    Group *g = v;
    if (!g) return;
    for (Entry *e = g->head; e;) { Entry *n = e->gnext; free(e); e = n; }
    free(g);
}

void dnsmap_free(DnsMap *m) {
    if (!m) return;
    fwmap_free(&m->groups, group_free);
    free(m);
}

size_t dnsmap_count(const DnsMap *m) { return m ? m->n : 0; }

static void lru_unlink(DnsMap *m, Entry *e) {
    if (e->prev) e->prev->next = e->next; else m->head = e->next;
    if (e->next) e->next->prev = e->prev; else m->tail = e->prev;
    e->prev = e->next = NULL;
}

static void lru_front(DnsMap *m, Entry *e) {
    e->prev = NULL;
    e->next = m->head;
    if (m->head) m->head->prev = e; else m->tail = e;
    m->head = e;
}

/* Removes e from everything. If the group ends up empty and drop_group, frees it too. */
static void entry_del(DnsMap *m, Entry *e, int drop_group) {
    Group *g = e->g;
    for (Entry **pp = &g->head; *pp; pp = &(*pp)->gnext) if (*pp == e) { *pp = e->gnext; break; }
    g->n--;
    lru_unlink(m, e);
    free(e);
    m->n--;
    if (drop_group && !g->head) {
        fwmap_take(&m->groups, g->key, g->klen);
        free(g);
    }
}

/* Key app\0fam addr (IPv4 mapped into IPv6 counts as IPv4). */
static int make_key(const char *app, int fam, const uint8_t *addr, unsigned char *key, size_t *klen) {
    char c[FW_APP_MAX];
    static const uint8_t mapped[12] = { 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff };
    if (!addr || fw_app_canon(app, c, sizeof c)) return -1;
    if (fam == 6 && !memcmp(addr, mapped, 12)) { fam = 4; addr += 12; }
    if (fam != 4 && fam != 6) return -1;
    size_t al = strlen(c), n = fam == 4 ? 4 : 16;
    memcpy(key, c, al + 1);
    key[al + 1] = (unsigned char)fam;
    memcpy(key + al + 2, addr, n);
    *klen = al + 2 + n;
    return 0;
}

/* e becomes the most recent of its group and of the whole table. */
static void touch(DnsMap *m, Entry *e) {
    Group *g = e->g;
    if (g->head != e) {
        for (Entry **pp = &g->head; *pp; pp = &(*pp)->gnext) if (*pp == e) { *pp = e->gnext; break; }
        e->gnext = g->head;
        g->head = e;
    }
    lru_unlink(m, e);
    lru_front(m, e);
}

int dnsmap_add(DnsMap *m, const char *app, const char *name, int fam, const uint8_t *addr, uint32_t ttl, long long now) {
    unsigned char key[KEY_MAX + 2];
    size_t klen;
    char nm[FW_NAME_MAX];
    if (!m || make_key(app, fam, addr, key, &klen) || fw_norm_domain(name, nm)) return -1;
    if (ttl < DNSMAP_TTL_MIN) ttl = DNSMAP_TTL_MIN;
    if (ttl > DNSMAP_TTL_MAX) ttl = DNSMAP_TTL_MAX;
    long long exp = now + ttl;

    Group *g = fwmap_get(&m->groups, key, klen);
    for (Entry *e = g ? g->head : NULL; e; e = e->gnext)
        if (!strcmp(e->name, nm)) {
            e->expires = exp;               /* the newest answer wins */
            touch(m, e);
            return 0;
        }
    /* table full: forget what was used longest ago (it may belong to this same group, which is
     * why the group is looked up again afterwards) */
    while (m->n >= m->max && m->tail) entry_del(m, m->tail, 1);
    g = fwmap_get(&m->groups, key, klen);
    if (g && g->n >= DNSMAP_PER_IP) {
        Entry *old = g->head;
        while (old->gnext) old = old->gnext;    /* the group's tail is the least used */
        entry_del(m, old, 0);                   /* DNSMAP_PER_IP - 1 remain: the group stays */
    }
    if (!g) {
        g = calloc(1, sizeof *g);
        if (!g) return -1;
        memcpy(g->key, key, klen);
        g->klen = klen;
        void **slot = fwmap_slot(&m->groups, key, klen, 1);
        if (!slot) { free(g); return -1; }
        *slot = g;
    }
    Entry *e = calloc(1, sizeof *e);
    if (!e) {
        if (!g->head) { fwmap_take(&m->groups, g->key, g->klen); free(g); }
        return -1;
    }
    memcpy(e->name, nm, sizeof e->name);
    e->expires = exp;
    e->g = g;
    e->gnext = g->head;
    g->head = e;
    g->n++;
    lru_front(m, e);
    m->n++;
    return 0;
}

int dnsmap_learn(DnsMap *m, const char *app, const uint8_t *pkt, size_t len, long long now) {
    if (!m) return -1;
    DnsResponse *r = malloc(sizeof *r);
    if (!r) return -1;
    if (dns_parse_response(pkt, len, r)) { free(r); return -1; }
    int added = 0;
    if (r->qname[0] && r->rcode == 0) {
        /* the chain from the question: qname → CNAME → ... (no repeats, at most DNS_MAX_CNAMES) */
        const char *chain[DNS_MAX_CNAMES + 1];
        int nc = 0;
        uint32_t cttl = UINT32_MAX;
        chain[nc++] = r->qname;
        for (int hop = 0; hop < DNS_MAX_CNAMES; hop++) {
            const DnsCname *found = NULL;
            for (int i = 0; i < r->ncname; i++) if (!strcmp(r->cname[i].from, chain[nc - 1])) { found = &r->cname[i]; break; }
            if (!found) break;
            int seen = 0;
            for (int k = 0; k < nc; k++) if (!strcmp(chain[k], found->to)) seen = 1;
            if (seen) break;                /* CNAME loop */
            if (found->ttl < cttl) cttl = found->ttl;
            chain[nc++] = found->to;
        }
        for (int i = 0; i < r->naddr; i++) {
            const DnsAddrRec *a = &r->addr[i];
            int in_chain = 0;
            for (int k = 0; k < nc; k++) if (!strcmp(chain[k], a->owner)) in_chain = 1;
            if (!in_chain) continue;        /* stray records: they don't come from the question */
            uint32_t ttl = a->ttl < cttl ? a->ttl : cttl;
            for (int k = 0; k < nc; k++) if (!dnsmap_add(m, app, chain[k], a->fam, a->addr, ttl, now)) added++;
        }
    }
    free(r);
    return added;
}

int dnsmap_lookup(DnsMap *m, const char *app, int fam, const uint8_t *addr, long long now, char (*out)[FW_NAME_MAX], int max) {
    unsigned char key[KEY_MAX + 2];
    size_t klen;
    if (!m || make_key(app, fam, addr, key, &klen)) return 0;
    Group *g = fwmap_get(&m->groups, key, klen);
    if (!g) return 0;
    for (Entry *e = g->head, *n; e; e = n) {
        n = e->gnext;
        if (e->expires <= now) entry_del(m, e, 0);
    }
    if (!g->head) { fwmap_take(&m->groups, g->key, g->klen); free(g); return 0; }
    /* the group is already ordered from most recent to oldest */
    Entry *v[DNSMAP_PER_IP];
    int k = 0, w = 0;
    for (Entry *e = g->head; e && k < DNSMAP_PER_IP; e = e->gnext) v[k++] = e;
    for (int i = 0; i < k && w < max; i++) memcpy(out[w++], v[i]->name, FW_NAME_MAX);
    /* they were used: to the front of the LRU, keeping their order */
    for (int i = k - 1; i >= 0; i--) { lru_unlink(m, v[i]); lru_front(m, v[i]); }
    return w;
}

void dnsmap_expire(DnsMap *m, long long now) {
    if (!m) return;
    for (Entry *e = m->head, *n; e; e = n) {
        n = e->next;
        if (e->expires <= now) entry_del(m, e, 1);
    }
}

void dnsmap_forget_app(DnsMap *m, const char *app) {
    char c[FW_APP_MAX];
    if (!m || fw_app_canon(app, c, sizeof c)) return;
    size_t cl = strlen(c) + 1;
    for (Entry *e = m->head, *n; e; e = n) {
        n = e->next;
        if (e->g->klen > cl && !memcmp(e->g->key, c, cl)) entry_del(m, e, 1);
    }
}

int dnsmap_each(DnsMap *m, const char *app, long long now,
                int (*cb)(void *u, int fam, const uint8_t *addr, const char (*names)[FW_NAME_MAX], int n), void *u) {
    char c[FW_APP_MAX];
    if (!m || !cb || fw_app_canon(app, c, sizeof c)) return 0;
    size_t cl = strlen(c) + 1;
    int groups = 0;
    FWMAP_EACH(&m->groups, me) {
        Group *g = me->val;
        if (g->klen <= cl + 1 || memcmp(g->key, c, cl)) continue;
        int fam = g->key[cl];
        if ((fam != 4 && fam != 6) || g->klen != cl + 1 + (fam == 4 ? 4u : 16u)) continue;
        char names[DNSMAP_PER_IP][FW_NAME_MAX];
        int k = 0;
        for (Entry *e = g->head; e && k < DNSMAP_PER_IP; e = e->gnext)
            if (e->expires > now) memcpy(names[k++], e->name, FW_NAME_MAX);
        if (!k) continue;
        groups++;
        if (cb(u, fam, g->key + cl + 1, (const char (*)[FW_NAME_MAX])names, k)) break;
    }
    return groups;
}
