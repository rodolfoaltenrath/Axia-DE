/* remote_fuzz.c — the remote desktop's parsers under ASan+UBSan with random and mutated inputs
 * (HTTP heads, login forms, cookies, WebSocket streams, JSON messages, addresses, the lockout file).
 * Built by `make -C server test` with -fsanitize=address,undefined; any memory error or undefined
 * behaviour aborts. Usage: remote-fuzz [iterations] [seed]. */
#include "remote/rsec.h"
#include "remote/ipnet.h"
#include "remote/lockout.h"
#include "remote/rtcwire.h"
#include "rtc_offer_chrome.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned long long st = 88172645463325252ULL;
static unsigned rnd(void) { st ^= st << 13; st ^= st >> 7; st ^= st << 17; return (unsigned)(st >> 11); }

static const char *const seeds[] = {
    "GET / HTTP/1.1\r\nHost: 10.0.0.5:5673\r\n\r\n",
    "POST /login HTTP/1.1\r\nHost: a:5673\r\nOrigin: https://a:5673\r\nContent-Type: application/x-www-form-urlencoded\r\n"
    "Content-Length: 20\r\nCookie: __Host-lordos_remote=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n\r\nuser=lord&pass=x",
    "GET /ws?w=1280&h=800 HTTP/1.1\r\nHost: [fe80::1]:5673\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
    "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nSec-Fetch-Site: same-origin\r\n\r\n",
    "user=lord&pass=a%20b+c%26",
    "{\"t\":\"m\",\"x\":10,\"y\":20}",
    "{\"t\":\"clip\",\"text\":\"h\\u00e9\\ud83d\\udc51\\n\"}",
    "{\"t\":\"pz\",\"p\":2,\"s\":65536,\"r\":-90}",
    "192.168.1.0/24", "2001:db8::/32", "::ffff:10.0.0.1",
    "10.0.0.1 2 1000 0 0 0\n2001:db8:1:2::/64 0 1990 1 1990 2500\n",
    "allow 203.0.113.0/24\nblock 192.168.1.66\n",
};
static const char alphabet[] = "\r\n :%&=+/?#\"\\{}[],.-_0123456789abcdefABCDEFghijkluvwxyzGETPOSTHosUpgrade\x00\x01\x7f\x80\xc3\xa9\xed\xff";

static size_t mutate(char *b, size_t n, size_t cap) {
    int k = 1 + (int)(rnd() % 8);
    while (k--) {
        unsigned op = rnd() % 6;
        if (op == 0 && n) b[rnd() % n] = alphabet[rnd() % (sizeof alphabet - 1)];
        else if (op == 1 && n) b[rnd() % n] ^= (char)(1 << (rnd() % 8));
        else if (op == 2 && n + 1 < cap) { size_t p = n ? rnd() % n : 0; memmove(b + p + 1, b + p, n - p); b[p] = alphabet[rnd() % (sizeof alphabet - 1)]; n++; }
        else if (op == 3 && n) { size_t p = rnd() % n; memmove(b + p, b + p + 1, n - p - 1); n--; }
        else if (op == 4 && n && n * 2 < cap) { size_t p = rnd() % n, l = 1 + rnd() % (n - p); memcpy(b + n, b + p, l); n += l; }
        else if (op == 5) n = n ? rnd() % n : 0;
    }
    return n;
}

static void on(void *ud, int op, unsigned char *d, size_t l) {
    (void)ud;
    if (op == 1) {
        char *sb = malloc(l + 1);
        RsJson o;
        RsInput in;
        if (sb && !rs_json_parse((const char *)d, l, &o, sb, l + 1)) rs_browser_input(&o, 1280, 800, &in);
        free(sb);
    }
}

