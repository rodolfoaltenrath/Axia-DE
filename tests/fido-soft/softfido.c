/* softfido.c — autenticador FIDO2 por software sobre /dev/uhid, para probar el cliente CTAP2 de
 * Contraseñas (server/src/vault/ctap2.c) en una VM sin llave física. NO es una llave de verdad:
 * no protege nada (la clave maestra vive en la memoria de este proceso) y firma con una firma
 * falsa (nuestro cliente no verifica firmas). Necesita root (crear un dispositivo uhid).
 *
 *   sudo build/softfido [--pin 1234] [--touches 3] [--seed hex32]
 *
 * Crea un dispositivo HID «LordOS Soft FIDO2» (página 0xF1D0) que aparece como /dev/hidrawN.
 * Habla CTAPHID (INIT, PING, CBOR, CANCEL, KEEPALIVE, ERROR) y CTAP2: getInfo, clientPIN
 * (getRetries, getKeyAgreement, setPIN, getPinToken; protocolo 1), makeCredential y
 * getAssertion con hmac-secret (como CTAP 2.0: un solo CredRandom por credencial).
 * El toque se simula mandando KEEPALIVE «UPNEEDED» --touches veces cada 100 ms.
 * Comando del fabricante 0xC1 (pruebas): dato 0 = normal, 1 = nunca tocan (hasta CANCEL o 30 s),
 * 2 = el usuario rechaza (OPERATION_DENIED). */
#include "vault/cbor.h"
#include "vault/p256.h"
#include "vault/vcrypto.h"
#include "pkg/crypto.h"
#include "monocypher.h"

#include <errno.h>
#include <fcntl.h>
#include <linux/uhid.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static const uint8_t RDESC[] = {
    0x06, 0xd0, 0xf1, 0x09, 0x01, 0xa1, 0x01,
    0x09, 0x20, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x40, 0x81, 0x02,
    0x09, 0x21, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x40, 0x91, 0x02,
    0xc0,
};

static int ufd = -1;
static volatile sig_atomic_t quit;
static uint8_t master[32], aaguid[16] = { 0x4c, 0x6f, 0x72, 0x64, 0x4f, 0x53, 0x2d, 0x53, 0x6f, 0x66, 0x74, 0x46, 0x49, 0x44, 0x4f, 0x32 };
static uint8_t ka_priv[32], ka_x[32], ka_y[32], pin_token[32];
static char pin[64];
static int pin_set, retries = 8, touches = 3, mode;   /* mode: 0 normal, 1 sin toque, 2 rechazar */
static uint32_t next_cid = 1;
static unsigned counter;

static long long now_ms(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000; }

/* ---- uhid ---- */

static int uhid_write(const struct uhid_event *ev) {
    ssize_t r = write(ufd, ev, sizeof *ev);
    return r == (ssize_t)sizeof *ev ? 0 : -1;
}

static void send_pkt(const uint8_t p[64]) {
    struct uhid_event ev;
    memset(&ev, 0, sizeof ev);
    ev.type = UHID_INPUT2;
    ev.u.input2.size = 64;
    memcpy(ev.u.input2.data, p, 64);
    uhid_write(&ev);
}

static void put_cid(uint8_t *p, uint32_t c) { p[0] = (uint8_t)(c >> 24); p[1] = (uint8_t)(c >> 16); p[2] = (uint8_t)(c >> 8); p[3] = (uint8_t)c; }

static void hid_reply(uint32_t cid, uint8_t cmd, const uint8_t *d, size_t n) {
    uint8_t p[64];
    memset(p, 0, 64);
    put_cid(p, cid);
    p[4] = cmd; p[5] = (uint8_t)(n >> 8); p[6] = (uint8_t)n;
    size_t k = n < 57 ? n : 57, off = k;
    if (k) memcpy(p + 7, d, k);
    send_pkt(p);
    for (uint8_t seq = 0; off < n; seq++) {
        memset(p + 4, 0, 60);
        p[4] = seq;
        k = n - off < 59 ? n - off : 59;
        memcpy(p + 5, d + off, k);
        off += k;
        send_pkt(p);
    }
}

