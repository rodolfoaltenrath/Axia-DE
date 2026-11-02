/* journal_test.c — test for our own journal reader (src/logs/journal.c, Logs app).
 *
 * Builds synthetic journal files with a small writer (normal and compact mode, repeated fields,
 * LZ4, XZ and flagged ZSTD), reads them back and compares; merges two files with filters; and
 * tries truncated files, randomly changed bytes and an array chain with a loop: it must not hang
 * or crash.
 *
 *   ./build/journal-test                  the tests
 *   ./build/journal-test dump <file>      one JSON line per entry (to compare with journalctl)
 *   ./build/journal-test bench <file>     entries per second */
#include "logs/journal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #c); fails++; } } while (0)

static const unsigned char xz_msg[] = { 0xfd,0x37,0x7a,0x58,0x5a,0x00,0x00,0x04,0xe6,0xd6,0xb4,0x46,0x04,0xc0,0x39,0x8b,0x03,0x21,0x01,0x1c,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xe8,0xf5,0x6f,0x6b,0xe0,0x01,0x8a,0x00,0x31,0x5d,0x00,0x26,0x91,0x46,0xc0,0xd1,0x94,0x57,0xe4,0x94,0x68,0xd5,0x3b,0x95,0x7d,0x5f,0xe8,0x9e,0x10,0x92,0x9b,0xc4,0x67,0x8a,0x11,0x25,0x86,0xca,0xeb,0xfd,0x90,0x33,0x36,0x6d,0x5b,0xa2,0xd5,0x29,0xca,0xe9,0x11,0x6a,0xb6,0x19,0xe5,0x6d,0x47,0x19,0x98,0x00,0x00,0x00,0x00,0x00,0x45,0x0a,0x87,0x3c,0xf0,0x17,0xce,0x19,0x00,0x01,0x55,0x8b,0x03,0x00,0x00,0x00,0xf4,0x63,0x95,0x4b,0xb1,0xc4,0x67,0xfb,0x02,0x00,0x00,0x00,0x00,0x04,0x59,0x5a };

/* ---- minimal writer ---- */

typedef struct { unsigned char *b; size_t n, cap; int compact; } W;

static void need(W *w, size_t n) {
    if (w->n + n <= w->cap) return;
    while (w->cap < w->n + n) w->cap = w->cap ? w->cap * 2 : 4096;
    w->b = realloc(w->b, w->cap);
}
static void put64(unsigned char *p, uint64_t v) { for (int i = 0; i < 8; i++) p[i] = (unsigned char)(v >> (8 * i)); }
static void put32(unsigned char *p, uint32_t v) { for (int i = 0; i < 4; i++) p[i] = (unsigned char)(v >> (8 * i)); }

static uint64_t obj(W *w, int type, int flags, const void *body, size_t blen) {
    while (w->n & 7) { need(w, 1); w->b[w->n++] = 0; }
    uint64_t off = w->n;
    need(w, 16 + blen + 8);
    memset(w->b + w->n, 0, 16);
    w->b[w->n] = (unsigned char)type;
    w->b[w->n + 1] = (unsigned char)flags;
    put64(w->b + w->n + 8, 16 + blen);
    memcpy(w->b + w->n + 16, body, blen);
    w->n += 16 + blen;
    return off;
}

typedef struct { char text[512]; uint64_t off; } Seen;
static Seen seen[256];
static int nseen;

static uint64_t data(W *w, const char *payload, size_t plen, int flags) {
    for (int i = 0; i < nseen && !flags; i++) if (!strcmp(seen[i].text, payload)) return seen[i].off;   /* deduplicated */
    size_t fixed = w->compact ? 56 : 48;
    unsigned char body[1024] = { 0 };
    memcpy(body + fixed, payload, plen);
    uint64_t off = obj(w, 1, flags, body, fixed + plen);
    if (!flags && nseen < 256) { snprintf(seen[nseen].text, sizeof seen[0].text, "%s", payload); seen[nseen++].off = off; }
    return off;
}

