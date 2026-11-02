/* ctap_test.c — the FIDO2 layer without hardware: CBOR (RFC 8949, appendix A, rejections and random
 * mutations), P-256 (ECDH and k·G vectors generated with python3-cryptography 43), the math of
 * PIN protocol 1 / hmac-secret (against the same vectors), CTAPHID (reassembling a response
 * from several packets, with KEEPALIVE, another channel and stray continuations in between) against a
 * fake device over a socketpair, and fidodev.c's recognition of HID descriptors. */
#include "vault/cbor.h"
#include "vault/p256.h"
#include "vault/ctap2.h"
#include "vault/fidodev.h"
#include "vault/vcrypto.h"
#include "pkg/crypto.h"

#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int fails, total;
static void check(int ok, const char *what) { total++; if (!ok) { fails++; printf("FAIL: %s\n", what); } }

static size_t unhex(const char *h, uint8_t *out) {
    size_t k = 0;
    for (; h[0] && h[1]; h += 2) { unsigned v; sscanf(h, "%2x", &v); out[k++] = (uint8_t)v; }
    return k;
}
static int eqhex(const uint8_t *b, size_t n, const char *h) { uint8_t x[512]; size_t k = unhex(h, x); return k == n && !memcmp(b, x, n); }

/* ---------- CBOR ---------- */

/* re-encodes a decoded item (canonical round-trip test) */
static int reenc(const Cb *c, int i, CbW *w) {
    const CbItem *it = &c->it[i];
    switch (it->type) {
    case CB_UINT: cbw_uint(w, it->v); break;
    case CB_NINT: { uint8_t b = 0x20; (void)b;
        /* -1 - v: where v can be 2^64-1 */
        if (it->v < 24) { uint8_t x = (uint8_t)(0x20 | it->v); cbw_raw(w, &x, 1); }
        else { uint8_t hb[9]; int n; uint64_t v = it->v;
            if (v <= 0xff) { hb[0] = 0x38; hb[1] = (uint8_t)v; n = 2; }
            else if (v <= 0xffff) { hb[0] = 0x39; hb[1] = (uint8_t)(v >> 8); hb[2] = (uint8_t)v; n = 3; }
            else if (v <= 0xffffffffu) { hb[0] = 0x3a; for (int k = 0; k < 4; k++) hb[1 + k] = (uint8_t)(v >> (24 - 8 * k)); n = 5; }
            else { hb[0] = 0x3b; for (int k = 0; k < 8; k++) hb[1 + k] = (uint8_t)(v >> (56 - 8 * k)); n = 9; }
            cbw_raw(w, hb, (size_t)n); }
        break; }
    case CB_BYTES: cbw_bytes(w, it->ptr, it->len); break;
    case CB_TEXT: cbw_textn(w, (const char *)it->ptr, it->len); break;
    case CB_ARRAY: case CB_MAP: {
        if (it->type == CB_ARRAY) cbw_array(w, it->v); else cbw_map(w, it->v);
        int k = i + 1;
        uint64_t n = it->type == CB_ARRAY ? it->v : it->v * 2;
        for (uint64_t j = 0; j < n; j++) { reenc(c, k, w); k = c->it[k].next; }
        break; }
    case CB_FALSE: cbw_bool(w, 0); break;
    case CB_TRUE: cbw_bool(w, 1); break;
    case CB_NULL: cbw_null(w); break;
    case CB_UNDEF: { uint8_t x = 0xf7; cbw_raw(w, &x, 1); break; }
    }
    return w->err;
}

static int roundtrip(const uint8_t *d, size_t n) {
    CbItem it[256];
    Cb c;
    if (cb_parse(&c, d, n, it, 256, 0)) return 0;
    uint8_t out[1024];
    CbW w;
    cbw_init(&w, out, sizeof out);
    reenc(&c, 0, &w);
    return !w.err && w.n == n && !memcmp(out, d, n);
}

