/* vcrypto_test.c — the vault's own primitives against the official vectors:
 * AES (FIPS-197 appendix C, SP 800-38A F.2 CBC), SHA-1 (FIPS 180), HMAC-SHA1 (RFC 2202),
 * HMAC-SHA256/512 (RFC 4231), HKDF-SHA256 (RFC 5869) and the secrets arena (smem). */
#include "vault/vcrypto.h"
#include "vault/smem.h"
#include "pkg/crypto.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails, total;
static void check(int ok, const char *what) { total++; if (!ok) { fails++; printf("FAIL: %s\n", what); } }

static void unhex(const char *h, uint8_t *out, size_t *n) {
    size_t k = 0;
    for (; h[0] && h[1]; h += 2) { unsigned v; sscanf(h, "%2x", &v); out[k++] = (uint8_t)v; }
    if (n) *n = k;
}
static int eqhex(const uint8_t *b, size_t n, const char *h) { uint8_t x[512]; size_t k; unhex(h, x, &k); return k == n && !memcmp(b, x, n); }

int main(void) {
    uint8_t key[64], in[256], out[256], iv[16];
    size_t kl, il;

    /* FIPS-197 C.1 / C.3 */
    const char *pt = "00112233445566778899aabbccddeeff";
    struct { int bits; const char *k, *ct; } fv[] = {
        { 128, "000102030405060708090a0b0c0d0e0f", "69c4e0d86a7b0430d8cdb78070b4c55a" },
        { 192, "000102030405060708090a0b0c0d0e0f1011121314151617", "dda97ca4864cdfe06eaf70a0ec0d7191" },
        { 256, "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f", "8ea2b7ca516745bfeafc49904b496089" },
    };
    for (int i = 0; i < 3; i++) {
        Aes a;
        unhex(fv[i].k, key, &kl);
        unhex(pt, in, &il);
        aes_init(&a, key, fv[i].bits);
        aes_encrypt(&a, in, out);
        check(eqhex(out, 16, fv[i].ct), "AES encrypt (FIPS-197)");
        aes_decrypt(&a, out, out);
        check(!memcmp(out, in, 16), "AES decrypt (FIPS-197)");
    }
    /* SP 800-38A F.2.5 CBC-AES256 */
    {
        Aes a;
        unhex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4", key, &kl);
        unhex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e5130c81c46a35ce411e5fbc1191a0a52eff69f2445df4f9b17ad2b417be66c3710", in, &il);
        unhex("000102030405060708090a0b0c0d0e0f", iv, NULL);
        aes_init(&a, key, 256);
        aes_cbc_encrypt(&a, iv, in, out, il);
        check(eqhex(out, il, "f58c4c04d6e5f1ba779eabfb5f7bfbd69cfc4e967edb808d679f777bc6702c7d39f23369a9d9bacfa530e26304231461b2eb05e2c39be9fcda6c19078c6a9d1b"), "AES-256-CBC (SP 800-38A)");
        unhex("000102030405060708090a0b0c0d0e0f", iv, NULL);
        aes_cbc_decrypt(&a, iv, out, out, il);
        check(!memcmp(out, in, il), "AES-256-CBC decrypt");
    }
    {
        uint8_t p[32] = { 0 };
        memset(p + 20, 12, 12);
        check(pkcs7_unpad(p, 32) == 20, "PKCS#7 valid");
        p[25] = 11;
        check(pkcs7_unpad(p, 32) == -1, "PKCS#7 invalid");
    }

    /* SHA-1 */
    uint8_t h[64];
    sha1("abc", 3, h);
    check(eqhex(h, 20, "a9993e364706816aba3e25717850c26c9cd0d89d"), "SHA-1 abc");
    sha1("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq", 56, h);
    check(eqhex(h, 20, "84983e441c3bd26ebaae4aa1f95129e5e54670f1"), "SHA-1 448 bits");
    {
        Sha1 s;
        sha1_init(&s);
        char a[1000];
        memset(a, 'a', sizeof a);
        for (int i = 0; i < 1000; i++) sha1_update(&s, a, sizeof a);
        sha1_final(&s, h);
        check(eqhex(h, 20, "34aa973cd4c4daa4f61eeb2bdbad27316534016f"), "SHA-1 million a's");
    }

    /* RFC 2202 HMAC-SHA1 cases 1, 2, 6 */
    memset(key, 0x0b, 20);
    hmac_sha1(key, 20, (const uint8_t *)"Hi There", 8, h);
    check(eqhex(h, 20, "b617318655057264e28bc0b6fb378c8ef146be00"), "HMAC-SHA1 case 1");
    hmac_sha1((const uint8_t *)"Jefe", 4, (const uint8_t *)"what do ya want for nothing?", 28, h);
    check(eqhex(h, 20, "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"), "HMAC-SHA1 case 2");
    {
        uint8_t k80[80];
        memset(k80, 0xaa, 80);
        hmac_sha1(k80, 80, (const uint8_t *)"Test Using Larger Than Block-Size Key - Hash Key First", 54, h);
        check(eqhex(h, 20, "aa4ae5e15272d00e95705637ce8a3b55ed402112"), "HMAC-SHA1 case 6");
    }
    /* RFC 4231 cases 1 and 2 */
    memset(key, 0x0b, 20);
    hmac_sha256(key, 20, (const uint8_t *)"Hi There", 8, h);
    check(eqhex(h, 32, "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"), "HMAC-SHA256 case 1");
    hmac_sha256((const uint8_t *)"Jefe", 4, (const uint8_t *)"what do ya want for nothing?", 28, h);
    check(eqhex(h, 32, "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"), "HMAC-SHA256 case 2");
    hmac_sha512(key, 20, (const uint8_t *)"Hi There", 8, h);
    check(eqhex(h, 64, "87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cdedaa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854"), "HMAC-SHA512 case 1");
    /* RFC 5869 A.1 */
    {
        uint8_t ikm[22], salt[13], info[10], okm[42];
        memset(ikm, 0x0b, 22);
        unhex("000102030405060708090a0b0c", salt, NULL);
        unhex("f0f1f2f3f4f5f6f7f8f9", info, NULL);
        hkdf_sha256(salt, 13, ikm, 22, info, 10, okm, 42);
        check(eqhex(okm, 42, "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"), "HKDF-SHA256 A.1");
        /* A.3: no salt or info */
        hkdf_sha256(NULL, 0, ikm, 22, NULL, 0, okm, 42);
        check(eqhex(okm, 42, "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"), "HKDF-SHA256 A.3");
    }

    /* secrets arena */
    {
        sm_harden_process();
        char *p[2000];
        for (int i = 0; i < 2000; i++) { p[i] = sm_alloc((size_t)(i % 300) + 1); if (p[i]) memset(p[i], 'x', (size_t)(i % 300) + 1); }
        int ok = 1;
        for (int i = 0; i < 2000; i++) if (!p[i]) ok = 0;
        check(ok, "smem: 2000 blocks");
        for (int i = 0; i < 2000; i += 2) sm_free(p[i]);
        char *q = sm_alloc(40);
        int zero = 1;
        for (int i = 0; i < 40; i++) if (q[i]) zero = 0;
        check(zero, "smem: reused and zeroed");
        char *big = sm_alloc(3 << 20);
        check(big && sm_size(big) >= (3u << 20), "smem: large block");
        big = sm_realloc(big, 5 << 20);
        check(big != NULL, "smem: large realloc");
        SmStats st;
        sm_stats(&st);
        printf("smem: in use %zu KB, mapped %zu KB, mlocked %zu KB (%s)\n", st.in_use / 1024, st.mapped / 1024, st.locked / 1024, st.all_locked ? "all" : "partial");
        sm_wipe_all();
        sm_stats(&st);
        check(st.in_use == 0 && st.mapped == 0, "smem: wipe all");
        char *r = sm_strdup("después de borrar");
        check(r && !strcmp(r, "después de borrar"), "smem: still works");
        sm_free(r);
    }

    printf("vcrypto-test: %d/%d\n", total - fails, total);
    return fails ? 1 : 0;
}
