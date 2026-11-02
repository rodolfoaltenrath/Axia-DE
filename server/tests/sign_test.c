/* sign_test.c — SHA-256 (NIST), Ed25519 (RFC 8032), the JSON reader and verification of
 * .lpk packages and of the catalog (server/src/sec). No network or files: everything in memory.
 *
 *   build/sign-test                  tests
 *   build/sign-test package.lpk      also verifies a real .lpk (e.g. made with tools/lpk.mjs) */
#include "sec/json.h"
#include "sec/sign.h"
#include "pkg/crypto.h"
#include "errcode.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void sig_test_official(const char *keyhex);

static int fails, checks;
#define CHECK(c, ...) do { checks++; if (!(c)) { fails++; fprintf(stderr, "FAIL %s:%d: ", __FILE__, __LINE__); fprintf(stderr, __VA_ARGS__); fputc('\n', stderr); } } while (0)

static void hexs(const uint8_t *b, size_t n, char *out) { hex_encode(b, n, out); }

static void test_sha256(void) {
    /* FIPS 180-2 / NIST CAVP (SHA-256 examples) */
    static const struct { const char *msg; const char *hex; } v[] = {
        { "", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" },
        { "abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" },
        { "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq", "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1" },
    };
    uint8_t h[32];
    char x[65];
    for (size_t i = 0; i < sizeof v / sizeof v[0]; i++) {
        sha256(v[i].msg, strlen(v[i].msg), h);
        hexs(h, 32, x);
        CHECK(!strcmp(x, v[i].hex), "sha256(\"%s\") = %s", v[i].msg, x);
    }
    /* a million 'a's, in uneven chunks (tests the internal buffer) */
    Sha256 s;
    sha256_init(&s);
    char a[997];
    memset(a, 'a', sizeof a);
    size_t left = 1000000;
    while (left) { size_t n = left < sizeof a ? left : sizeof a; sha256_update(&s, a, n); left -= n; }
    sha256_final(&s, h);
    hexs(h, 32, x);
    CHECK(!strcmp(x, "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"), "sha256 million a = %s", x);
}

static void test_ed25519(void) {
    /* RFC 8032, section 7.1: TEST 1, 2, 3 and SHA(abc) */
    static const struct { const char *seed, *pub, *msg, *sig; } v[] = {
        { "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
          "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a", "",
          "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b" },
        { "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
          "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c", "72",
          "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00" },
        { "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
          "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025", "af82",
          "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a" },
        { "833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42",
          "ec172b93ad5e563bf4932c70e1245034c35467ef2efd4d64ebf819683467e2bf",
          "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
          "dc2a4459e7369633a52b1bf277839a00201009a3efbf3ecb69bea2186c26b58909351fc9ac90b3ecfdfbc7c66431e0303dca179c138ac17ad9bef1177331a704" },
    };
    for (size_t i = 0; i < sizeof v / sizeof v[0]; i++) {
        uint8_t seed[32], pub[32], want_pub[32], sig[64], want[64], msg[64];
        size_t mlen = strlen(v[i].msg) / 2;
        hex_decode(v[i].seed, seed, 32);
        hex_decode(v[i].pub, want_pub, 32);
        hex_decode(v[i].sig, want, 64);
        if (mlen) hex_decode(v[i].msg, msg, mlen);
        ed25519_public(seed, pub);
        CHECK(!memcmp(pub, want_pub, 32), "ed25519 public key %zu", i + 1);
        ed25519_sign(seed, msg, mlen, sig);
        CHECK(!memcmp(sig, want, 64), "ed25519 signature %zu equals RFC 8032", i + 1);
        CHECK(ed25519_verify(want_pub, want, msg, mlen) == 1, "ed25519 verifica %zu", i + 1);
        uint8_t bad[64];
        memcpy(bad, want, 64);
        bad[63] ^= 0x10;
        CHECK(ed25519_verify(want_pub, bad, msg, mlen) == 0, "ed25519 rechaza firma alterada %zu", i + 1);
        if (mlen) { msg[0] ^= 1; CHECK(ed25519_verify(want_pub, want, msg, mlen) == 0, "ed25519 rechaza mensaje alterado %zu", i + 1); }
    }
}

static void test_json(void) {
    JDoc d;
    char err[96];
    const char *ok = "{\"a\": [1, 2.5e1, -3], \"b\": \"\\u00e1\\ud83d\\ude00\\n\", \"c\": {\"d\": true, \"e\": null}}";
    CHECK(!json_parse(ok, strlen(ok), &d, err, sizeof err), "valid json: %s", err);
    if (d.root) {
        const JVal *a = json_get(d.root, "a");
        CHECK(a && a->type == J_ARR && a->len == 3 && a->kid->next->num == 25, "json arreglo");
        CHECK(!strcmp(json_str(d.root, "b"), "\xc3\xa1\xf0\x9f\x98\x80\n"), "json escapes y sustitutos");
        CHECK(json_bool(json_get(d.root, "c"), "d", 0) == 1, "json bool");
        json_free(&d);
    }
    static const char *bad[] = {
        "", "{", "{\"a\":1,}", "[1,]", "{\"a\":1,\"a\":2}", "\"\\u0000\"", "01", "1.", "{\"a\" 1}", "[1] x",
        "\"\\ud800\"", "\"a\nb\"", "tru", "{'a':1}", "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[1]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]",
    };
    for (size_t i = 0; i < sizeof bad / sizeof bad[0]; i++)
        CHECK(json_parse(bad[i], strlen(bad[i]), &d, NULL, 0) != 0, "invalid json accepted: %s", bad[i]);
    char q[64];
    json_quote(q, sizeof q, "a\"b\\c\n");
    CHECK(!strcmp(q, "\"a\\\"b\\\\c\\u000a\""), "json_quote %s", q);
}

/* ---- in-memory packages ---- */

typedef struct { char text[8192]; size_t len; } Txt;
static Txt contents, signature;

static void make_contents(LpkFile *f, int n, const char *id, const char *ver, const char *signer_key) {
    size_t o = (size_t)snprintf(contents.text, sizeof contents.text, "{\"format\":1,\"id\":\"%s\",\"version\":\"%s\",", id, ver);
    if (signer_key) o += (size_t)snprintf(contents.text + o, sizeof contents.text - o, "\"signer\":{\"name\":\"Ana\",\"key\":\"%s\"},", signer_key);
    o += (size_t)snprintf(contents.text + o, sizeof contents.text - o, "\"files\":[");
    for (int i = 0; i < n; i++) {
        uint8_t h[32];
        char x[65];
        sha256(f[i].data, f[i].size, h);
        hex_encode(h, 32, x);
        o += (size_t)snprintf(contents.text + o, sizeof contents.text - o, "%s{\"path\":\"%s\",\"size\":%zu,\"sha256\":\"%s\"}", i ? "," : "", f[i].path, f[i].size, x);
    }
    o += (size_t)snprintf(contents.text + o, sizeof contents.text - o, "]}");
    contents.len = o;
}

static void test_packages(void) {
    uint8_t seed[32], pub[32], seed2[32];
    char key[65];
    memset(seed, 7, 32);
    memset(seed2, 9, 32);
    ed25519_public(seed, pub);
    hex_encode(pub, 32, key);
    static const char manifest[] = "{\"id\":\"hola\",\"version\":\"1.0.0\"}", appjs[] = "var __lordosApp={};";
    LpkFile files[5] = { { "manifest.json", (const uint8_t *)manifest, sizeof manifest - 1 }, { "app.js", (const uint8_t *)appjs, sizeof appjs - 1 } };
    Lpk pk = { NULL, files, 2 };
    LpkSigInfo si;
    char err[160];

    CHECK(!sig_verify_package(&pk, "hola", "1.0.0", &si, err, sizeof err) && si.trust == LPK_UNSIGNED && !si.has_contents, "unsigned: %s", err);

    make_contents(files, 2, "hola", "1.0.0", key);
    files[2] = (LpkFile){ SIG_CONTENTS, (const uint8_t *)contents.text, contents.len };
    sig_make_detached(seed, contents.text, contents.len, signature.text, sizeof signature.text);
    signature.len = strlen(signature.text);
    files[3] = (LpkFile){ SIG_SIGNATURE, (const uint8_t *)signature.text, signature.len };
    pk.nfiles = 4;
    int r = sig_verify_package(&pk, "hola", "1.0.0", &si, err, sizeof err);
    CHECK(!r && si.trust == LPK_SIGNED && !strcmp(si.key, key) && !strcmp(si.signer, "Ana"), "signed: %s", err);

    /* a changed file */
    static const char evil[] = "var __lordosApp={x:1};";
    files[1].data = (const uint8_t *)evil;
    files[1].size = sizeof evil - 1;
    CHECK(sig_verify_package(&pk, "hola", "1.0.0", &si, err, sizeof err) && lerr_has(err, "sig.fileModified") && strstr(err, "path=app.js"), "altered file: %s", err);
    files[1] = (LpkFile){ "app.js", (const uint8_t *)appjs, sizeof appjs - 1 };

    /* an extra file */
    files[4] = (LpkFile){ "extra.js", (const uint8_t *)"x", 1 };
    pk.nfiles = 5;
    CHECK(sig_verify_package(&pk, "hola", "1.0.0", &si, err, sizeof err), "extra file accepted");
    pk.nfiles = 4;

    /* another version or id */
    CHECK(sig_verify_package(&pk, "hola", "2.0.0", &si, err, sizeof err), "another version accepted");
    CHECK(sig_verify_package(&pk, "chau", "1.0.0", &si, err, sizeof err), "another id accepted");

    /* signature by another key over the same contents (the contents key does not match) */
    sig_make_detached(seed2, contents.text, contents.len, signature.text, sizeof signature.text);
    signature.len = strlen(signature.text);
    files[3].size = signature.len;
    CHECK(sig_verify_package(&pk, "hola", "1.0.0", &si, err, sizeof err), "signature by another key accepted");

    /* firma rota */
    sig_make_detached(seed, contents.text, contents.len, signature.text, sizeof signature.text);
    signature.len = strlen(signature.text);
    files[3].size = signature.len;
    char *s = strstr(signature.text, "\"sig\": \"");
    s[9] = s[9] == '0' ? '1' : '0';
    CHECK(sig_verify_package(&pk, "hola", "1.0.0", &si, err, sizeof err), "broken signature accepted");
}

static void test_catalog(void) {
    uint8_t seed[32], pub[32];
    char key[65], sig[400], err[160];
    memset(seed, 3, 32);
    ed25519_public(seed, pub);
    hex_encode(pub, 32, key);
    char cat[1024];
    int n = snprintf(cat, sizeof cat,
        "{\"format\":1,\"serial\":4,\"authors\":[{\"key\":\"%s\",\"name\":\"Ana\"}],"
        "\"packages\":[{\"id\":\"hola\",\"version\":\"1.0.0\",\"size\":10,\"sha256\":\"%064d\"}],"
        "\"revoked\":{\"keys\":[\"%064d\"],\"packages\":[{\"id\":\"vieja\",\"version\":\"0.9\"}]}}", key, 0, 1);
    sig_make_detached(seed, cat, (size_t)n, sig, sizeof sig);
    Catalog c;
    CHECK(catalog_parse(cat, (size_t)n, sig, strlen(sig), &c, err, sizeof err) != 0, "catalog with an unofficial key accepted");
    sig_test_official(key);
    CHECK(!catalog_parse(cat, (size_t)n, sig, strlen(sig), &c, err, sizeof err), "official catalog: %s", err);
    CHECK(c.serial == 4 && c.nauthors == 1 && c.npackages == 1 && c.nrevoked_keys == 1 && c.nrevoked == 1, "catalog: fields");
    catalog_free(&c);
    strstr(cat, "\"serial\":4")[9] = '5';     /* bump the serial without re-signing */
    CHECK(catalog_parse(cat, (size_t)n, sig, strlen(sig), &c, err, sizeof err) != 0, "altered catalog accepted");
    sig_test_official("");
}

static void test_file(const char *path) {
    Lpk pk;
    char err[160];
    if (lpk_open(path, &pk, err, sizeof err)) { CHECK(0, "%s: %s", path, err); return; }
    const LpkFile *m = lpk_find(&pk, "manifest.json");
    JDoc d;
    if (!m || json_parse((const char *)m->data, m->size, &d, err, sizeof err)) { CHECK(0, "%s: manifest", path); lpk_free(&pk); return; }
    LpkSigInfo si;
    int r = sig_verify_package(&pk, json_str(d.root, "id"), json_str(d.root, "version"), &si, err, sizeof err);
    printf("%s: %s, %s %s%s%s\n", path, r ? err : "ok", sig_trust_name(si.trust), si.signer, si.key[0] ? "" : "(no key)",
           si.revoked ? " [REVOKED]" : "");
    CHECK(r == 0, "%s no verifica", path);
    json_free(&d);
    lpk_free(&pk);
}

int main(int argc, char **argv) {
    test_sha256();
    test_ed25519();
    test_json();
    test_packages();
    test_catalog();
    for (int i = 1; i < argc; i++) test_file(argv[i]);
    printf("sign-test: %d/%d passed\n", checks - fails, checks);
    return fails ? 1 : 0;
}
