/* ipp_test.c — our own IPP encoder and decoder (src/print/ipp.c), mDNS reader
 * (src/print/mdns.c) and page ranges (src/print/cups.c). Includes broken messages: truncated at
 * every byte and with random byte changes, which must be rejected without breaking anything. */
#include "print/ipp.h"
#include "print/mdns.h"
#include "print/cups.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails, total;
#define CHECK(c, ...) do { total++; if (!(c)) { fails++; printf("FAIL %s:%d: ", __FILE__, __LINE__); printf(__VA_ARGS__); printf("\n"); } } while (0)

static void t_roundtrip(void) {
    IppBuf b;
    ipp_init(&b, IPP_OP_PRINT_JOB, 42);
    ipp_std(&b, "ipp://localhost/printers/PDF", "lord");
    ipp_add_str(&b, IPP_T_NAME, "job-name", "Informe ñ.pdf");
    ipp_add_str(&b, IPP_T_MIMETYPE, "document-format", "application/pdf");
    ipp_group(&b, IPP_G_JOB);
    ipp_add_int(&b, IPP_T_INTEGER, "copies", 3);
    ipp_add_range(&b, "page-ranges", 1, 3);
    ipp_add_range(&b, NULL, 5, 5);
    ipp_add_str(&b, IPP_T_KEYWORD, "sides", "two-sided-long-edge");
    ipp_add_bool(&b, "x-bool", 1);
    ipp_add_int(&b, IPP_T_ENUM, "orientation-requested", 4);
    ipp_end(&b);
    CHECK(!b.err, "codificar");
    /* what follows the message is the document data */
    size_t n = b.n;
    uint8_t *buf = malloc(n + 4);
    memcpy(buf, b.p, n);
    memcpy(buf + n, "%PDF", 4);
    IppMsg m;
    CHECK(ipp_parse(buf, n + 4, &m) == 0, "decodificar");
    CHECK(m.major == 1 && m.minor == 1 && m.code == IPP_OP_PRINT_JOB && m.request_id == 42, "encabezado");
    CHECK(m.used == n, "used=%zu n=%zu", m.used, n);
    CHECK(!strcmp(ipp_str(ipp_find(&m, NULL, IPP_G_OPERATION, "attributes-charset")), "utf-8"), "charset");
    CHECK(!strcmp(ipp_str(ipp_find(&m, NULL, 0, "job-name")), "Informe ñ.pdf"), "job-name");
    CHECK(ipp_int(ipp_find(&m, NULL, IPP_G_JOB, "copies"), 0) == 3, "copies");
    const IppAttr *pr = ipp_find(&m, NULL, IPP_G_JOB, "page-ranges");
    CHECK(pr && pr->nvals == 2 && pr->vals->i == 1 && pr->vals->i2 == 3 && pr->vals->next->i == 5, "page-ranges");
    CHECK(ipp_int(ipp_find(&m, NULL, IPP_G_JOB, "x-bool"), 0) == 1, "bool");
    CHECK(ipp_int(ipp_find(&m, NULL, IPP_G_JOB, "orientation-requested"), 0) == 4, "enum");
    CHECK(!ipp_find(&m, NULL, IPP_G_OPERATION, "copies"), "grupo");
    ipp_msg_free(&m);
    free(buf);
    ipp_free(&b);
}

/* A response like CUPS-Get-Printers': two printers, a collection and textWithLanguage. */
static size_t build_response(uint8_t *out) {
    IppBuf b;
    ipp_init(&b, 0x0000, 7);
    ipp_std(&b, NULL, NULL);
    ipp_add_str(&b, IPP_T_TEXT, "status-message", "successful-ok");
    ipp_group(&b, IPP_G_PRINTER);
    ipp_add_str(&b, IPP_T_NAME, "printer-name", "PDF");
    ipp_add_int(&b, IPP_T_ENUM, "printer-state", 3);
    ipp_add_str(&b, IPP_T_KEYWORD, "media-supported", "iso_a4_210x297mm");
    ipp_add_str(&b, IPP_T_KEYWORD, NULL, "na_letter_8.5x11in");
    ipp_group(&b, IPP_G_PRINTER);
    ipp_add_str(&b, IPP_T_NAME, "printer-name", "Oficina");
    ipp_add_int(&b, IPP_T_ENUM, "printer-state", 5);
    ipp_end(&b);
    /* hand-built collection before the end: media-col = { media-size-name: "a4" } */
    size_t n = b.n - 1;
    memcpy(out, b.p, n);
    const uint8_t col[] = {
        IPP_T_BEGCOL, 0, 9, 'm', 'e', 'd', 'i', 'a', '-', 'c', 'o', 'l', 0, 0,
        IPP_T_MEMBERNAME, 0, 0, 0, 4, 's', 'i', 'z', 'e',
        IPP_T_KEYWORD, 0, 0, 0, 2, 'a', '4',
        IPP_T_ENDCOL, 0, 0, 0, 0,
        IPP_T_TEXTLANG, 0, 4, 'i', 'n', 'f', 'o', 0, 12, 0, 2, 'e', 's', 0, 6, 'h', 'o', 'l', 'a', '!', '!',
        IPP_G_END,
    };
    memcpy(out + n, col, sizeof col);
    ipp_free(&b);
    return n + sizeof col;
}