static uint64_t entry(W *w, uint64_t seq, uint64_t rt, const uint64_t *d, int nd) {
    unsigned char body[2048] = { 0 };
    put64(body, seq);
    put64(body + 8, rt);
    put64(body + 16, rt / 3);
    size_t isz = w->compact ? 4 : 16;
    for (int i = 0; i < nd; i++) { if (w->compact) put32(body + 48 + i * 4, (uint32_t)d[i]); else put64(body + 48 + i * 16, d[i]); }
    return obj(w, 3, 0, body, 48 + isz * nd);
}

static uint64_t earray(W *w, const uint64_t *e, int ne, int cap) {
    unsigned char body[1024] = { 0 };
    size_t isz = w->compact ? 4 : 8;
    for (int i = 0; i < ne; i++) { if (w->compact) put32(body + 8 + i * 4, (uint32_t)e[i]); else put64(body + 8 + i * 8, e[i]); }
    return obj(w, 6, 0, body, 8 + isz * cap);
}

/* Hand-made LZ4: "MESSAGE=" + "lz4 " literal, then a repeated distance-4 match */
static size_t lz4_blob(unsigned char *out) {
    const char *lit = "MESSAGE=lz4 ";
    size_t ll = strlen(lit), ml = 40;           /* "lz4 " × 10 more */
    size_t n = 0;
    put64(out, ll + ml); n = 8;
    out[n++] = (unsigned char)((ll << 4) | 15); /* ll < 15 */
    memcpy(out + n, lit, ll); n += ll;
    out[n++] = 4; out[n++] = 0;                 /* distance 4 */
    out[n++] = (unsigned char)(ml - 4 - 15);    /* 15 + remainder */
    out[n++] = 0x10;                            /* last sequence: one literal */
    out[n++] = '!';
    put64(out, ll + ml + 1);
    return n;
}

static const char *write_file(const char *path, int compact, uint64_t base) {
    W w = { 0 };
    w.compact = compact;
    nseen = 0;
    need(&w, 272);
    memset(w.b, 0, 272);
    w.n = 272;
    uint64_t ents[16];
    int ne = 0;
    for (int i = 0; i < 6; i++) {
        uint64_t d[12];
        int nd = 0;
        char msg[200], prio[16], pid[32];
        snprintf(msg, sizeof msg, "MESSAGE=entrada %d \xc3\xb1 \"comillas\"\tfin", i);
        snprintf(prio, sizeof prio, "PRIORITY=%d", i % 8);
        snprintf(pid, sizeof pid, "_PID=%d", 100 + i);
        d[nd++] = data(&w, msg, strlen(msg), 0);
        d[nd++] = data(&w, prio, strlen(prio), 0);
        d[nd++] = data(&w, pid, strlen(pid), 0);
        d[nd++] = data(&w, i % 2 ? "SYSLOG_IDENTIFIER=impar" : "SYSLOG_IDENTIFIER=par", i % 2 ? 23 : 21, 0);
        d[nd++] = data(&w, "_COMM=prueba", 12, 0);
        d[nd++] = data(&w, "_BOOT_ID=0123456789abcdef0123456789abcdef", 41, 0);
        d[nd++] = data(&w, "_SYSTEMD_UNIT=user@1000.service", 31, 0);
        if (i == 3) d[nd++] = data(&w, "_SYSTEMD_USER_UNIT=pipewire.service", 35, 0);
        d[nd++] = data(&w, "_HOSTNAME=equipo", 16, 0);
        d[nd++] = data(&w, "_UID=1000", 9, 0);
        if (i == 4) d[nd++] = data(&w, "_TRANSPORT=kernel", 17, 0);
        ents[ne++] = entry(&w, base / 1000 + (uint64_t)i, base + (uint64_t)i * 1000000, d, nd);
    }
    {
        unsigned char blob[128];
        size_t bl = lz4_blob(blob);
        uint64_t d[2] = { data(&w, (const char *)blob, bl, 2), data(&w, "PRIORITY=6", 10, 0) };
        ents[ne++] = entry(&w, base / 1000 + 6, base + 6000000, d, 2);
    }
    {
        uint64_t d[1] = { data(&w, (const char *)xz_msg, sizeof xz_msg, 1) };
        ents[ne++] = entry(&w, base / 1000 + 7, base + 7000000, d, 1);
    }
    {
        uint64_t d[1] = { data(&w, "\x28\xb5\x2f\xfd basura", 11, 4) };
        ents[ne++] = entry(&w, base / 1000 + 8, base + 8000000, d, 1);
    }
    uint64_t first = earray(&w, ents, 4, 4);
    uint64_t second = earray(&w, ents + 4, ne - 4, 16);
    put64(w.b + first + 16, second);            /* next_entry_array_offset */
    /* header */
    memcpy(w.b, "LPKSHHRH", 8);
    put32(w.b + 12, (uint32_t)((compact ? 16 : 0) | 1 | 2 | 8));
    w.b[16] = 1;                                /* STATE_ONLINE */
    put64(w.b + 88, 272);
    put64(w.b + 96, w.n - 272);
    put64(w.b + 144, 0);
    put64(w.b + 152, (uint64_t)ne);
    put64(w.b + 176, first);
    FILE *f = fopen(path, "wb");
    if (!f || fwrite(w.b, 1, w.n, f) != w.n) { perror(path); exit(1); }
    fclose(f);
    free(w.b);
    return path;
}