static void hid_error(uint32_t cid, uint8_t code) { hid_reply(cid, 0xbf, &code, 1); }
static void ctap_reply(uint32_t cid, uint8_t status, const uint8_t *cbor, size_t n) {
    uint8_t b[2048];
    b[0] = status;
    if (n) memcpy(b + 1, cbor, n);
    hid_reply(cid, 0x90, b, n + 1);
}
static void keepalive(uint32_t cid, uint8_t st) {
    uint8_t p[64];
    memset(p, 0, 64);
    put_cid(p, cid);
    p[4] = 0xbb; p[6] = 1; p[7] = st;
    send_pkt(p);
}

/* ---- CTAP2 ---- */

static void new_key_agreement(void) { p256_keypair(ka_priv, ka_x, ka_y); }

static void cose(CbW *w, const uint8_t x[32], const uint8_t y[32], int alg) {
    cbw_map(w, 5);
    cbw_int(w, 1); cbw_int(w, 2);
    cbw_int(w, 3); cbw_int(w, alg);
    cbw_int(w, -1); cbw_int(w, 1);
    cbw_int(w, -2); cbw_bytes(w, x, 32);
    cbw_int(w, -3); cbw_bytes(w, y, 32);
}

static void aes_iv0(const uint8_t key[32], const uint8_t *in, uint8_t *out, size_t n, int enc) {
    Aes a;
    uint8_t iv[16] = { 0 };
    aes_init(&a, key, 256);
    if (enc) aes_cbc_encrypt(&a, iv, in, out, n); else aes_cbc_decrypt(&a, iv, in, out, n);
    aes_wipe(&a);
}

/* sharedSecret a partir de la clave COSE de la plataforma; 0 bien */
static int shared_from(const Cb *c, int key, uint8_t shared[32]) {
    int x = cb_map_int(c, key, -2), y = cb_map_int(c, key, -3);
    if (cb_type(c, x) != CB_BYTES || c->it[x].len != 32 || cb_type(c, y) != CB_BYTES || c->it[y].len != 32) return -1;
    uint8_t z[32];
    if (p256_ecdh(ka_priv, c->it[x].ptr, c->it[y].ptr, z)) return -1;
    sha256(z, 32, shared);
    return 0;
}

static int check_pin_auth(const Cb *c, int auth, const uint8_t *cdh) {
    if (cb_type(c, auth) != CB_BYTES || c->it[auth].len != 16) return 0x33;
    uint8_t mac[32];
    hmac_sha256(pin_token, 32, cdh, 32, mac);
    return memcmp(mac, c->it[auth].ptr, 16) ? 0x33 : 0;
}

static void do_get_info(uint32_t cid) {
    uint8_t b[256];
    CbW w;
    cbw_init(&w, b, sizeof b);
    cbw_map(&w, 6);
    cbw_uint(&w, 1); cbw_array(&w, 1); cbw_text(&w, "FIDO_2_0");
    cbw_uint(&w, 2); cbw_array(&w, 1); cbw_text(&w, "hmac-secret");
    cbw_uint(&w, 3); cbw_bytes(&w, aaguid, 16);
    cbw_uint(&w, 4); cbw_map(&w, 3);
    cbw_text(&w, "rk"); cbw_bool(&w, 0);
    cbw_text(&w, "up"); cbw_bool(&w, 1);
    cbw_text(&w, "clientPin"); cbw_bool(&w, pin_set);
    cbw_uint(&w, 5); cbw_uint(&w, 1200);
    cbw_uint(&w, 6); cbw_array(&w, 1); cbw_uint(&w, 1);
    ctap_reply(cid, 0, b, w.n);
}