static void test_cbor(void) {
    static const char *ok[] = {
        "00", "01", "0a", "17", "1818", "1819", "1864", "1903e8", "1a000f4240", "1b000000e8d4a51000",
        "1bffffffffffffffff", "3bffffffffffffffff", "20", "29", "3863", "3903e7", "f4", "f5", "f6", "f7",
        "40", "4401020304", "60", "6161", "6449455446", "62225c", "62c3bc", "63e6b0b4", "64f0908591",
        "80", "83010203", "8301820203820405",
        "98190102030405060708090a0b0c0d0e0f101112131415161718181819",
        "a0", "a201020304", "a26161016162820203", "826161a161626163", "a56161614161626142616361436164614461656145",
    };
    for (size_t i = 0; i < sizeof ok / sizeof *ok; i++) {
        uint8_t b[256];
        size_t n = unhex(ok[i], b);
        char what[96];
        snprintf(what, sizeof what, "CBOR RFC 8949 A: %s", ok[i]);
        check(roundtrip(b, n), what);
    }
    /* the writer alone */
    {
        uint8_t out[64];
        CbW w;
        cbw_init(&w, out, sizeof out);
        cbw_map(&w, 2); cbw_text(&w, "a"); cbw_int(&w, 1); cbw_text(&w, "b"); cbw_array(&w, 2); cbw_uint(&w, 2); cbw_uint(&w, 3);
        check(eqhex(out, w.n, "a26161016162820203"), "CBOR write {a:1,b:[2,3]}");
        cbw_init(&w, out, sizeof out);
        cbw_int(&w, -1000); cbw_int(&w, -25); cbw_uint(&w, 1000000000000ull);
        check(eqhex(out, w.n, "3903e738181b000000e8d4a51000"), "CBOR write integers");
        cbw_init(&w, out, 3);
        cbw_bytes(&w, "abcd", 4);
        check(w.err, "CBOR write: a small buffer gives an error");
    }
    /* what is rejected: floats, tags, indefinite lengths, reserved simple values, non-minimal encodings,
     * truncation, invalid UTF-8, trailing bytes, nesting too deep */
    static const char *bad[] = {
        "f90000", "fa47c35000", "fb3ff199999999999a", "c074323031332d30332d32315432303a30343a30305a",
        "5f42010243030405ff", "9f018202039f0405ffff", "bf61610161629f0203ffff", "7f657374726561646d696e67ff",
        "f0", "f818", "fc", "1817", "190017", "1a0000ffff", "1b00000000ffffffff", "3817", "5800", "7800",
        "1a0001", "62c3", "61ff", "62c328", "63eda080", "c3", "8301", "a201", "0000", "",
        "818181818181818181818181818181818101",
        "9bffffffffffffffff", "bbffffffffffffffff",
    };
    for (size_t i = 0; i < sizeof bad / sizeof *bad; i++) {
        uint8_t b[256];
        size_t n = unhex(bad[i], b);
        CbItem it[64];
        Cb c;
        char what[96];
        snprintf(what, sizeof what, "CBOR rejects %s", bad[i][0] ? bad[i] : "(empty)");
        check(cb_parse(&c, b, n, it, 64, 0) != 0, what);
    }
    /* with allow_trailing: used says where it ended */
    {
        uint8_t b[8];
        size_t n = unhex("820102ffee", b);
        CbItem it[8];
        Cb c;
        check(cb_parse(&c, b, n, it, 8, 1) == 0 && c.used == 3, "CBOR prefix: used");
    }
    /* map lookup */
    {
        uint8_t b[64];
        size_t n = unhex("a3010238180c626964a1617801", b);   /* {1:2, -25:12, "id":{"x":1}} */
        CbItem it[16];
        Cb c;
        int64_t v;
        int ok2 = cb_parse(&c, b, n, it, 16, 0) == 0;
        ok2 = ok2 && cb_int(&c, cb_map_int(&c, 0, 1), &v) == 0 && v == 2;
        ok2 = ok2 && cb_int(&c, cb_map_int(&c, 0, -25), &v) == 0 && v == 12;
        int m = cb_map_text(&c, 0, "id");
        ok2 = ok2 && cb_type(&c, m) == CB_MAP && cb_int(&c, cb_map_text(&c, m, "x"), &v) == 0 && v == 1;
        ok2 = ok2 && cb_map_int(&c, 0, 7) == -1 && cb_map_text(&c, 0, "nada") == -1;
        check(ok2, "CBOR maps: int and text keys");
    }
    /* random mutations: it never crashes, and whatever it accepts re-encodes identically (canonical) */
    {
        uint64_t s = 0x9e3779b97f4a7c15ull;
        int accepted = 0, bad_rt = 0;
        for (int round = 0; round < 200000; round++) {
            uint8_t b[64];
            const char *src = ok[round % (sizeof ok / sizeof *ok)];
            size_t n = unhex(src, b);
            s ^= s << 13; s ^= s >> 7; s ^= s << 17;
            int muts = 1 + (int)(s % 3);
            for (int m = 0; m < muts && n; m++) {
                s ^= s << 13; s ^= s >> 7; s ^= s << 17;
                size_t pos = (size_t)(s >> 8) % n;
                switch (s % 4) {
                case 0: b[pos] ^= (uint8_t)(1u << ((s >> 32) % 8)); break;
                case 1: b[pos] = (uint8_t)(s >> 40); break;
                case 2: n = pos; break;                                    /* truncate */
                default: if (n < sizeof b) { memmove(b + pos + 1, b + pos, n - pos); b[pos] = (uint8_t)(s >> 48); n++; } break;
                }
            }
            CbItem it[64];
            Cb c;
            if (cb_parse(&c, b, n, it, 64, 0) == 0) { accepted++; if (!roundtrip(b, n)) bad_rt++; }
        }
        printf("cbor: 200000 mutations, %d accepted\n", accepted);
        check(bad_rt == 0, "CBOR mutations: what is accepted is canonical");
    }
    /* more items than the caller's array */
    {
        uint8_t b[64];
        size_t n = unhex("98190102030405060708090a0b0c0d0e0f101112131415161718181819", b);
        CbItem it[10];
        Cb c;
        check(cb_parse(&c, b, n, it, 10, 0) != 0, "CBOR: item limit");
    }
}

