/* dnsmap_test.c — IP → names per app (src/fw/dnsmap.c): DNS answer reader (A, AAAA,
 * CNAME chains, compression) with broken, truncated and pointer-loop packets; the LRU with
 * TTL; and its use together with the domain rules. Ends with random mutations. */
#ifndef _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#include "fw/dnsmap.h"
#include "fw/rules.h"

#include <arpa/inet.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails, checks;
#define CHECK(c) do { checks++; if (!(c)) { fails++; fprintf(stderr, "%s:%d: failed: %s\n", __FILE__, __LINE__, #c); } } while (0)

/* ---- packet building ---------------------------------------------------------------------------- */

typedef struct { uint8_t b[4096]; size_t n; } Pkt;

static void u8(Pkt *p, unsigned v) { p->b[p->n++] = (uint8_t)v; }
static void u16(Pkt *p, unsigned v) { u8(p, v >> 8); u8(p, v & 255); }
static void u32(Pkt *p, uint32_t v) { u16(p, v >> 16); u16(p, v & 0xffff); }

/* Name as text ("www.example.com"); "@N" at the end = pointer to N (e.g. "www@12"). */
static void name(Pkt *p, const char *s) {
    while (*s) {
        if (*s == '@') { unsigned off = (unsigned)atoi(s + 1); u16(p, 0xc000 | off); return; }
        const char *e = s;
        while (*e && *e != '.' && *e != '@') e++;
        u8(p, (unsigned)(e - s));
        for (; s < e; s++) u8(p, (unsigned char)*s);
        if (*s == '.') s++;
    }
    u8(p, 0);
}

static void header(Pkt *p, unsigned flags, unsigned qd, unsigned an) {
    p->n = 0;
    u16(p, 0x1234); u16(p, flags); u16(p, qd); u16(p, an); u16(p, 0); u16(p, 0);
}

static void question(Pkt *p, const char *q, unsigned type) { name(p, q); u16(p, type); u16(p, 1); }

static void rr_a(Pkt *p, const char *owner, const char *ip, uint32_t ttl) {
    uint8_t a[16];
    int v6 = strchr(ip, ':') != NULL;
    inet_pton(v6 ? AF_INET6 : AF_INET, ip, a);
    name(p, owner);
    u16(p, v6 ? 28 : 1); u16(p, 1); u32(p, ttl); u16(p, v6 ? 16 : 4);
    for (int i = 0; i < (v6 ? 16 : 4); i++) u8(p, a[i]);
}

static void rr_cname(Pkt *p, const char *owner, const char *to, uint32_t ttl) {
    name(p, owner);
    u16(p, 5); u16(p, 1); u32(p, ttl);
    size_t at = p->n;
    u16(p, 0);
    name(p, to);
    size_t len = p->n - at - 2;
    p->b[at] = (uint8_t)(len >> 8); p->b[at + 1] = (uint8_t)len;
}

static int ip(const char *s, uint8_t *a) {
    memset(a, 0, 16);
    if (strchr(s, ':')) { inet_pton(AF_INET6, s, a); return 6; }
    inet_pton(AF_INET, s, a);
    return 4;
}

static int look(DnsMap *m, const char *app, const char *addr, long long now, char (*out)[FW_NAME_MAX], int max) {
    uint8_t a[16];
    int fam = ip(addr, a);
    return dnsmap_lookup(m, app, fam, a, now, out, max);
}

static int has(char (*v)[FW_NAME_MAX], int n, const char *s) {
    for (int i = 0; i < n; i++) if (!strcmp(v[i], s)) return 1;
    return 0;
}

/* ---- lector ------------------------------------------------------------------------------------ */