static void one(const char *b, size_t n) {
    /* each parser gets an exact-size heap copy, so ASan sees any byte read past the end */
    char *c = malloc(n + 1);
    memcpy(c, b, n);
    RsReq r;
    rs_parse_head(c, n, &r);
    char u[33], p[RS_PASS_MAX + 1], raw[33], tok[65];
    size_t pl;
    rs_parse_login(c, n, u, p, &pl, raw);
    {
        RsJson o;
        RsInput in;
        char *sb = malloc(n + 1);
        if (!rs_json_parse(c, n, &o, sb, n + 1)) rs_browser_input(&o, (int)(rnd() % 3000), (int)(rnd() % 3000), &in);
        free(sb);
    }
    rs_utf8_valid((unsigned char *)c, n);
    {
        RsWs w;
        rs_ws_init(&w, 1 + rnd() % 70000);
        unsigned char *m = malloc(n ? n : 1);
        memcpy(m, c, n);
        size_t pos = 0, end = 0, used = 0;
        while (end < n) {                       /* the stream arrives in random pieces, as TLS records do */
            end += 1 + rnd() % (n - end);
            if (rs_ws_consume(&w, m + pos, end - pos, &used, on, NULL)) break;
            pos += used;
        }
        free(m);
        rs_ws_free(&w);
    }
    c[n] = 0;                                   /* the string functions get a C string */
    rs_cookie_token(c, "__Host-lordos_remote", tok);
    rs_valid_host(c);
    rs_same_origin(c, "a:5673");
    rs_form_type(c);
    rs_has_token(c, "upgrade");
    rs_valid_wskey(c);
    rs_valid_username(c);
    RsCidr cd;
    RsAddr ad;
    if (!rs_cidr_parse(c, &cd)) { char t[64]; rs_cidr_format(&cd, t, sizeof t); RsCidr back; if (rs_cidr_parse(t, &back) || !rs_cidr_equal(&back, &cd)) abort(); }
    if (!rs_addr_parse(c, &ad)) { char t[64]; rs_addr_key(&ad, t, sizeof t); rs_addr_is_local(&ad); }
    static RlTable t;
    rl_parse(&t, c, 1e9);
    char out[RL_MAX * 128 + 256];
    rl_format(&t, 1e9, out, sizeof out);
    rl_unblock(&t, c);
    RsAccess acc;
    rs_access_parse(c, &acc, NULL);
    char at[8192];
    rs_access_format(&acc, at, sizeof at);
    free(c);
}

/* ---- the video path (rtcwire.c): SDP offers, candidates, STUN/TURN, RTCP, DCEP, RTP payloads ---- */

static void fake_hmac(const uint8_t *key, size_t kl, const uint8_t *m, size_t n, uint8_t out[20]) {
    uint32_t h = 2166136261u;
    for (size_t i = 0; i < kl; i++) h = (h ^ key[i]) * 16777619u;
    for (size_t i = 0; i < n; i++) h = (h ^ m[i]) * 16777619u;
    for (int i = 0; i < 20; i++) out[i] = (uint8_t)(h >> (i % 4 * 8));
}
static void pk(void *ud, const uint8_t *h, size_t hl, const uint8_t *d, size_t n, int marker) {
    (void)ud; (void)marker;
    volatile uint8_t x = 0;
    for (size_t i = 0; i < hl; i++) x ^= h[i];
    for (size_t i = 0; i < n; i++) x ^= d[i];                  /* ASan checks every byte handed out */
}
static void fb(void *ud, const RwFb *f) { (void)ud; (void)f; }