/* ---------- P-256 ---------- */

static const char *ECDH[][7] = {
    { "e7345e4dfe1500181b1d2863b7db19dcf358d41af25bef24707a70424007d218", "a39179a9978bfcad4ab237b97d403b60f1d4df456936364bbc91ae8d1f77da7c", "57aa5b7d0b3a282f791e8d4e96ad9a31c4cf585640b8458a6efefa201cdb1766", "97299f68bc9efa27f90a5934e0d8e4c0e34099c0b02f2f9d18478594a007ef88", "07b16bd47246dc0a286b5be8d2d17822deae2c577548f02d0f84a7b7be749e7e", "bb58e26c09249a6a4f12e08ce1a14af47f70cd3c604e5cc579b44d63fe394c82", "359761009157477986a649f762b8ea8520960e23a12e333b70e984544d313c05" },
    { "88779a5bebc8071c03709c424f7722adbd6d963944ff1893e4e5f6b2cc83cd4c", "ae1ba8d225f82bf1172839f2021f031a5aef6af3344a74add21535e3cb4ba3e2", "388291c4f877963b0e08b58977be5effffbbc474f4e79012b3238f41fb772f6b", "a09141d187f8020e0fbb77a5764f7238bf9f02bf36608d645f4471a7a6b016c4", "90455db08fa7bcc634aecce1c89359bd0395100068a84b66503badca1c3aee45", "c2d1f2b1570b2ab6e068f2d1a1633d159104da0fb3e450618c6ced013d0d5d9b", "e64e6b865a3448efa2db76a9152a2a32c8bd9ce4bbe613edcd27e7c9383f22ff" },
    { "39c0f8cfa669ddc5ae75504b1853bf2f4403858ed6665de4fd5f5c8c06c9172e", "2b1b84837d67df87db12151bb5d2470f2265fa5de46982059b3e9ff5bea1d9cb", "26e67fe5cd5c6e03f8b18f315552d19edddaed7932d826eddb64b9a60476d49c", "700a7460a08955011d5ee41f18a028db62fcf2b71a606d13251507564fd17a3b", "47ea7461b4866d3011004de45a456c8127555c18afb81007992ed59307a7bc2e", "a92d2ee9650a9ea6aa49f46b654e4709038716b37b2ff643c0ea6df03a7af388", "0182d5e3f79b20bb1f8b1d154cf515941c426595e8fb701b498a612865c18211" },
    { "445f594d88b97690aee0916af051ee3a1768eeaab487c24322b6fba2ef6b613d", "d8e485d0da3cbf25280f429db8a0789ec4ac7fd2d01443f6a5909d26ae5c2a79", "f7139e54ee36dde90bf790f2f124800fc8a201a8700812e0754684a63d0ff9b5", "e90457b0aadab4521e91199f2ae67b424073bed5421612c60343fa069e50c9fd", "46e090c156c0ea047f6ed220642ebbae035a62257be5178f0e641410274d2779", "98a09366effad2502e77756b3d17557a975d35c70098ccf467586d2a39922e70", "2170619bca2e2049c16d5b6f4fefc6cc980cec7a7f1b16a8543b1d90854688ba" },
    { "8053f76fbf6338546aeecdc79fa9eb033421020232d5704af92fe96afeca9850", "35f6bffba82c1d928309471e57a1ff3e61b2c2236732455cb62e93c00189c19b", "0c74adf6819e9c26ee1d9395b2de4423c5e42d1f60205499c0219ae4f0d3b3d4", "3cc2eee3f4fe18944138e575a8b0db96e1f591d7094fbb183f94f0b8ffc61722", "47995a06d15d53885aa051b9a28d06f18dc6ed3d9d9d9990412369552d34f703", "99ae6e453fab3050a046b87a5c4d019d1a75f9705a07958f3ba470bf8d0d419f", "8c5a576d201a574f15ce6d1596f5048a0d9463bebf7e2725b23177e3fce62d86" },
};
static const char *BASE[][3] = {
    { "0000000000000000000000000000000000000000000000000000000000000001", "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296", "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5" },
    { "0000000000000000000000000000000000000000000000000000000000000002", "7cf27b188d034f7e8a52380304b51ac3c08969e277f21b35a60b48fc47669978", "07775510db8ed040293d9ac69f7430dbba7dade63ce982299e04b79d227873d1" },
    { "0000000000000000000000000000000000000000000000000000000000000003", "5ecbe4d1a6330a44c8f7ef951d4bf165e6c6b721efada985fb41661bc6e7fd6c", "8734640c4998ff7e374b06ce1a64a2ecd82ab036384fb83d9a79b127a27d5032" },
    { "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632550", "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296", "b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a" },
    { "00000000000000000000000000000000000000000000000000000000deadbeef", "b487d183dc4806058eb31a29bedefd7bcca987b77a381a3684871d8449c18394", "2a122cc711a80453678c3032de4b6fff2c86342e82d1e7adb617c4165c43ce5e" },
};