static void test_parse(void) {
    DnsResponse *r = malloc(sizeof *r);
    Pkt p;

    /* simple A, with the owner compressed to the question (offset 12) */
    header(&p, 0x8180, 1, 2);
    question(&p, "WWW.Example.com", 1);
    rr_a(&p, "@12", "93.184.216.34", 300);
    rr_a(&p, "@12", "93.184.216.35", 100);
    CHECK(dns_parse_response(p.b, p.n, r) == 0);
    CHECK(!strcmp(r->qname, "www.example.com") && r->naddr == 2 && r->addr[0].fam == 4 && r->addr[1].ttl == 100);
    CHECK(!strcmp(r->addr[0].owner, "www.example.com"));

    /* truncated anywhere: always -1 */
    size_t full = p.n;
    int all = 1;
    for (size_t n = 0; n < full; n++) if (dns_parse_response(p.b, n, r) != -1) all = 0;
    CHECK(all);

    /* not an answer, another opcode, two questions, none */
    p.b[2] = 0x01; CHECK(dns_parse_response(p.b, full, r) == -1);
    p.b[2] = 0x81 | (2 << 3); CHECK(dns_parse_response(p.b, full, r) == -1);
    p.b[2] = 0x81; p.b[5] = 2; CHECK(dns_parse_response(p.b, full, r) == -1);
    p.b[5] = 0; CHECK(dns_parse_response(p.b, full, r) == -1);
    p.b[5] = 1; CHECK(dns_parse_response(p.b, full, r) == 0);
    CHECK(dns_parse_response(NULL, 10, r) == -1);

    /* NXDOMAIN: valid, no data */
    header(&p, 0x8183, 1, 0);
    question(&p, "nope.example", 1);
    CHECK(dns_parse_response(p.b, p.n, r) == 0 && r->rcode == 3 && r->naddr == 0);

    /* compression loops: to itself, forward, two pointing at each other, inside its own name */
    header(&p, 0x8180, 1, 0);
    u16(&p, 0xc00c); u16(&p, 1); u16(&p, 1);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);
    header(&p, 0x8180, 1, 0);
    u16(&p, 0xc010); u16(&p, 1); u16(&p, 1); u8(&p, 0);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);
    header(&p, 0x8180, 1, 1);
    question(&p, "a.b", 1);                             /* 12..16 */
    size_t loop = p.n;
    u8(&p, 1); u8(&p, 'x'); u16(&p, 0xc000 | (unsigned)(loop + 4));   /* x → (further ahead) */
    u8(&p, 1); u8(&p, 'y'); u16(&p, 0xc000 | (unsigned)loop);         /* y → x */
    u16(&p, 1); u16(&p, 1); u32(&p, 60); u16(&p, 4); u32(&p, 0x01020304);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);
    header(&p, 0x8180, 1, 1);
    question(&p, "a.b", 1);
    size_t self = p.n;
    u8(&p, 1); u8(&p, 'q'); u16(&p, 0xc000 | (unsigned)self);        /* q → q */
    u16(&p, 1); u16(&p, 1); u32(&p, 60); u16(&p, 4); u32(&p, 0x01020304);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);

    /* label types 0x40/0x80, name longer than 255 bytes, truncated pointer */
    header(&p, 0x8180, 1, 0);
    u8(&p, 0x41); for (int i = 0; i < 0x41; i++) u8(&p, 'a'); u8(&p, 0); u16(&p, 1); u16(&p, 1);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);
    header(&p, 0x8180, 1, 0);
    for (int l = 0; l < 5; l++) { u8(&p, 63); for (int i = 0; i < 63; i++) u8(&p, 'a'); }
    u8(&p, 0); u16(&p, 1); u16(&p, 1);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);
    header(&p, 0x8180, 1, 0);
    u8(&p, 0xc0);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);

    /* rdata with a wrong length: A of 5, CNAME that doesn't fill exactly its rdata, rdlen past the end */
    header(&p, 0x8180, 1, 1);
    question(&p, "a.example", 1);
    name(&p, "@12"); u16(&p, 1); u16(&p, 1); u32(&p, 60); u16(&p, 5); u32(&p, 0x01020304); u8(&p, 0);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);
    header(&p, 0x8180, 1, 1);
    question(&p, "a.example", 1);
    name(&p, "@12"); u16(&p, 5); u16(&p, 1); u32(&p, 60); u16(&p, 8); name(&p, "b.example");
    CHECK(dns_parse_response(p.b, p.n, r) == -1);
    header(&p, 0x8180, 1, 1);
    question(&p, "a.example", 1);
    name(&p, "@12"); u16(&p, 1); u16(&p, 1); u32(&p, 60); u16(&p, 400); u32(&p, 0x01020304);
    CHECK(dns_parse_response(p.b, p.n, r) == -1);

    /* names that aren't host names and other classes: the packet is valid, the record is not used */
    header(&p, 0x8180, 1, 2);
    question(&p, "ok.example", 1);
    rr_a(&p, "bad name.example", "1.1.1.1", 60);
    name(&p, "@12"); u16(&p, 1); u16(&p, 3); u32(&p, 60); u16(&p, 4); u32(&p, 0x02020202);   /* CHAOS */
    CHECK(dns_parse_response(p.b, p.n, r) == 0 && r->naddr == 0);
    header(&p, 0x8180, 1, 1);
    question(&p, "a\x01z.example", 1);
    rr_a(&p, "@12", "1.1.1.1", 60);
    CHECK(dns_parse_response(p.b, p.n, r) == 0 && r->qname[0] == 0 && r->naddr == 0);

    /* TTL with the high bit = 0; more than DNS_MAX_ADDRS records: the first ones are taken */
    header(&p, 0x8180, 1, 70);
    question(&p, "many.example", 1);
    for (int i = 0; i < 70; i++) {
        char a[32];
        snprintf(a, sizeof a, "10.0.%d.%d", i / 250, i % 250 + 1);
        rr_a(&p, "@12", a, i == 0 ? 0x80000001u : 60);
    }
    CHECK(dns_parse_response(p.b, p.n, r) == 0 && r->naddr == DNS_MAX_ADDRS && r->addr[0].ttl == 0);
    free(r);
}