static void do_client_pin(uint32_t cid, const Cb *c) {
    int64_t sub = 0, proto = 0;
    if (cb_int(c, cb_map_int(c, 0, 2), &sub) || cb_int(c, cb_map_int(c, 0, 1), &proto) || proto != 1) { ctap_reply(cid, 0x11, NULL, 0); return; }
    uint8_t b[256];
    CbW w;
    cbw_init(&w, b, sizeof b);
    if (sub == 1) {                                   /* getRetries */
        cbw_map(&w, 1); cbw_uint(&w, 3); cbw_uint(&w, (uint64_t)retries);
        ctap_reply(cid, 0, b, w.n);
    } else if (sub == 2) {                            /* getKeyAgreement */
        cbw_map(&w, 1); cbw_uint(&w, 1); cose(&w, ka_x, ka_y, -25);
        ctap_reply(cid, 0, b, w.n);
    } else if (sub == 3) {                            /* setPIN */
        uint8_t shared[32], mac[32], np[64];
        int ka = cb_map_int(c, 0, 3), au = cb_map_int(c, 0, 4), ne = cb_map_int(c, 0, 5);
        if (pin_set) { ctap_reply(cid, 0x30, NULL, 0); return; }
        if (shared_from(c, ka, shared) || cb_type(c, ne) != CB_BYTES || c->it[ne].len != 64 || cb_type(c, au) != CB_BYTES || c->it[au].len != 16) { ctap_reply(cid, 0x14, NULL, 0); return; }
        hmac_sha256(shared, 32, c->it[ne].ptr, 64, mac);
        if (memcmp(mac, c->it[au].ptr, 16)) { ctap_reply(cid, 0x33, NULL, 0); return; }
        aes_iv0(shared, c->it[ne].ptr, np, 64, 0);
        size_t l = strnlen((char *)np, 64);
        if (l < 4 || l > 63) { ctap_reply(cid, 0x37, NULL, 0); return; }
        memcpy(pin, np, l); pin[l] = 0;
        pin_set = 1; retries = 8;
        fprintf(stderr, "softfido: PIN puesto\n");
        ctap_reply(cid, 0, NULL, 0);
    } else if (sub == 5) {                            /* getPINToken */
        uint8_t shared[32], h[32], dec[16];
        int ka = cb_map_int(c, 0, 3), he = cb_map_int(c, 0, 6);
        if (!pin_set) { ctap_reply(cid, 0x35, NULL, 0); return; }
        if (retries <= 0) { ctap_reply(cid, 0x32, NULL, 0); return; }
        if (shared_from(c, ka, shared) || cb_type(c, he) != CB_BYTES || c->it[he].len != 16) { ctap_reply(cid, 0x14, NULL, 0); return; }
        aes_iv0(shared, c->it[he].ptr, dec, 16, 0);
        sha256(pin, strlen(pin), h);
        if (memcmp(dec, h, 16)) {
            retries--;
            new_key_agreement();
            ctap_reply(cid, retries <= 0 ? 0x32 : 0x31, NULL, 0);
            return;
        }
        retries = 8;
        uint8_t enc[32];
        aes_iv0(shared, pin_token, enc, 32, 1);
        cbw_map(&w, 1); cbw_uint(&w, 2); cbw_bytes(&w, enc, 32);
        ctap_reply(cid, 0, b, w.n);
    } else ctap_reply(cid, 0x02, NULL, 0);
}

/* credencial = nonce(24) || XChaCha20-Poly1305(master, CredRandom(32), ad = rpIdHash) || mac(16) */
static void make_cred_id(const uint8_t rph[32], const uint8_t credrandom[32], uint8_t id[72]) {
    vc_random(id, 24);
    crypto_aead_lock(id + 24, id + 56, master, id, rph, 32, credrandom, 32);
}
static int open_cred_id(const uint8_t rph[32], const uint8_t *id, size_t n, uint8_t credrandom[32]) {
    if (n != 72) return -1;
    return crypto_aead_unlock(credrandom, id + 56, master, id, rph, 32, id + 24, 32);
}