static void test_p256(void) {
    uint8_t k[32], x[32], y[32], z[32];
    for (size_t i = 0; i < sizeof BASE / sizeof *BASE; i++) {
        unhex(BASE[i][0], k);
        check(p256_mul_base(k, x, y) == 0 && eqhex(x, 32, BASE[i][1]) && eqhex(y, 32, BASE[i][2]), "P-256 k·G");
    }
    unhex("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551", k);   /* n */
    check(p256_mul_base(k, x, y) == -1, "P-256 n·G = infinity");
    check(p256_public(k, x, y) == -1, "P-256 private key = n is rejected");
    memset(k, 0, 32);
    check(p256_public(k, x, y) == -1, "P-256 private key 0 is rejected");
    for (size_t i = 0; i < sizeof ECDH / sizeof *ECDH; i++) {
        uint8_t d1[32], d2[32], x1[32], y1[32], x2[32], y2[32];
        unhex(ECDH[i][0], d1); unhex(ECDH[i][1], d2);
        check(p256_public(d1, x1, y1) == 0 && eqhex(x1, 32, ECDH[i][2]) && eqhex(y1, 32, ECDH[i][3]), "P-256 public 1");
        check(p256_public(d2, x2, y2) == 0 && eqhex(x2, 32, ECDH[i][4]) && eqhex(y2, 32, ECDH[i][5]), "P-256 public 2");
        check(p256_ecdh(d1, x2, y2, z) == 0 && eqhex(z, 32, ECDH[i][6]), "P-256 ECDH d1·Q2");
        check(p256_ecdh(d2, x1, y1, z) == 0 && eqhex(z, 32, ECDH[i][6]), "P-256 ECDH d2·Q1");
        check(p256_on_curve(x1, y1), "P-256 on the curve");
        y1[31] ^= 1;
        check(!p256_on_curve(x1, y1) && p256_ecdh(d2, x1, y1, z) == -1, "P-256 point off the curve is rejected");
    }
    /* coordinate >= p */
    memset(x, 0xff, 32);
    memset(y, 0xff, 32);
    check(!p256_on_curve(x, y), "P-256 coordinate >= p is rejected");
    /* random pair: symmetric ECDH */
    {
        uint8_t a[32], ax[32], ay[32], b[32], bx[32], by[32], z1[32], z2[32];
        int ok = 1;
        for (int i = 0; i < 20 && ok; i++) {
            ok = p256_keypair(a, ax, ay) == 0 && p256_keypair(b, bx, by) == 0 &&
                 p256_ecdh(a, bx, by, z1) == 0 && p256_ecdh(b, ax, ay, z2) == 0 && !memcmp(z1, z2, 32);
        }
        check(ok, "P-256 20 random pairs: symmetric ECDH");
    }
}