static void rtc_one(const uint8_t *b, size_t n) {
    uint8_t *c = malloc(n + 1);
    memcpy(c, b, n);
    RwOffer *o = malloc(sizeof *o);
    if (!rw_sdp_parse_offer((const char *)c, n, (int)(rnd() & 1), o)) {
        static char ans[16384];
        uint8_t fp[32] = { 0 };
        RwLocalCand lc[2] = { { "192.168.1.5", 4000, RW_CAND_HOST, 1, "", 0, 0 }, { "192.168.1.5", 5673, RW_CAND_HOST, 1, "", 0, 1 } };
        RwAnswer a = { "ufraguf", "passwordpasswordpassword", fp, 1, 2, 1, lc, 2, 3, (unsigned)(rnd() % 5) };
        rw_sdp_answer(o, &a, ans, (size_t)(1 + rnd() % sizeof ans));
    }
    free(o);
    RwCand cd;
    rw_cand_parse((const char *)c, n, &cd);
    RwStun st;
    if (!rw_stun_parse(c, n, &st)) rw_stun_check_mi(c, n, &st, (const uint8_t *)"k", 1, fake_hmac);
    rw_is_stun(c, n);
    rw_rtcp_parse(c, n, fb, NULL);
    RwDcOpen dc;
    rw_dcep_open(c, n, &dc);
    rw_h264_packetize(c, n, 64 + rnd() % 1500, pk, NULL);
    rw_h264_is_idr(c, n);
    rw_vp8_packetize(c, n, 64 + rnd() % 1500, pk, NULL);
    /* ICE-TCP framing, the TURN stream, the root child's route (all on untrusted bytes) */
    size_t fl = 0, off = 0;
    while (off < n && rw_tcp_frame(c + off, n - off, &fl) == 1) { char route[9]; rw_ice_route(c + off + 2, fl, route); off += 2 + fl; }
    for (off = 0; off < n && rw_stun_stream(c + off, n - off, &fl) == 1; off += fl) { RwStun s2; rw_stun_parse(c + off, fl, &s2); }
    { char route[9]; rw_ice_route(c, n, route); }
    c[n] = 0;
    rw_turn_transport((const char *)c);
    { uint8_t pin[32]; rw_fp_parse((const char *)c, pin); }
    rw_ice_server_valid((const char *)c);
    rw_turn_user_valid((const char *)c);
    rw_turn_pass_valid((const char *)c);
    RwAddr ad;
    if (!rw_addr_text((const char *)c, &ad)) { char t[64]; rw_addr_format(&ad, t, sizeof t); }
    free(c);
}

/* seeds for rtc_one: well-formed STUN/TURN messages and RTCP compounds, built here */
static size_t rtc_seed(uint8_t *b, size_t cap) {
    uint8_t tid[12];
    for (int i = 0; i < 12; i++) tid[i] = (uint8_t)rnd();
    RwStunOut o;
    RwAddr a = { (int)(rnd() % 2 ? 4 : 6), { 10, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13 }, (uint16_t)rnd() };
    switch (rnd() % 8) {
    case 0: {
        size_t n = strlen(chrome_offer);
        memcpy(b, chrome_offer, n);
        return mutate((char *)b, n, cap);
    }
    case 1: {
        static const char *const cs[] = { "candidate:1 1 udp 2113937151 192.168.1.20 54321 typ host generation 0 network-cost 999",
                                          "candidate:3 1 UDP 33554431 2001:db8::7 3478 typ relay raddr 2001:db8::1 rport 9",
                                          "candidate:4 1 udp 2113937151 3d9863a1-3b3d-48f6-9ff1-49c2bc04877f.local 64198 typ host",
                                          "stun.example.org:3478", "[2001:db8::1]:3478" };
        const char *s = cs[rnd() % 5];
        size_t n = strlen(s);
        memcpy(b, s, n);
        return mutate((char *)b, n, 2048);
    }
    case 2: case 3:
        rw_so_init(&o, (uint16_t)(rnd() % 2 ? RW_STUN_BINDING_REQ : RW_TURN_ALLOCATE_OK), tid);
        if (rnd() % 2) { const char *u = rnd() % 2 ? "abcd:efgh" : "Ab3dEf7hWxYz:efgh"; rw_so_attr(&o, RW_ATTR_USERNAME, u, strlen(u)); }
        if (rnd() % 2) rw_so_u32(&o, RW_ATTR_PRIORITY, rnd());
        if (rnd() % 2) rw_so_xaddr(&o, RW_ATTR_XOR_MAPPED, &a);
        if (rnd() % 2) rw_so_xaddr(&o, RW_ATTR_XOR_RELAYED, &a);
        if (rnd() % 3 == 0) rw_so_attr(&o, RW_ATTR_DATA, "\x16\xfe\xfd payload", 12);
        if (rnd() % 2) rw_so_mi(&o, (const uint8_t *)"k", 1, fake_hmac);
        if (rnd() % 2) rw_so_fingerprint(&o);
        if (rnd() % 3 == 0 && o.n + 2 <= cap) {       /* framed for ICE-TCP (RFC 4571) */
            b[0] = (uint8_t)(o.n >> 8);
            b[1] = (uint8_t)o.n;
            memcpy(b + 2, o.b, o.n);
            return rnd() % 2 ? mutate((char *)b, o.n + 2, 1500) : o.n + 2;
        }
        memcpy(b, o.b, o.n);
        return rnd() % 2 ? mutate((char *)b, o.n, 1500) : o.n;
    case 4: {
        static const uint8_t rr[] = { 0x81, 0xc9, 0, 7, 1, 1, 1, 1, 2, 2, 2, 2, 25, 0, 0, 5, 0, 0, 1, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0,
                                      0x81, 0xcd, 0, 3, 1, 1, 1, 1, 2, 2, 2, 2, 0, 100, 0, 15,
                                      0x8f, 0xce, 0, 5, 1, 1, 1, 1, 0, 0, 0, 0, 'R', 'E', 'M', 'B', 1, 8, 0xe8, 0, 2, 2, 2, 2 };
        memcpy(b, rr, sizeof rr);
        return mutate((char *)b, sizeof rr, 1500);
    }
    case 5: {
        static const uint8_t op[] = { 3, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 'i', 'n', 'p', 'u', 't' };
        memcpy(b, op, sizeof op);
        return mutate((char *)b, sizeof op, 200);
    }
    default: {                                  /* an Annex B stream of random NAL units */
        size_t n = 0;
        while (n + 2000 < cap && rnd() % 6) {
            static const uint8_t sc[] = { 0, 0, 0, 1 };
            size_t k = 3 + rnd() % 2;
            memcpy(b + n, sc + 4 - k, k);
            n += k;
            size_t l = rnd() % (rnd() % 4 ? 200 : 1800);
            for (size_t i = 0; i < l; i++) b[n++] = (uint8_t)(rnd() % 5 ? rnd() | 1 : 0);
        }
        return n;
    }
    }
}