/* lo que se hace después del toque */
static void finish_make(uint32_t cid, const Cb *c) {
    int rp = cb_map_int(c, 0, 2), id = cb_map_text(c, rp, "id");
    if (cb_type(c, id) != CB_TEXT) { ctap_reply(cid, 0x14, NULL, 0); return; }
    uint8_t rph[32], cr[32], credid[72], px[32], py[32], priv[32];
    sha256(c->it[id].ptr, c->it[id].len, rph);
    int ext = cb_map_int(c, 0, 6);
    int want_hmac = cb_type(c, cb_map_text(c, ext, "hmac-secret")) == CB_TRUE;
    vc_random(cr, 32);
    make_cred_id(rph, cr, credid);
    p256_keypair(priv, px, py);
    uint8_t ad[512];
    size_t n = 0;
    memcpy(ad, rph, 32); n = 32;
    ad[n++] = (uint8_t)(0x01 | 0x40 | (want_hmac ? 0x80 : 0) | (cb_map_int(c, 0, 8) >= 0 ? 0x04 : 0));
    counter++;
    ad[n++] = (uint8_t)(counter >> 24); ad[n++] = (uint8_t)(counter >> 16); ad[n++] = (uint8_t)(counter >> 8); ad[n++] = (uint8_t)counter;
    memcpy(ad + n, aaguid, 16); n += 16;
    ad[n++] = 0; ad[n++] = 72;
    memcpy(ad + n, credid, 72); n += 72;
    CbW w;
    cbw_init(&w, ad + n, sizeof ad - n);
    cose(&w, px, py, -7);
    if (want_hmac) { cbw_map(&w, 1); cbw_text(&w, "hmac-secret"); cbw_bool(&w, 1); }
    n += w.n;
    uint8_t b[1024];
    cbw_init(&w, b, sizeof b);
    cbw_map(&w, 3);
    cbw_uint(&w, 1); cbw_text(&w, "none");
    cbw_uint(&w, 2); cbw_bytes(&w, ad, n);
    cbw_uint(&w, 3); cbw_map(&w, 0);
    ctap_reply(cid, 0, b, w.n);
}

static void finish_assert(uint32_t cid, const Cb *c, const uint8_t rph[32], const uint8_t cr[32], const uint8_t *credid, size_t cl, int uv) {
    int ext = cb_map_int(c, 0, 4), hs = cb_map_text(c, ext, "hmac-secret");
    uint8_t out[64];
    size_t outn = 0;
    if (hs >= 0) {
        uint8_t shared[32], mac[32], salt[64];
        int ka = cb_map_int(c, hs, 1), se = cb_map_int(c, hs, 2), sa = cb_map_int(c, hs, 3);
        if (shared_from(c, ka, shared) || cb_type(c, se) != CB_BYTES || (c->it[se].len != 32 && c->it[se].len != 64) ||
            cb_type(c, sa) != CB_BYTES || c->it[sa].len != 16) { ctap_reply(cid, 0x14, NULL, 0); return; }
        hmac_sha256(shared, 32, c->it[se].ptr, c->it[se].len, mac);
        if (memcmp(mac, c->it[sa].ptr, 16)) { ctap_reply(cid, 0x14, NULL, 0); return; }   /* saltAuth mal */
        size_t sl = c->it[se].len;
        aes_iv0(shared, c->it[se].ptr, salt, sl, 0);
        uint8_t plain[64];
        hmac_sha256(cr, 32, salt, 32, plain);
        if (sl == 64) hmac_sha256(cr, 32, salt + 32, 32, plain + 32);
        aes_iv0(shared, plain, out, sl, 1);
        outn = sl;
    }
    uint8_t ad[256];
    size_t n = 0;
    memcpy(ad, rph, 32); n = 32;
    ad[n++] = (uint8_t)(0x01 | (uv ? 0x04 : 0) | (outn ? 0x80 : 0));
    counter++;
    ad[n++] = (uint8_t)(counter >> 24); ad[n++] = (uint8_t)(counter >> 16); ad[n++] = (uint8_t)(counter >> 8); ad[n++] = (uint8_t)counter;
    CbW w;
    if (outn) {
        cbw_init(&w, ad + n, sizeof ad - n);
        cbw_map(&w, 1); cbw_text(&w, "hmac-secret"); cbw_bytes(&w, out, outn);
        n += w.n;
    }
    uint8_t fake_sig[8] = { 0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01 };   /* firma falsa (DER de r=s=1) */
    uint8_t b[1024];
    cbw_init(&w, b, sizeof b);
    cbw_map(&w, 3);
    cbw_uint(&w, 1); cbw_map(&w, 2); cbw_text(&w, "id"); cbw_bytes(&w, credid, cl); cbw_text(&w, "type"); cbw_text(&w, "public-key");
    cbw_uint(&w, 2); cbw_bytes(&w, ad, n);
    cbw_uint(&w, 3); cbw_bytes(&w, fake_sig, sizeof fake_sig);
    ctap_reply(cid, 0, b, w.n);
}