/* ---------- PIN protocol 1 / hmac-secret ---------- */

static void test_pinproto(void) {
    uint8_t d1[32], d2[32], x2[32], y2[32], z[32], key[32], h[32], enc[32], mac[32], iv[16];
    unhex(ECDH[0][0], d1); unhex(ECDH[0][1], d2);
    p256_public(d2, x2, y2);
    p256_ecdh(d1, x2, y2, z);
    sha256(z, 32, key);
    check(eqhex(key, 32, "59a9953d0eeb2594edb344fadd5f263d9466bf6b023a7aab8a613242f12d211c"), "PIN 1: sharedSecret = SHA-256(x)");
    Aes a;
    aes_init(&a, key, 256);
    sha256("1234", 4, h);
    memset(iv, 0, 16);
    aes_cbc_encrypt(&a, iv, h, enc, 16);
    check(eqhex(enc, 16, "1c8f0cdfe6cbc48221058411827dc33b"), "PIN 1: pinHashEnc");
    uint8_t salt[32];
    for (int i = 0; i < 32; i++) salt[i] = (uint8_t)i;
    memset(iv, 0, 16);
    aes_cbc_encrypt(&a, iv, salt, enc, 32);
    check(eqhex(enc, 32, "3cfd59a28b89586e0b84a6f765d8095966af4a77a74a7b1d5cc3667d24ba4ffb"), "hmac-secret: saltEnc");
    hmac_sha256(key, 32, enc, 32, mac);
    check(eqhex(mac, 16, "136d3e6bdb1297da1a47bc83df8ff340"), "hmac-secret: saltAuth");
    aes_wipe(&a);
}

/* ---------- CTAPHID with a fake device ---------- */

static int dev_cbor_caps = 0x04;
static uint8_t info_cbor[512];
static size_t info_len;

static void fake_send(int s, uint32_t cid, uint8_t cmd, const uint8_t *d, size_t n, int noise) {
    uint8_t p[64];
    if (noise) {
        memset(p, 0, 64); p[0] = 0xde; p[1] = 0xad; p[2] = 0xbe; p[3] = 0xef; p[4] = 0x90; p[6] = 3; p[7] = 9;   /* another channel */
        send(s, p, 64, 0);
        memset(p, 0, 64); p[0] = (uint8_t)(cid >> 24); p[1] = (uint8_t)(cid >> 16); p[2] = (uint8_t)(cid >> 8); p[3] = (uint8_t)cid;
        p[4] = 0xbb; p[6] = 1; p[7] = 2;                        /* KEEPALIVE UPNEEDED */
        send(s, p, 64, 0);
        p[4] = 0x05;                                            /* stray continuation */
        send(s, p, 64, 0);
    }
    memset(p, 0, 64);
    p[0] = (uint8_t)(cid >> 24); p[1] = (uint8_t)(cid >> 16); p[2] = (uint8_t)(cid >> 8); p[3] = (uint8_t)cid;
    p[4] = cmd; p[5] = (uint8_t)(n >> 8); p[6] = (uint8_t)n;
    size_t k = n < 57 ? n : 57, off = k;
    memcpy(p + 7, d, k);
    send(s, p, 64, 0);
    for (uint8_t seq = 0; off < n; seq++) {
        memset(p + 4, 0, 60);
        p[4] = seq;
        k = n - off < 59 ? n - off : 59;
        memcpy(p + 5, d + off, k);
        off += k;
        send(s, p, 64, 0);
        if (noise && seq == 0) {                                /* keepalive between continuations */
            uint8_t q[64];
            memcpy(q, p, 4); memset(q + 4, 0, 60); q[4] = 0xbb; q[6] = 1; q[7] = 1;
            send(s, q, 64, 0);
        }
    }
}