/* random WebSocket stream: well-formed frames with random flags, lengths and payloads */
static size_t ws_stream(unsigned char *b, size_t cap) {
    size_t o = 0;
    while (o + 200 < cap && rnd() % 8) {
        size_t len = rnd() % 4 == 0 ? rnd() % 70000 : rnd() % 130;
        if (o + len + 14 >= cap) break;
        int op = (int)(rnd() % 11);
        b[o++] = (unsigned char)((rnd() % 4 ? 0x80 : 0) | op | (rnd() % 50 == 0 ? 0x40 : 0));
        if (len < 126) b[o++] = (unsigned char)(0x80 | len);
        else if (len < 65536) { b[o++] = 0x80 | 126; b[o++] = (unsigned char)(len >> 8); b[o++] = (unsigned char)len; }
        else { b[o++] = 0x80 | 127; for (int i = 0; i < 8; i++) b[o++] = (unsigned char)((unsigned long long)len >> (56 - 8 * i)); }
        for (int i = 0; i < 4; i++) b[o++] = (unsigned char)rnd();
        for (size_t i = 0; i < len; i++) b[o++] = (unsigned char)(rnd() % 3 ? (unsigned)(unsigned char)"{\"t\":\"m\",\"x\":1}"[i % 15] : rnd());
    }
    return o;
}

int main(int argc, char **argv) {
    long iters = argc > 1 ? atol(argv[1]) : 100000;
    if (argc > 2) st = strtoull(argv[2], NULL, 10) | 1;
    static char b[200000];
    for (long i = 0; i < iters; i++) {
        size_t n;
        unsigned kind = rnd() % 14;
        if (kind >= 10) {
            n = rtc_seed((uint8_t *)b, 40000);
            rtc_one((const uint8_t *)b, n);
            continue;
        }
        if (kind < 6) {
            const char *s = seeds[rnd() % (sizeof seeds / sizeof *seeds)];
            n = strlen(s);
            memcpy(b, s, n);
            n = mutate(b, n, 16384);
        } else if (kind < 8) {
            n = rnd() % 2048;
            for (size_t k = 0; k < n; k++) b[k] = (char)rnd();
        } else n = ws_stream((unsigned char *)b, sizeof b);
        one(b, n);
    }
    printf("remote-fuzz: %ld inputs, no memory errors or undefined behaviour\n", iters);
    return 0;
}