/* Espera el «toque»: KEEPALIVE cada 100 ms. Devuelve 0 si tocaron, o el estado de error
 * (CANCEL → 0x2D, rechazo → 0x27, 30 s → 0x2F). Mientras tanto atiende los paquetes que llegan. */
static int read_event(int timeout, uint8_t pkt[64]);
static int wait_touch(uint32_t cid) {
    long long start = now_ms();
    int sent = 0;
    for (;;) {
        if (mode != 1 && sent >= touches) return mode == 2 ? 0x27 : 0;
        if (now_ms() - start > 30000) return 0x2f;
        keepalive(cid, 2);
        sent++;
        long long until = now_ms() + 100;
        while (now_ms() < until) {
            uint8_t p[64];
            int r = read_event((int)(until - now_ms()), p);
            if (r < 0) return 0x2d;
            if (r == 1) {
                uint32_t pc = (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3];
                if (pc == cid && p[4] == 0x91) return 0x2d;            /* CANCEL */
                if (pc != cid && (p[4] & 0x80)) hid_error(pc, 0x06);   /* otro canal: ocupado */
            }
        }
    }
}

static void do_make(uint32_t cid, const Cb *c) {
    int cdh = cb_map_int(c, 0, 1), params = cb_map_int(c, 0, 4), auth = cb_map_int(c, 0, 8);
    if (cb_type(c, cdh) != CB_BYTES || c->it[cdh].len != 32) { ctap_reply(cid, 0x14, NULL, 0); return; }
    int es256 = 0;
    for (int k = 0; cb_at(c, params, k) >= 0; k++) {
        int64_t alg;
        if (cb_int(c, cb_map_text(c, cb_at(c, params, k), "alg"), &alg) == 0 && alg == -7) es256 = 1;
    }
    if (!es256) { ctap_reply(cid, 0x26, NULL, 0); return; }
    if (pin_set && auth < 0) { ctap_reply(cid, 0x36, NULL, 0); return; }
    if (auth >= 0) { int e = check_pin_auth(c, auth, c->it[cdh].ptr); if (e) { ctap_reply(cid, (uint8_t)e, NULL, 0); return; } }
    int st = wait_touch(cid);
    if (st) { ctap_reply(cid, (uint8_t)st, NULL, 0); return; }
    finish_make(cid, c);
}

