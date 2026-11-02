/* crypt.c — backup encryption with Monocypher (see crypt.h). */
#include "crypt.h"
#include "monocypher.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <unistd.h>

#define KDF_BLOCKS 65536u               /* 64 MiB */
#define KDF_PASSES 3u

static const uint8_t magic[4] = { 'L', 'B', 'K', '1' };

void bkc_wipe(void *p, size_t n) { crypto_wipe(p, n); }

uint64_t bkc_enc_size(uint64_t plain) {
    uint64_t chunks = plain ? (plain + BKC_CHUNK - 1) / BKC_CHUNK : 1;
    return BKC_HDR + plain + chunks * BKC_MAC;
}

int bkc_plain_size(uint64_t enc, uint64_t *plain) {
    if (enc < BKC_HDR + BKC_MAC) return -1;
    uint64_t body = enc - BKC_HDR;
    uint64_t full = body / (BKC_CHUNK + BKC_MAC), rest = body % (BKC_CHUNK + BKC_MAC);
    uint64_t p;
    if (rest == 0) p = full * BKC_CHUNK;                    /* the last chunk is full */
    else if (rest >= BKC_MAC) p = full * BKC_CHUNK + (rest - BKC_MAC);
    else return -1;
    if (bkc_enc_size(p) != enc) return -1;
    *plain = p;
    return 0;
}

static void chunk_nonce(const uint8_t base[24], uint64_t i, uint8_t out[24]) {
    memcpy(out, base, 24);
    for (int k = 0; k < 8; k++) out[16 + k] ^= (uint8_t)(i >> (8 * k));
}

static void chunk_ad(uint64_t i, int last, uint8_t ad[9]) {
    for (int k = 0; k < 8; k++) ad[k] = (uint8_t)(i >> (8 * k));
    ad[8] = (uint8_t)(last ? 1 : 0);
}

static ssize_t read_full(int fd, uint8_t *b, size_t n) {
    size_t o = 0;
    while (o < n) {
        ssize_t k = read(fd, b + o, n - o);
        if (k < 0 && errno == EINTR) continue;
        if (k < 0) return -1;
        if (k == 0) break;
        o += (size_t)k;
    }
    return (ssize_t)o;
}

static int write_full(int fd, const uint8_t *b, size_t n) {
    size_t o = 0;
    while (o < n) {
        ssize_t k = write(fd, b + o, n - o);
        if (k < 0 && errno == EINTR) continue;
        if (k <= 0) return errno == ENOSPC || errno == EDQUOT ? -2 : -1;
        o += (size_t)k;
    }
    return 0;
}

int bkc_random(uint8_t *buf, size_t n) {
    size_t o = 0;
    while (o < n) {
        ssize_t k = getrandom(buf + o, n - o, 0);
        if (k < 0 && errno == EINTR) continue;
        if (k <= 0) return -1;
        o += (size_t)k;
    }
    return 0;
}

int bkc_encrypt_fd(int in, int out, const uint8_t key[BKC_KEY], int (*tick)(void *ud, size_t n), void *ud) {
    uint8_t hdr[BKC_HDR];
    memcpy(hdr, magic, 4);
    if (bkc_random(hdr + 4, 24)) return -1;
    int r = write_full(out, hdr, sizeof hdr);
    if (r) return r;
    uint8_t *a = malloc(BKC_CHUNK), *b = malloc(BKC_CHUNK), *c = malloc(BKC_CHUNK + BKC_MAC);
    if (!a || !b || !c) { free(a); free(b); free(c); return -1; }
    /* read one chunk ahead to know which one is the last */
    ssize_t n = read_full(in, a, BKC_CHUNK);
    uint64_t i = 0;
    while (n >= 0) {
        ssize_t next = (size_t)n == BKC_CHUNK ? read_full(in, b, BKC_CHUNK) : 0;
        if (next < 0) { r = -1; break; }
        int last = next == 0;
        uint8_t nonce[24], ad[9];
        chunk_nonce(hdr + 4, i, nonce);
        chunk_ad(i, last, ad);
        crypto_aead_lock(c, c + n, key, nonce, ad, sizeof ad, a, (size_t)n);
        if ((r = write_full(out, c, (size_t)n + BKC_MAC))) break;
        if (tick && tick(ud, (size_t)n)) { r = 1; break; }
        if (last) break;
        uint8_t *t = a; a = b; b = t;
        n = next;
        i++;
    }
    if (n < 0) r = -1;
    crypto_wipe(a, BKC_CHUNK);
    crypto_wipe(b, BKC_CHUNK);
    free(a); free(b); free(c);
    return r;
}