/* ---- tabla ------------------------------------------------------------------------------------- */

static void test_map(void) {
    DnsMap *m = dnsmap_new(0);
    char out[16][FW_NAME_MAX];
    Pkt p;
    const char *A = "desktop:org.chromium.Chromium";

    /* CNAME chain with compression, plus a stray record that doesn't follow from the question */
    header(&p, 0x8180, 1, 5);
    question(&p, "www.example.com", 1);
    rr_cname(&p, "@12", "edge.cdn.net", 3600);
    rr_cname(&p, "edge.cdn.net", "a1.cdn.net", 120);
    rr_a(&p, "a1.cdn.net", "203.0.113.10", 300);
    rr_a(&p, "a1.cdn.net", "2001:db8::10", 300);
    rr_a(&p, "evil.example", "203.0.113.66", 300);
    int added = dnsmap_learn(m, A, p.b, p.n, 1000);
    CHECK(added == 6);                                      /* 2 direcciones × 3 nombres */
    int n = look(m, A, "203.0.113.10", 1000, out, 16);
    CHECK(n == 3 && has(out, n, "www.example.com") && has(out, n, "edge.cdn.net") && has(out, n, "a1.cdn.net"));
    CHECK(look(m, A, "2001:db8::10", 1000, out, 16) == 3);
    CHECK(look(m, A, "203.0.113.66", 1000, out, 16) == 0);  /* the stray one was ignored */
    CHECK(look(m, A, "::ffff:203.0.113.10", 1000, out, 16) == 3);   /* IPv4 mapeada */
    CHECK(look(m, "desktop:other", "203.0.113.10", 1000, out, 16) == 0);
    CHECK(look(m, "app-lordos-org.chromium.Chromium.scope", "203.0.113.10", 1000, out, 16) == 3);
    CHECK(look(m, A, "203.0.113.10", 1000, out, 1) == 1);
    /* TTL: the smallest in the chain (120) */
    CHECK(look(m, A, "203.0.113.10", 1119, out, 16) == 3);
    CHECK(look(m, A, "203.0.113.10", 1120, out, 16) == 0);
    CHECK(look(m, A, "2001:db8::10", 1120, out, 16) == 0);
    CHECK(dnsmap_count(m) == 0);

    /* looping CNAME: cut off without hanging; the final address is not in the chain */
    header(&p, 0x8180, 1, 3);
    question(&p, "loop.example", 1);
    rr_cname(&p, "@12", "b.example", 60);
    rr_cname(&p, "b.example", "loop.example", 60);
    rr_a(&p, "b.example", "198.51.100.1", 60);
    CHECK(dnsmap_learn(m, A, p.b, p.n, 2000) == 2);
    CHECK(look(m, A, "198.51.100.1", 2000, out, 16) == 2);

    /* TTL 0 is raised to DNSMAP_TTL_MIN; a huge one lowered to DNSMAP_TTL_MAX */
    uint8_t a[16];
    int fam = ip("192.0.2.1", a);
    CHECK(!dnsmap_add(m, A, "zero.example", fam, a, 0, 5000));
    CHECK(look(m, A, "192.0.2.1", 5000 + DNSMAP_TTL_MIN - 1, out, 16) == 1);
    CHECK(look(m, A, "192.0.2.1", 5000 + DNSMAP_TTL_MIN, out, 16) == 0);
    CHECK(!dnsmap_add(m, A, "big.example", fam, a, 0xffffffffu, 5000));
    CHECK(look(m, A, "192.0.2.1", 5000 + DNSMAP_TTL_MAX - 1, out, 16) == 1);
    CHECK(look(m, A, "192.0.2.1", 5000 + DNSMAP_TTL_MAX, out, 16) == 0);
    /* invalid input */
    CHECK(dnsmap_add(m, A, "bad name", fam, a, 60, 0) == -1);
    CHECK(dnsmap_add(m, "bad app", "x.example", fam, a, 60, 0) == -1);
    CHECK(dnsmap_add(m, A, "x.example", 5, a, 60, 0) == -1);
    CHECK(dnsmap_learn(m, A, (const uint8_t *)"junk", 4, 0) == -1);

    /* names per IP: at most DNSMAP_PER_IP, the newest stay, from most recent to oldest */
    for (int i = 0; i < 20; i++) {
        char nm[32];
        snprintf(nm, sizeof nm, "n%d.example", i);
        CHECK(!dnsmap_add(m, A, nm, fam, a, 600, 6000));
    }
    n = look(m, A, "192.0.2.1", 6000, out, 16);
    CHECK(n == DNSMAP_PER_IP && !strcmp(out[0], "n19.example") && !strcmp(out[DNSMAP_PER_IP - 1], "n12.example"));
    /* seeing a name again moves it to the front */
    CHECK(!dnsmap_add(m, A, "n14.example", fam, a, 600, 6001));
    CHECK(look(m, A, "192.0.2.1", 6001, out, 16) == DNSMAP_PER_IP && !strcmp(out[0], "n14.example"));

    dnsmap_forget_app(m, A);
    CHECK(dnsmap_count(m) == 0);
    dnsmap_free(m);

    /* LRU global acotada */
    m = dnsmap_new(16);
    for (int i = 0; i < 100; i++) {
        char addr[32];
        snprintf(addr, sizeof addr, "10.0.0.%d", i);
        ip(addr, a);
        CHECK(!dnsmap_add(m, i % 2 ? "lpk:x" : "lpk:y", "host.example", 4, a, 600, 0));
        CHECK(dnsmap_count(m) <= 16);
    }
    CHECK(look(m, "lpk:x", "10.0.0.99", 0, out, 16) == 1);
    CHECK(look(m, "lpk:y", "10.0.0.0", 0, out, 16) == 0);
    /* what gets looked up is not evicted first */
    CHECK(look(m, "lpk:y", "10.0.0.84", 0, out, 16) == 1);
    for (int i = 100; i < 115; i++) {
        char addr[32];
        snprintf(addr, sizeof addr, "10.0.1.%d", i);
        ip(addr, a);
        dnsmap_add(m, "lpk:z", "other.example", 4, a, 600, 0);
    }
    CHECK(look(m, "lpk:y", "10.0.0.84", 0, out, 16) == 1);
    CHECK(look(m, "lpk:x", "10.0.0.85", 0, out, 16) == 0);
    dnsmap_expire(m, 600);
    CHECK(dnsmap_count(m) == 0);
    dnsmap_free(m);

    m = dnsmap_new(1);
    ip("10.9.9.9", a);
    CHECK(!dnsmap_add(m, "lpk:x", "a.example", 4, a, 600, 0));
    CHECK(!dnsmap_add(m, "lpk:x", "b.example", 4, a, 600, 0));
    CHECK(dnsmap_count(m) == 1 && look(m, "lpk:x", "10.9.9.9", 0, out, 16) == 1 && !strcmp(out[0], "b.example"));
    dnsmap_free(m);
}