static int fake_packets;
static void *fake_dev(void *arg) {
    int s = *(int *)arg;
    uint8_t in[65], msg[1024];
    size_t total = 0, got = 0;
    uint8_t cmd = 0;
    uint32_t cid = 0;
    for (;;) {
        ssize_t r = recv(s, in, sizeof in, 0);
        if (r <= 0) break;
        if (r != 65 || in[0] != 0) { printf("FAIL: the client did not write 65 bytes with report 0 (%zd)\n", r); fails++; break; }
        fake_packets++;
        uint8_t *p = in + 1;
        if (p[4] & 0x80) { cid = (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; cmd = p[4]; total = (size_t)p[5] << 8 | p[6]; got = total < 57 ? total : 57; memcpy(msg, p + 7, got); }
        else { size_t k = total - got < 59 ? total - got : 59; memcpy(msg + got, p + 5, k); got += k; }
        if (got < total) continue;
        if (cmd == 0x86) {
            uint8_t rsp[17];
            memcpy(rsp, msg, 8);
            rsp[8] = 0x11; rsp[9] = 0x22; rsp[10] = 0x33; rsp[11] = 0x44;
            rsp[12] = 2; rsp[13] = 5; rsp[14] = 4; rsp[15] = 3; rsp[16] = (uint8_t)dev_cbor_caps;
            /* first a reply to ANOTHER init (different nonce): the client has to ignore it */
            uint8_t other[17];
            memcpy(other, rsp, 17);
            other[0] ^= 0xff;
            fake_send(s, 0xffffffffu, 0x86, other, 17, 0);
            fake_send(s, 0xffffffffu, 0x86, rsp, 17, 0);
        } else if (cmd == 0x90 && msg[0] == 0x04) {
            uint8_t rsp[600];
            rsp[0] = 0;
            memcpy(rsp + 1, info_cbor, info_len);
            fake_send(s, cid, 0x90, rsp, info_len + 1, 1);
        } else if (cmd == 0x90 && msg[0] == 0x06) {
            uint8_t rsp[] = { 0x00, 0xa1, 0x03, 0x07 };        /* {3: 7} retries */
            fake_send(s, cid, 0x90, rsp, sizeof rsp, 0);
        } else if (cmd == 0xc1) {
            fake_send(s, cid, 0xc1, msg, total, 0);            /* echo */
        }
    }
    return NULL;
}

static void test_ctaphid(void) {
    /* getInfo like a real key's (CTAP 2.1 PRE): 4 packets */
    CbW w;
    cbw_init(&w, info_cbor, sizeof info_cbor);
    cbw_map(&w, 6);
    cbw_uint(&w, 1); cbw_array(&w, 3); cbw_text(&w, "U2F_V2"); cbw_text(&w, "FIDO_2_0"); cbw_text(&w, "FIDO_2_1_PRE");
    cbw_uint(&w, 2); cbw_array(&w, 2); cbw_text(&w, "credProtect"); cbw_text(&w, "hmac-secret");
    uint8_t aaguid[16];
    unhex("ee882879721c491397753dfcce97072a", aaguid);
    cbw_uint(&w, 3); cbw_bytes(&w, aaguid, 16);
    cbw_uint(&w, 4); cbw_map(&w, 5);
    cbw_text(&w, "rk"); cbw_bool(&w, 1); cbw_text(&w, "up"); cbw_bool(&w, 1); cbw_text(&w, "plat"); cbw_bool(&w, 0);
    cbw_text(&w, "clientPin"); cbw_bool(&w, 1); cbw_text(&w, "credentialMgmtPreview"); cbw_bool(&w, 1);
    cbw_uint(&w, 5); cbw_uint(&w, 1200);
    cbw_uint(&w, 6); cbw_array(&w, 1); cbw_uint(&w, 1);
    info_len = w.n;
    for (int round = 0; round < 2; round++) {
        dev_cbor_caps = round == 0 ? 0x04 : 0x00;
        int sv[2];
        socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv);
        pthread_t th;
        pthread_create(&th, NULL, fake_dev, &sv[1]);
        FidoInfo info;
        int rc = fido_get_info(sv[0], &info);
        if (round == 0) {
            check(rc == 0, "CTAPHID: getInfo in several packets");
            check(!strcmp(info.versions, "U2F_V2,FIDO_2_0,FIDO_2_1_PRE"), "CTAPHID: versions");
            check(info.hmac_secret == 1 && info.client_pin == 1 && info.pin_retries == 7, "CTAPHID: hmac-secret, PIN and retries");
            check(!memcmp(info.aaguid, aaguid, 16), "CTAPHID: aaguid");
            uint8_t echo[200], back[200];
            for (int i = 0; i < 200; i++) echo[i] = (uint8_t)(i * 7);
            size_t bl = 0;
            rc = ctaphid_command(sv[0], 0xc1, echo, sizeof echo, back, sizeof back, &bl, 2000);
            check(rc == 0 && bl == 200 && !memcmp(echo, back, 200), "CTAPHID: 200-byte raw command (round trip)");
        } else {
            check(rc == 0 && info.hmac_secret == 0, "CTAPHID: U2F-only key → no hmac-secret");
            uint8_t cred[8] = { 1 }, salt[32] = { 0 }, out[32];
            check(fido_hmac_secret(sv[0], NULL, cred, 8, salt, out, 1000, NULL, NULL) == FIDO_ERR_NO_HMAC, "CTAPHID: U2F → FIDO_ERR_NO_HMAC");
        }
        close(sv[0]);
        pthread_join(th, NULL);
        close(sv[1]);
    }
    /* nobody answers: the INIT deadline expires */
    {
        int sv[2];
        socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv);
        FidoInfo info;
        check(fido_get_info(sv[0], &info) == FIDO_ERR_TIMEOUT, "CTAPHID: no answer → timeout");
        close(sv[0]);
        close(sv[1]);
    }
    check(!strcmp(fido_strerror(FIDO_ERR_PIN_REQUIRED), "pin-required"), "fido_strerror");
}