int bkc_decrypt_fd(int in, int out, const uint8_t key[BKC_KEY], int (*tick)(void *ud, size_t n), void *ud) {
    uint8_t hdr[BKC_HDR];
    if (read_full(in, hdr, sizeof hdr) != (ssize_t)sizeof hdr || memcmp(hdr, magic, 4)) return -3;
    uint8_t *a = malloc(BKC_CHUNK + BKC_MAC), *b = malloc(BKC_CHUNK + BKC_MAC), *p = malloc(BKC_CHUNK);
    if (!a || !b || !p) { free(a); free(b); free(p); return -1; }
    int r = 0;
    ssize_t n = read_full(in, a, BKC_CHUNK + BKC_MAC);
    uint64_t i = 0;
    for (;;) {
        if (n < (ssize_t)BKC_MAC) { r = n < 0 ? -1 : -3; break; }       /* truncated */
        ssize_t next = (size_t)n == BKC_CHUNK + BKC_MAC ? read_full(in, b, BKC_CHUNK + BKC_MAC) : 0;
        if (next < 0) { r = -1; break; }
        int last = next == 0;
        uint8_t nonce[24], ad[9];
        chunk_nonce(hdr + 4, i, nonce);
        chunk_ad(i, last, ad);
        size_t len = (size_t)n - BKC_MAC;
        if (crypto_aead_unlock(p, a + len, key, nonce, ad, sizeof ad, a, len)) { r = -3; break; }
        if ((r = write_full(out, p, len))) break;
        if (tick && tick(ud, len)) { r = 1; break; }
        if (last) break;
        uint8_t *t = a; a = b; b = t;
        n = next;
        i++;
    }
    crypto_wipe(p, BKC_CHUNK);
    free(a); free(b); free(p);
    return r;
}

int bkc_derive(const char *pass, const uint8_t salt[BKC_SALT], uint8_t key[BKC_KEY]) {
    void *work = malloc((size_t)KDF_BLOCKS * 1024);
    if (!work) return -1;
    crypto_argon2_config cfg = { .algorithm = CRYPTO_ARGON2_ID, .nb_blocks = KDF_BLOCKS, .nb_passes = KDF_PASSES, .nb_lanes = 1 };
    crypto_argon2_inputs inp = { .pass = (const uint8_t *)pass, .salt = salt, .pass_size = (uint32_t)strlen(pass), .salt_size = BKC_SALT };
    crypto_argon2(key, BKC_KEY, work, cfg, inp, crypto_argon2_no_extras);
    crypto_wipe(work, (size_t)KDF_BLOCKS * 1024);
    free(work);
    return 0;
}

void bkc_check(const uint8_t key[BKC_KEY], uint8_t out[16]) {
    static const char label[] = "LordOS-Backups key check v1";
    crypto_blake2b_keyed(out, 16, key, BKC_KEY, (const uint8_t *)label, sizeof label - 1);
}

static void hex(const uint8_t *b, size_t n, char *out) {
    for (size_t i = 0; i < n; i++) sprintf(out + 2 * i, "%02x", b[i]);
}

static int unhex(const char *s, uint8_t *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        unsigned v;
        if (sscanf(s + 2 * i, "%2x", &v) != 1) return -1;
        b[i] = (uint8_t)v;
    }
    return 0;
}

int bkc_info_write(const char *machine, const uint8_t salt[BKC_SALT], const uint8_t check[16]) {
    char p[4200], tmp[4210], hs[2 * BKC_SALT + 1], hc[33];
    if ((size_t)snprintf(p, sizeof p, "%s/" BKC_FILE, machine) >= sizeof p) return -1;
    snprintf(tmp, sizeof tmp, "%s.new", p);
    hex(salt, BKC_SALT, hs);
    hex(check, 16, hc);
    FILE *f = fopen(tmp, "wx");
    if (!f) { unlink(tmp); f = fopen(tmp, "wx"); }
    if (!f) return -1;
    fprintf(f, "{\"v\":1,\"aead\":\"xchacha20-poly1305\",\"chunk\":%u,\"kdf\":\"argon2id\",\"blocks\":%u,\"passes\":%u,\"salt\":\"%s\",\"check\":\"%s\"}\n",
            BKC_CHUNK, KDF_BLOCKS, KDF_PASSES, hs, hc);
    int ok = fflush(f) == 0 && fsync(fileno(f)) == 0;
    ok = fclose(f) == 0 && ok;
    if (!ok || rename(tmp, p)) { unlink(tmp); return -1; }
    return 0;
}

int bkc_info_read(const char *machine, uint8_t salt[BKC_SALT], uint8_t check[16]) {
    char p[4200], buf[512];
    if ((size_t)snprintf(p, sizeof p, "%s/" BKC_FILE, machine) >= sizeof p) return -1;
    int fd = open(p, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) return -1;
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) return -1;
    buf[n] = 0;
    const char *s = strstr(buf, "\"salt\":\""), *c = strstr(buf, "\"check\":\"");
    if (!s || !c || !strstr(buf, "\"v\":1") || unhex(s + 8, salt, BKC_SALT) || unhex(c + 9, check, 16)) return -1;
    return 0;
}

int bkc_unlock(const char *machine, const char *pass, uint8_t key[BKC_KEY]) {
    uint8_t salt[BKC_SALT], check[16], got[16];
    if (bkc_info_read(machine, salt, check)) return -2;
    if (bkc_derive(pass, salt, key)) return -1;
    bkc_check(key, got);
    if (crypto_verify16(got, check)) { crypto_wipe(key, BKC_KEY); return -1; }
    return 0;
}

int bkc_setup(const char *machine, const char *pass, uint8_t key[BKC_KEY]) {
    uint8_t salt[BKC_SALT], check[16];
    if (bkc_random(salt, sizeof salt) || bkc_derive(pass, salt, key)) return -1;
    bkc_check(key, check);
    if (bkc_info_write(machine, salt, check)) { crypto_wipe(key, BKC_KEY); return -1; }
    return 0;
}