/* ---- with the rules ---------------------------------------------------------------------------- */

static void test_with_rules(void) {
    DnsMap *m = dnsmap_new(0);
    FwStore *s = fw_store_new();
    fw_set_mode(s, FW_MODE_ASK);
    const char *A = "desktop:org.chromium.Chromium";
    FwRule r;
    memset(&r, 0, sizeof r);
    snprintf(r.app, sizeof r.app, "%s", A);
    r.action = FW_DENY;
    CHECK(!fw_parse_dest("*.doubleclick.net", &r) && !fw_rule_add(s, &r, NULL));
    r.action = FW_ALLOW;
    CHECK(!fw_parse_dest("example.com", &r) && !fw_rule_add(s, &r, NULL));

    Pkt p;
    header(&p, 0x8180, 1, 2);
    question(&p, "ad.doubleclick.net", 1);
    rr_cname(&p, "@12", "pagead.l.google.com", 300);
    rr_a(&p, "pagead.l.google.com", "142.250.1.1", 300);
    CHECK(dnsmap_learn(m, A, p.b, p.n, 0) == 2);
    header(&p, 0x8180, 1, 1);
    question(&p, "example.com", 28);
    rr_a(&p, "@12", "2606:2800:220:1::1", 300);
    CHECK(dnsmap_learn(m, A, p.b, p.n, 0) == 1);

    char names[DNSMAP_PER_IP][FW_NAME_MAX];
    const char *hosts[DNSMAP_PER_IP];
    FwQuery q;
    memset(&q, 0, sizeof q);
    q.app = A; q.dir = FW_OUT; q.proto = FW_PROTO_TCP; q.port = 443;
    q.fam = ip("142.250.1.1", q.addr);
    q.nhosts = dnsmap_lookup(m, A, q.fam, q.addr, 10, names, DNSMAP_PER_IP);
    for (int i = 0; i < q.nhosts; i++) hosts[i] = names[i];
    q.hosts = hosts;
    CHECK(q.nhosts == 2 && fw_decide(s, &q).verdict == FW_V_DENY);
    q.fam = ip("2606:2800:220:1::1", q.addr);
    q.nhosts = dnsmap_lookup(m, A, q.fam, q.addr, 10, names, DNSMAP_PER_IP);
    CHECK(q.nhosts == 1 && fw_decide(s, &q).verdict == FW_V_ALLOW);
    /* another app does not inherit the names Chromium resolved */
    q.app = "desktop:other";
    q.nhosts = dnsmap_lookup(m, q.app, q.fam, q.addr, 10, names, DNSMAP_PER_IP);
    CHECK(q.nhosts == 0 && fw_decide(s, &q).verdict == FW_V_ASK);
    fw_store_free(s);
    dnsmap_free(m);
}