/* ---------- HID descriptors ---------- */

static void test_desc(void) {
    uint8_t d[64];
    size_t n = unhex("06d0f10901a1010920150026ff007508954081020921150026ff00750895409102c0", d);
    check(fidodev_desc_is_fido(d, n), "HID: FIDO descriptor");
    n = unhex("0b0100d0f1a101c0", d);                        /* 32-bit Usage (page in the high 16 bits) */
    check(fidodev_desc_is_fido(d, n), "HID: 32-bit Usage");
    n = unhex("05010906a101050719e029e7150025017501950881020507a102c0c0", d);   /* keyboard */
    check(!fidodev_desc_is_fido(d, n), "HID: a keyboard is not FIDO");
    n = unhex("06d0f10901a1", d);                            /* cut in the middle */
    check(!fidodev_desc_is_fido(d, n), "HID: truncated");
    n = unhex("06d0f10902a101c0", d);                        /* another usage on the same page */
    check(!fidodev_desc_is_fido(d, n), "HID: FIDO page with another usage");
    check(fidodev_open("/etc/passwd") == -1 && fidodev_open("/dev/hidraw../x") == -1, "fidodev_open: paths that are not hidraw");
}

int main(void) {
    test_cbor();
    test_p256();
    test_pinproto();
    test_ctaphid();
    test_desc();
    printf("ctap-test: %d/%d\n", total - fails, total);
    return fails ? 1 : 0;
}