static unsigned char *slurp(const char *p, size_t *n) {
    FILE *f = fopen(p, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    *n = (size_t)ftell(f);
    fseek(f, 0, SEEK_SET);
    unsigned char *b = malloc(*n);
    if (fread(b, 1, *n, f) != *n) { free(b); b = NULL; }
    fclose(f);
    return b;
}

static void spit(const char *p, const unsigned char *b, size_t n) {
    FILE *f = fopen(p, "wb");
    if (!f) exit(1);
    if (n && fwrite(b, 1, n, f) != n) exit(1);
    fclose(f);
}

/* Reads everything and makes sure nothing breaks (doesn't compare content). */
static long read_all(const char *p) {
    JFile *f = jf_open(p);
    if (!f) return -1;
    long ok = 0;
    for (size_t i = 0; i < jf_count(f); i++) {
        JEntry e;
        if (!jf_entry(f, i, &e)) {
            char buf[4096];
            jentry_json(&e, buf, sizeof buf, 500);
            ok++;
        }
        jentry_free(&e);
    }
    jf_close(f);
    return ok;
}

static void check_file(const char *p, int compact, uint64_t base) {
    JFile *f = jf_open(p);
    CHECK(f != NULL);
    if (!f) return;
    CHECK(jf_compact(f) == compact);
    CHECK(jf_count(f) == 9);
    JEntry e;
    CHECK(jf_entry(f, 0, &e) == 0);
    CHECK(!strcmp(e.message, "entrada 0 \xc3\xb1 \"comillas\"\tfin"));
    CHECK(e.priority == 0 && e.pid == 100 && e.uid == 1000);
    CHECK(!strcmp(e.ident, "par") && !strcmp(e.comm, "prueba"));
    CHECK(!strcmp(e.unit, "user@1000.service"));
    CHECK(!strncmp(e.boot, "0123456789abcdef", 16));
    CHECK(e.realtime == base);
    CHECK(!e.kernel);
    jentry_free(&e);
    CHECK(jf_entry(f, 3, &e) == 0);
    CHECK(!strcmp(e.unit, "pipewire.service"));      /* the user unit wins */
    CHECK(!strcmp(e.ident, "impar") && e.priority == 3);
    jentry_free(&e);
    CHECK(jf_entry(f, 4, &e) == 0 && e.kernel);
    jentry_free(&e);
    CHECK(jf_entry(f, 6, &e) == 0);
    CHECK(!strcmp(e.message, "lz4 lz4 lz4 lz4 lz4 lz4 lz4 lz4 lz4 lz4 lz4 !"));
    CHECK(e.priority == 6);
    jentry_free(&e);
    CHECK(jf_entry(f, 7, &e) == 0);
    CHECK(!strncmp(e.message, "mensaje comprimido con xz: repetido repetido", 44));
    jentry_free(&e);
    CHECK(jf_entry(f, 8, &e) == 0);
    CHECK(!strcmp(e.message, "[zstd-compressed]"));
    jentry_free(&e);
    CHECK(jf_realtime(f, 5) == base + 5000000);
    CHECK(jf_entry(f, 99, &e) == -1);
    jf_close(f);
}

static int dump(const char *p) {
    JFile *f = jf_open(p);
    if (!f) { fprintf(stderr, "not a journal: %s\n", p); return 1; }
    char *buf = malloc(JMSG_MAX * 6 + 512);
    for (size_t i = 0; i < jf_count(f); i++) {
        JEntry e;
        if (jf_entry(f, i, &e)) { printf("{\"error\":%zu}\n", i); continue; }
        if (jentry_json(&e, buf, JMSG_MAX * 6 + 512, JMSG_MAX + 8)) puts(buf);
        jentry_free(&e);
    }
    free(buf);
    jf_close(f);
    return 0;
}

static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }

static int bench(const char *p) {
    double t0 = now();
    JFile *f = jf_open(p);
    if (!f) return 1;
    double t1 = now();
    size_t n = jf_count(f), ok = 0;
    for (size_t i = 0; i < n; i++) { JEntry e; if (!jf_entry(f, i, &e)) ok++; jentry_free(&e); }
    double t2 = now();
    jf_close(f);
    JQuery q;
    jq_parse("limit=1000", &q, 5000);
    char *paths[1] = { (char *)p };
    JEntry *r;
    double t3 = now();
    long nr = jq_run(paths, 1, &q, &r);
    double t4 = now();
    jq_free(r, nr);
    printf("%zu entries (%zu ok): open %.1f ms, all %.1f ms (%.0f entries/s); last 1000: %.1f ms\n",
           n, ok, (t1 - t0) * 1e3, (t2 - t1) * 1e3, ok / (t2 - t1 > 0 ? t2 - t1 : 1e-9), (t4 - t3) * 1e3);
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 3 && !strcmp(argv[1], "dump")) return dump(argv[2]);
    if (argc == 3 && !strcmp(argv[1], "bench")) return bench(argv[2]);
    const uint64_t b1 = 1790000000000000ull, b2 = 1790000000500000ull;
    write_file("build/jtest-normal.journal", 0, b1);
    write_file("build/jtest-compact.journal", 1, b2);
    check_file("build/jtest-normal.journal", 0, b1);
    check_file("build/jtest-compact.journal", 1, b2);

    /* LZ4 directly: an invalid distance must not read before the start */
    { unsigned char out[64]; const unsigned char bad[] = { 0x14, 'a', 9, 0 };
      CHECK(lz4_block_decode(bad, sizeof bad, out, sizeof out) == -1);
      const unsigned char big[] = { 0xf0, 0xff, 0xff };
      CHECK(lz4_block_decode(big, sizeof big, out, sizeof out) == -1); }

    /* queries over both files, merged by time */
    char *paths[] = { "build/jtest-normal.journal", "build/jtest-compact.journal" };
    JQuery q;
    JEntry *r;
    CHECK(jq_parse("limit=100", &q, 5000) == 0);
    long n = jq_run(paths, 2, &q, &r);
    CHECK(n == 18);
    for (long i = 1; i < n; i++) CHECK(r[i - 1].realtime <= r[i].realtime);
    CHECK(n > 1 && r[0].realtime == b1 && r[1].realtime == b2);
    jq_free(r, n);
    CHECK(jq_parse("prio=2\nlimit=100", &q, 5000) == 0);
    n = jq_run(paths, 2, &q, &r);
    CHECK(n == 6);                               /* 0, 1, 2 from each file */
    jq_free(r, n);
    CHECK(jq_parse("match=impar\n", &q, 5000) == 0);
    n = jq_run(paths, 2, &q, &r);
    CHECK(n == 6);
    jq_free(r, n);
    CHECK(jq_parse("match=pipewire\ngrep=ENTRADA 3", &q, 5000) == 0);
    n = jq_run(paths, 2, &q, &r);
    CHECK(n == 2);
    jq_free(r, n);
    CHECK(jq_parse("kernel=1", &q, 5000) == 0);
    n = jq_run(paths, 2, &q, &r);
    CHECK(n == 2);
    jq_free(r, n);
    CHECK(jq_parse("limit=3", &q, 5000) == 0);
    n = jq_run(paths, 2, &q, &r);
    CHECK(n == 3 && r[2].realtime == b2 + 8000000);   /* the newest ones */
    jq_free(r, n);
    { char t[64]; snprintf(t, sizeof t, "since=%llu", (unsigned long long)(b1 + 6000000));
      CHECK(jq_parse(t, &q, 5000) == 0);
      n = jq_run(paths, 2, &q, &r);
      CHECK(n == 5);                             /* 7 and 8 from normal, 6..8 from compact */
      jq_free(r, n); }
    CHECK(jq_parse("since=1790000000", &q, 5000) == 0 && q.since == 1790000000000000ull);

    /* query validation */
    CHECK(jq_parse("match=a;b", &q, 5000) == -1);
    CHECK(jq_parse("prio=9", &q, 5000) == -1);
    CHECK(jq_parse("otra=1", &q, 5000) == -1);
    CHECK(jq_parse("limit=5001", &q, 5000) == -1);
    CHECK(jq_parse("grep=a\x01", &q, 5000) == -1);
    CHECK(jq_parse("since=12a", &q, 5000) == -1);

    /* JSON: control chars, quotes and invalid UTF-8 */
    { JEntry e = { 0 }; char buf[256];
      e.message = strdup("a\x01" "b\"c\\" "\xff\xc3(\xe2\x82\xac");
      e.priority = 3; e.pid = -1; e.uid = 5;
      CHECK(jentry_json(&e, buf, sizeof buf, 100) > 0);
      CHECK(strstr(buf, "\"m\":\"a\\u0001b\\\"c\\\\\xef\xbf\xbd\xef\xbf\xbd(\xe2\x82\xac\"") != NULL);
      for (char *p = buf; *p; p++) CHECK((unsigned char)*p >= 32);
      CHECK(jentry_json(&e, buf, 10, 100) == 0);         /* doesn't fit: 0, without overrunning */
      free(e.message); }

    /* files truncated at every length (in steps of 13 bytes) and randomly changed bytes */
    size_t len;
    unsigned char *orig = slurp("build/jtest-compact.journal", &len);
    CHECK(orig != NULL);
    for (size_t cut = 0; orig && cut < len; cut += 13) {
        spit("build/jtest-cut.journal", orig, cut);
        long k = read_all("build/jtest-cut.journal");
        CHECK(k <= 9);
    }
    unsigned seed = 12345;
    for (int it = 0; orig && it < 3000; it++) {
        unsigned char *m = malloc(len);
        memcpy(m, orig, len);
        for (int k = 0; k < 8; k++) {
            seed = seed * 1103515245u + 12345u;
            size_t at = (seed >> 8) % len;
            seed = seed * 1103515245u + 12345u;
            m[at] = (unsigned char)(seed >> 16);
        }
        spit("build/jtest-fuzz.journal", m, len);
        read_all("build/jtest-fuzz.journal");
        free(m);
    }
    /* array chain with a loop: the second points to the first */
    if (orig) {
        unsigned char *m = malloc(len);
        memcpy(m, orig, len);
        uint64_t first = 0;
        for (int i = 0; i < 8; i++) first |= (uint64_t)m[176 + i] << (8 * i);
        uint64_t second = 0;
        for (int i = 0; i < 8; i++) second |= (uint64_t)m[first + 16 + i] << (8 * i);
        put64(m + second + 16, first);
        put64(m + 152, 1000000);                 /* and a lying entry count */
        spit("build/jtest-loop.journal", m, len);
        JFile *f = jf_open("build/jtest-loop.journal");
        CHECK(f && jf_count(f) == 9);
        jf_close(f);
        free(m);
    }
    free(orig);
    CHECK(jf_open("build/no-existe.journal") == NULL);
    spit("build/jtest-cut.journal", (const unsigned char *)"LPKSHHRH", 8);
    CHECK(jf_open("build/jtest-cut.journal") == NULL);
    unlink("build/jtest-cut.journal");
    unlink("build/jtest-fuzz.journal");
    unlink("build/jtest-loop.journal");
    if (fails) { fprintf(stderr, "journal-test: %d failures\n", fails); return 1; }
    printf("journal-test: ok (normal, compact, LZ4, XZ, flagged ZSTD, queries, JSON, truncation, 3000 random, loop)\n");
    return 0;
}