/* ---- random ------------------------------------------------------------------------------------ */

static void fuzz(void) {
    DnsMap *m = dnsmap_new(256);
    DnsResponse *r = malloc(sizeof *r);
    Pkt base;
    header(&base, 0x8180, 1, 4);
    question(&base, "www.example.com", 1);
    rr_cname(&base, "@12", "e.cdn.net", 60);
    rr_cname(&base, "e.cdn.net", "f.cdn.net", 60);
    rr_a(&base, "f.cdn.net", "203.0.113.1", 60);
    rr_a(&base, "@12", "2001:db8::1", 60);
    CHECK(dns_parse_response(base.b, base.n, r) == 0 && r->ncname == 2 && r->naddr == 2);
    srand(7);
    uint8_t buf[600];
    long ok = 0, bad = 0;
    for (int it = 0; it < 200000; it++) {
        size_t n;
        if (it % 4 == 0) {
            n = (size_t)(rand() % (int)sizeof buf);
            for (size_t i = 0; i < n; i++) buf[i] = (uint8_t)rand();
            if (n > 3 && rand() % 2) { buf[2] = 0x81; buf[3] = 0x80; buf[4] = 0; buf[5] = 1; }
        } else {
            n = base.n;
            memcpy(buf, base.b, n);
            int k = 1 + rand() % 4;
            for (int x = 0; x < k && n; x++) {
                size_t pos = (size_t)rand() % n;
                switch (rand() % 4) {
                case 0: buf[pos] = (uint8_t)rand(); break;
                case 1: buf[pos] = (uint8_t)(0xc0 | rand() % 2); if (pos + 1 < n) buf[pos + 1] = (uint8_t)rand(); break;
                case 2: n = pos; break;
                case 3: buf[pos] ^= (uint8_t)(1u << (rand() % 8)); break;
                }
            }
        }
        /* exact copy on the heap: ASan sees any over-read */
        uint8_t *h = malloc(n ? n : 1);
        memcpy(h, buf, n);
        int res = dns_parse_response(h, n, r);
        if (res == 0) {
            ok++;
            for (int i = 0; i < r->naddr; i++) CHECK(strlen(r->addr[i].owner) < FW_NAME_MAX && (r->addr[i].fam == 4 || r->addr[i].fam == 6));
            for (int i = 0; i < r->ncname; i++) CHECK(strlen(r->cname[i].from) <= 253 && strlen(r->cname[i].to) <= 253);
        } else bad++;
        dnsmap_learn(m, "lpk:fuzz", h, n, it / 100);
        free(h);
        CHECK(dnsmap_count(m) <= 256);
    }
    printf("  random: 200000 packets, %ld valid, %ld rejected, %zu names in the table\n", ok, bad, dnsmap_count(m));
    free(r);
    dnsmap_free(m);
}

int main(void) {
    test_parse();
    test_map();
    test_with_rules();
    fuzz();
    printf("dnsmap-test: %d/%d passed\n", checks - fails, checks);
    return fails ? 1 : 0;
}