static void t_response(void) {
    uint8_t buf[1024];
    size_t n = build_response(buf);
    IppMsg m;
    CHECK(ipp_parse(buf, n, &m) == 0, "respuesta");
    const IppAttr *a = ipp_find(&m, NULL, IPP_G_PRINTER, "printer-name");
    const IppAttr *b = ipp_find(&m, a, IPP_G_PRINTER, "printer-name");
    CHECK(a && b && !strcmp(ipp_str(a), "PDF") && !strcmp(ipp_str(b), "Oficina"), "dos impresoras");
    CHECK(a && b && a->gseq != b->gseq, "grupos separados");
    CHECK(ipp_int(ipp_find_in(a, "printer-state"), 0) == 3 && ipp_int(ipp_find_in(b, "printer-state"), 0) == 5, "state per group");
    const IppAttr *ms = ipp_find_in(a, "media-supported");
    CHECK(ms && ms->nvals == 2 && !strcmp(ms->vals->next->str, "na_letter_8.5x11in"), "varios valores");
    CHECK(!ipp_find_in(b, "media-supported"), "does not look into another printer's group");
    const IppAttr *mc = ipp_find(&m, NULL, 0, "media-col");
    CHECK(mc && mc->vals->tag == IPP_T_BEGCOL && mc->vals->col && !strcmp(mc->vals->col->name, "size") &&
          !strcmp(mc->vals->col->vals->str, "a4"), "collection");
    CHECK(!strcmp(ipp_str(ipp_find(&m, NULL, 0, "info")) ? ipp_str(ipp_find(&m, NULL, 0, "info")) : "", "hola!!"), "textWithLanguage");
    ipp_msg_free(&m);

    /* truncated at every byte: always -1, never an out-of-bounds access */
    int bad = 0;
    for (size_t k = 0; k < n; k++) { if (ipp_parse(buf, k, &m) == 0) { bad++; ipp_msg_free(&m); } }
    CHECK(bad == 0, "%d cortes aceptados", bad);
    /* random byte changes: may accept or reject, but without breaking (run with ASAN) */
    srand(1234);
    int ok = 0;
    for (int it = 0; it < 20000; it++) {
        uint8_t mut[1024];
        memcpy(mut, buf, n);
        for (int j = 0; j < 1 + rand() % 4; j++) mut[rand() % n] = (uint8_t)rand();
        if (!ipp_parse(mut, n, &m)) { ok++; ipp_msg_free(&m); }
    }
    total++;
    printf("ipp: 20000 mutations, %d accepted without memory errors\n", ok);
    /* largos imposibles */
    const uint8_t huge[] = { 1, 1, 0, 0, 0, 0, 0, 1, 1, IPP_T_NAME, 0xff, 0xff, 'a' };
    CHECK(ipp_parse(huge, sizeof huge, &m) == -1, "name longer than the message");
    const uint8_t noattr[] = { 1, 1, 0, 0, 0, 0, 0, 1, 1, IPP_T_NAME, 0, 0, 0, 1, 'x', 3 };
    CHECK(ipp_parse(noattr, sizeof noattr, &m) == -1, "extra value without an attribute");
    const uint8_t badint[] = { 1, 1, 0, 0, 0, 0, 0, 1, 1, IPP_T_INTEGER, 0, 1, 'n', 0, 2, 0, 1, 3 };
    CHECK(ipp_parse(badint, sizeof badint, &m) == -1, "2-byte integer");
    uint8_t deep[600];
    size_t d = 0;
    const uint8_t hdr[] = { 1, 1, 0, 0, 0, 0, 0, 1, 1 };
    memcpy(deep, hdr, sizeof hdr);
    d = sizeof hdr;
    for (int i = 0; i < 40 && d + 20 < sizeof deep; i++) {
        const uint8_t c1[] = { IPP_T_BEGCOL, 0, (uint8_t)(i ? 0 : 1), 'c' };
        memcpy(deep + d, c1, i ? 3 : 4);
        d += i ? 3 : 4;
        deep[d++] = 0; deep[d++] = 0;
        const uint8_t mem[] = { IPP_T_MEMBERNAME, 0, 0, 0, 1, 'm' };
        memcpy(deep + d, mem, sizeof mem);
        d += sizeof mem;
    }
    deep[d++] = 3;
    CHECK(ipp_parse(deep, d, &m) == -1, "too deeply nested collections");
}

/* Hand-built mDNS response: PTR + SRV + TXT + A, with compressed names. */
static size_t put_name(uint8_t *p, const char *name) {
    size_t o = 0;
    const char *s = name;
    while (*s) {
        const char *dot = strchr(s, '.');
        size_t l = dot ? (size_t)(dot - s) : strlen(s);
        p[o++] = (uint8_t)l;
        memcpy(p + o, s, l);
        o += l;
        s += l + (dot ? 1 : 0);
    }
    p[o++] = 0;
    return o;
}

