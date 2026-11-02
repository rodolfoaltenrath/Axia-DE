/* totp.c — HOTP/TOTP, base32 and otpauth:// (see totp.h). */
#include "totp.h"
#include "vcrypto.h"

#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

long base32_decode(const char *s, uint8_t *out, size_t cap) {
    uint32_t acc = 0;
    int bits = 0;
    size_t n = 0;
    for (; *s; s++) {
        int c = (unsigned char)*s, v;
        if (c == ' ' || c == '-' || c == '=' || c == '\t' || c == '\n' || c == '\r') continue;
        if (c >= 'a' && c <= 'z') c -= 32;
        if (c >= 'A' && c <= 'Z') v = c - 'A';
        else if (c >= '2' && c <= '7') v = c - '2' + 26;
        else return -1;
        acc = acc << 5 | (uint32_t)v;
        bits += 5;
        if (bits >= 8) {
            bits -= 8;
            if (n >= cap) return -1;
            out[n++] = (uint8_t)(acc >> bits);
        }
    }
    return (long)n;
}

size_t base32_encode(const uint8_t *in, size_t n, char *out, size_t cap) {
    static const char A[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    uint32_t acc = 0;
    int bits = 0;
    size_t o = 0;
    for (size_t i = 0; i < n; i++) {
        acc = acc << 8 | in[i];
        bits += 8;
        while (bits >= 5) { bits -= 5; if (o + 1 < cap) out[o++] = A[(acc >> bits) & 31]; }
    }
    if (bits > 0 && o + 1 < cap) out[o++] = A[(acc << (5 - bits)) & 31];
    if (cap) out[o] = 0;
    return o;
}

/* decodes %XX of a URI into dst (cap bytes) */
static void pct_decode(const char *s, size_t n, char *dst, size_t cap) {
    size_t o = 0;
    for (size_t i = 0; i < n && o + 1 < cap; i++) {
        if (s[i] == '%' && i + 2 < n && isxdigit((unsigned char)s[i + 1]) && isxdigit((unsigned char)s[i + 2])) {
            char h[3] = { s[i + 1], s[i + 2], 0 };
            dst[o++] = (char)strtol(h, NULL, 16);
            i += 2;
        } else dst[o++] = s[i] == '+' ? ' ' : s[i];
    }
    dst[o] = 0;
}

int totp_parse(const char *text, Totp *t) {
    memset(t, 0, sizeof *t);
    t->digits = 6;
    t->period = 30;
    t->algo = TOTP_SHA1;
    while (*text == ' ' || *text == '\t' || *text == '\n') text++;
    if (strncasecmp(text, "otpauth://", 10)) {
        long n = base32_decode(text, t->key, sizeof t->key);
        if (n <= 0) return -1;
        t->keylen = (size_t)n;
        return 0;
    }
    const char *p = text + 10;
    if (!strncasecmp(p, "totp/", 5)) p += 5;
    else if (!strncasecmp(p, "hotp/", 5)) { p += 5; t->hotp = 1; }
    else return -1;
    const char *q = strchr(p, '?');
    size_t llen = q ? (size_t)(q - p) : strlen(p);
    char label[256];
    pct_decode(p, llen, label, sizeof label);
    char *colon = strchr(label, ':');
    if (colon) {
        *colon = 0;
        snprintf(t->issuer, sizeof t->issuer, "%.127s", label);
        const char *acc = colon + 1;
        while (*acc == ' ') acc++;
        snprintf(t->account, sizeof t->account, "%.127s", acc);
    } else snprintf(t->account, sizeof t->account, "%.127s", label);
    int have_secret = 0;
    while (q && *q) {
        q++;
        const char *amp = strchr(q, '&');
        size_t n = amp ? (size_t)(amp - q) : strlen(q);
        const char *eq = memchr(q, '=', n);
        if (eq) {
            size_t kl = (size_t)(eq - q);
            char val[512];
            pct_decode(eq + 1, n - kl - 1, val, sizeof val);
            if (kl == 6 && !strncasecmp(q, "secret", 6)) {
                long k = base32_decode(val, t->key, sizeof t->key);
                if (k <= 0) { explicit_bzero(val, sizeof val); return -1; }
                t->keylen = (size_t)k;
                have_secret = 1;
            } else if (kl == 6 && !strncasecmp(q, "issuer", 6)) snprintf(t->issuer, sizeof t->issuer, "%.127s", val);
            else if (kl == 9 && !strncasecmp(q, "algorithm", 9)) {
                if (!strcasecmp(val, "SHA256")) t->algo = TOTP_SHA256;
                else if (!strcasecmp(val, "SHA512")) t->algo = TOTP_SHA512;
                else if (strcasecmp(val, "SHA1")) return -1;
            } else if (kl == 6 && !strncasecmp(q, "digits", 6)) {
                int d = atoi(val);
                if (d < 6 || d > 10) return -1;
                t->digits = d;
            } else if (kl == 6 && !strncasecmp(q, "period", 6)) {
                int pr = atoi(val);
                if (pr < 1 || pr > 3600) return -1;
                t->period = pr;
            } else if (kl == 7 && !strncasecmp(q, "counter", 7)) t->counter = strtoull(val, NULL, 10);
            explicit_bzero(val, sizeof val);
        }
        q = amp;
    }
    explicit_bzero(label, sizeof label);
    return have_secret ? 0 : -1;
}

uint32_t hotp_code(const Totp *t, uint64_t counter) {
    uint8_t msg[8], mac[64];
    for (int i = 7; i >= 0; i--) { msg[i] = (uint8_t)counter; counter >>= 8; }
    size_t ml;
    if (t->algo == TOTP_SHA256) { hmac_sha256(t->key, t->keylen, msg, 8, mac); ml = 32; }
    else if (t->algo == TOTP_SHA512) { hmac_sha512(t->key, t->keylen, msg, 8, mac); ml = 64; }
    else { hmac_sha1(t->key, t->keylen, msg, 8, mac); ml = 20; }
    int off = mac[ml - 1] & 15;
    uint32_t bin = (uint32_t)(mac[off] & 0x7f) << 24 | (uint32_t)mac[off + 1] << 16 | (uint32_t)mac[off + 2] << 8 | mac[off + 3];
    explicit_bzero(mac, sizeof mac);
    uint64_t mod = 1;
    for (int i = 0; i < t->digits && i < 10; i++) mod *= 10;
    return (uint32_t)(bin % mod);
}

uint32_t totp_code(const Totp *t, uint64_t now, int *remaining) {
    int period = t->period > 0 ? t->period : 30;
    if (remaining) *remaining = period - (int)(now % (uint64_t)period);
    return hotp_code(t, now / (uint64_t)period);
}

void totp_format(const Totp *t, uint32_t code, char *out) {
    snprintf(out, 11, "%0*u", t->digits > 10 ? 10 : t->digits, code);
}

void totp_wipe(Totp *t) { explicit_bzero(t, sizeof *t); }