static void do_assert(uint32_t cid, const Cb *c) {
    int rp = cb_map_int(c, 0, 1), cdh = cb_map_int(c, 0, 2), al = cb_map_int(c, 0, 3), auth = cb_map_int(c, 0, 6);
    if (cb_type(c, rp) != CB_TEXT || cb_type(c, cdh) != CB_BYTES || c->it[cdh].len != 32) { ctap_reply(cid, 0x14, NULL, 0); return; }
    uint8_t rph[32], cr[32];
    sha256(c->it[rp].ptr, c->it[rp].len, rph);
    int found = -1;
    for (int k = 0; cb_at(c, al, k) >= 0 && found < 0; k++) {
        int id = cb_map_text(c, cb_at(c, al, k), "id");
        if (cb_type(c, id) == CB_BYTES && open_cred_id(rph, c->it[id].ptr, c->it[id].len, cr) == 0) found = id;
    }
    if (found < 0) { ctap_reply(cid, 0x2e, NULL, 0); return; }
    int uv = 0;
    if (auth >= 0) { int e = check_pin_auth(c, auth, c->it[cdh].ptr); if (e) { ctap_reply(cid, (uint8_t)e, NULL, 0); return; } uv = 1; }
    int st = wait_touch(cid);
    if (st) { ctap_reply(cid, (uint8_t)st, NULL, 0); return; }
    finish_assert(cid, c, rph, cr, c->it[found].ptr, c->it[found].len, uv);
}

static void handle_msg(uint32_t cid, uint8_t cmd, const uint8_t *d, size_t n) {
    if (cmd == 0x86) {                                    /* INIT */
        if (n != 8) { hid_error(cid, 0x03); return; }
        uint8_t r[17];
        memcpy(r, d, 8);
        uint32_t nc = cid == 0xffffffffu ? next_cid++ : cid;
        put_cid(r + 8, nc);
        r[12] = 2; r[13] = 1; r[14] = 0; r[15] = 0; r[16] = 0x04 | 0x08;   /* CBOR, sin MSG */
        hid_reply(cid, 0x86, r, 17);
        return;
    }
    if (cid == 0xffffffffu || cid == 0) { hid_error(cid, 0x0b); return; }
    if (cmd == 0x81) { hid_reply(cid, 0x81, d, n); return; }          /* PING */
    if (cmd == 0x91) return;                                          /* CANCEL sin nada pendiente */
    if (cmd == 0xc1) { if (n >= 1) mode = d[0]; hid_reply(cid, 0xc1, NULL, 0); fprintf(stderr, "softfido: modo %d\n", mode); return; }
    if (cmd != 0x90 || n < 1) { hid_error(cid, 0x01); return; }
    CbItem it[256];
    Cb c;
    if (n > 1 && cb_parse(&c, d + 1, n - 1, it, 256, 0)) { ctap_reply(cid, 0x12, NULL, 0); return; }   /* INVALID_CBOR */
    if (n == 1) { c.it = it; c.n = 0; }
    switch (d[0]) {
    case 0x04: do_get_info(cid); break;
    case 0x06: do_client_pin(cid, &c); break;
    case 0x01: do_make(cid, &c); break;
    case 0x02: do_assert(cid, &c); break;
    default: ctap_reply(cid, 0x01, NULL, 0); break;
    }
}

/* ---- armado de mensajes ---- */

static uint8_t asm_buf[7609];
static size_t asm_total, asm_got;
static uint32_t asm_cid;
static uint8_t asm_cmd;
static int asm_seq = -1;

/* Lee un evento de uhid: 1 = paquete de salida en pkt, 0 = otro evento o nada, -1 = cerrar */
static int read_event(int timeout, uint8_t pkt[64]) {
    struct pollfd p = { ufd, POLLIN, 0 };
    if (poll(&p, 1, timeout < 0 ? 0 : timeout) <= 0) return 0;
    struct uhid_event ev;
    ssize_t r = read(ufd, &ev, sizeof ev);
    if (r <= 0) return quit ? -1 : 0;
    if (ev.type == UHID_OUTPUT) {
        const uint8_t *d = ev.u.output.data;
        size_t n = ev.u.output.size;
        if (n == 65 && d[0] == 0) { d++; n--; }
        if (n != 64) return 0;
        memcpy(pkt, d, 64);
        return 1;
    }
    if (ev.type == UHID_GET_REPORT) {
        struct uhid_event rep;
        memset(&rep, 0, sizeof rep);
        rep.type = UHID_GET_REPORT_REPLY;
        rep.u.get_report_reply.id = ev.u.get_report.id;
        rep.u.get_report_reply.err = EIO;
        uhid_write(&rep);
    } else if (ev.type == UHID_SET_REPORT) {
        struct uhid_event rep;
        memset(&rep, 0, sizeof rep);
        rep.type = UHID_SET_REPORT_REPLY;
        rep.u.set_report_reply.id = ev.u.set_report.id;
        rep.u.set_report_reply.err = EIO;
        uhid_write(&rep);
    }
    return 0;
}