static void t_mdns(void) {
    uint8_t p[512];
    memset(p, 0, sizeof p);
    p[2] = 0x84;                                   /* respuesta, autoritativa */
    p[7] = 1; p[11] = 3;                           /* 1 respuesta, 3 adicionales */
    size_t o = 12;
    size_t svc = o;
    o += put_name(p + o, "_ipp._tcp.local");
    p[o + 1] = 12; p[o + 3] = 1; p[o + 7] = 120;    /* PTR IN ttl */
    size_t rl = o + 8;
    o += 10;
    size_t inst = o;
    p[o++] = 7; memcpy(p + o, "Oficina", 7); o += 7;
    p[o++] = 0xc0; p[o++] = (uint8_t)svc;          /* "Oficina" + puntero a _ipp._tcp.local */
    p[rl + 1] = (uint8_t)(o - rl - 2);
    /* SRV: name = pointer to the instance */
    p[o++] = 0xc0; p[o++] = (uint8_t)inst;
    p[o + 1] = 33; p[o + 3] = 1; p[o + 7] = 120;
    rl = o + 8;
    o += 10;
    size_t srv = o;
    p[o + 4] = 0x02; p[o + 5] = 0x77;              /* puerto 631 */
    o += 6;
    size_t host = o;
    o += put_name(p + o, "impre.local");
    p[rl + 1] = (uint8_t)(o - srv);
    /* TXT */
    p[o++] = 0xc0; p[o++] = (uint8_t)inst;
    p[o + 1] = 16; p[o + 3] = 1; p[o + 7] = 120;
    rl = o + 8;
    o += 10;
    size_t txt = o;
    const char *kv[] = { "rp=ipp/print", "ty=Impresora de prueba", "Color=T" };
    for (int i = 0; i < 3; i++) { p[o++] = (uint8_t)strlen(kv[i]); memcpy(p + o, kv[i], strlen(kv[i])); o += strlen(kv[i]); }
    p[rl + 1] = (uint8_t)(o - txt);
    /* A */
    p[o++] = 0xc0; p[o++] = (uint8_t)host;
    p[o + 1] = 1; p[o + 3] = 1; p[o + 7] = 120; p[o + 9] = 4;
    o += 10;
    p[o++] = 192; p[o++] = 168; p[o++] = 1; p[o++] = 20;
    MdnsCache *c = calloc(1, sizeof *c);
    CHECK(mdns_parse(p, o, c) == 0, "mdns parse");
    const char *types[] = { "_ipp._tcp" };
    MdnsSvc s[4];
    int n = mdns_collect(c, types, 1, s, 4);
    CHECK(n == 1, "one service (%d)", n);
    if (n == 1) {
        char v[64];
        CHECK(!strcmp(s[0].name, "Oficina") && !strcmp(s[0].host, "impre.local") && s[0].port == 631 && !strcmp(s[0].addr, "192.168.1.20"),
              "servicio %s %s %d %s", s[0].name, s[0].host, s[0].port, s[0].addr);
        CHECK(!strcmp(mdns_txt(&s[0], "rp", v, sizeof v), "ipp/print") && !strcmp(mdns_txt(&s[0], "ty", v, sizeof v), "Impresora de prueba"), "txt");
        CHECK(!mdns_txt(&s[0], "nope", v, sizeof v)[0], "txt ausente");
    }
    /* truncated at every byte, and a pointer pointing at itself */
    for (size_t k = 0; k < o; k++) { memset(c, 0, sizeof *c); mdns_parse(p, k, c); }
    total++;
    uint8_t loop[] = { 0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0xc0, 12, 0, 12, 0, 1, 0, 0, 0, 0, 0, 0 };
    memset(c, 0, sizeof *c);
    CHECK(mdns_parse(loop, sizeof loop, c) == -1, "pointer loop");
    free(c);
}

static void t_ranges(void) {
    int lo[32], hi[32];
    CHECK(cups_page_ranges("1-3,5, 8 - 9", lo, hi, 32) == 3 && lo[0] == 1 && hi[0] == 3 && lo[1] == 5 && hi[1] == 5 && lo[2] == 8 && hi[2] == 9, "ranges");
    CHECK(cups_page_ranges("", lo, hi, 32) == 0, "empty");
    CHECK(cups_page_ranges("3-1", lo, hi, 32) == -1, "reversed");
    CHECK(cups_page_ranges("0", lo, hi, 32) == -1, "zero");
    CHECK(cups_page_ranges("1;2", lo, hi, 32) == -1, "separator");
    CHECK(cups_page_ranges("a", lo, hi, 32) == -1, "letters");
    CHECK(cups_valid_name("Oficina_2-B") && !cups_valid_name("a b") && !cups_valid_name("../x") && !cups_valid_name(""), "nombres");
    CHECK(cups_valid_uri("ipp://192.168.1.20:631/ipp/print") && !cups_valid_uri("file:///etc/passwd") && !cups_valid_uri("ipp://a b") &&
          !cups_valid_uri("cups-pdf:/"), "uris");
}

int main(void) {
    t_roundtrip();
    t_response();
    t_mdns();
    t_ranges();
    printf("ipp-test: %d/%d\n", total - fails, total);
    return fails ? 1 : 0;
}