static void on_packet(const uint8_t p[64]) {
    uint32_t cid = (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3];
    if (p[4] & 0x80) {
        size_t total = (size_t)p[5] << 8 | p[6];
        if (total > sizeof asm_buf) { hid_error(cid, 0x03); asm_seq = -1; return; }
        asm_cid = cid; asm_cmd = p[4]; asm_total = total;
        asm_got = total < 57 ? total : 57;
        memcpy(asm_buf, p + 7, asm_got);
        asm_seq = 0;
    } else {
        if (asm_seq < 0 || cid != asm_cid) return;
        if (p[4] != asm_seq) { hid_error(cid, 0x04); asm_seq = -1; return; }
        size_t k = asm_total - asm_got < 59 ? asm_total - asm_got : 59;
        memcpy(asm_buf + asm_got, p + 5, k);
        asm_got += k;
        asm_seq++;
    }
    if (asm_seq >= 0 && asm_got >= asm_total) {
        asm_seq = -1;
        handle_msg(asm_cid, asm_cmd, asm_buf, asm_total);
    }
}

static void on_signal(int s) { (void)s; quit = 1; }

int main(int argc, char **argv) {
    vc_random(master, 32);
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--pin") && i + 1 < argc) { snprintf(pin, sizeof pin, "%s", argv[++i]); pin_set = 1; }
        else if (!strcmp(argv[i], "--touches") && i + 1 < argc) touches = atoi(argv[++i]);
        else if (!strcmp(argv[i], "--seed") && i + 1 < argc) {
            const char *h = argv[++i];
            for (int k = 0; k < 32 && h[2 * k] && h[2 * k + 1]; k++) { unsigned v; sscanf(h + 2 * k, "%2x", &v); master[k] = (uint8_t)v; }
        } else { fprintf(stderr, "uso: softfido [--pin PIN] [--touches N] [--seed hex32]\n"); return 2; }
    }
    vc_random(pin_token, 32);
    new_key_agreement();
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    ufd = open("/dev/uhid", O_RDWR | O_CLOEXEC);
    if (ufd < 0) { perror("softfido: /dev/uhid"); return 1; }
    struct uhid_event ev;
    memset(&ev, 0, sizeof ev);
    ev.type = UHID_CREATE2;
    snprintf((char *)ev.u.create2.name, sizeof ev.u.create2.name, "LordOS Soft FIDO2");
    snprintf((char *)ev.u.create2.phys, sizeof ev.u.create2.phys, "softfido");
    ev.u.create2.rd_size = sizeof RDESC;
    memcpy(ev.u.create2.rd_data, RDESC, sizeof RDESC);
    ev.u.create2.bus = 0x03;          /* BUS_USB */
    ev.u.create2.vendor = 0x1209;     /* pid.codes (pruebas) */
    ev.u.create2.product = 0x5f1d;
    ev.u.create2.version = 1;
    if (uhid_write(&ev)) { perror("softfido: UHID_CREATE2"); return 1; }
    fprintf(stderr, "softfido: listo%s\n", pin_set ? " (con PIN)" : "");
    while (!quit) {
        uint8_t p[64];
        int r = read_event(500, p);
        if (r < 0) break;
        if (r == 1) on_packet(p);
    }
    memset(&ev, 0, sizeof ev);
    ev.type = UHID_DESTROY;
    uhid_write(&ev);
    close(ufd);
    return 0;
}
