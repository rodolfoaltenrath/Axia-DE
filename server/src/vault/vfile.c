/* vfile.c — .lvault format (see vfile.h and docs/PASSWORDS.md). */
#include "vfile.h"
#include "smem.h"
#include "vcrypto.h"
#include "monocypher.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/sysinfo.h>
#include <time.h>
#include <unistd.h>

static const char TRAILER[8] = { 'L', 'V', 'L', 'T', 'E', 'N', 'D', 0 };

const char *vf_strerror(int e) {
    switch (e) {
    case VF_OK: return "ok";
    case VF_EIO: return "io";
    case VF_EFORMAT: return "not-a-vault";
    case VF_EVERSION: return "unsupported-version";
    case VF_ETRUNC: return "truncated";
    case VF_EBADKEY: return "bad-key";
    case VF_ECORRUPT: return "corrupt";
    case VF_ENOMEM: return "memory";
    case VF_ENOSLOT: return "no-slot";
    }
    return "error";
}

/* ---- little endian ---- */
static void put16(uint8_t *p, uint16_t v) { p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); }
static void put32(uint8_t *p, uint32_t v) { for (int i = 0; i < 4; i++) p[i] = (uint8_t)(v >> (8 * i)); }
static void put64(uint8_t *p, uint64_t v) { for (int i = 0; i < 8; i++) p[i] = (uint8_t)(v >> (8 * i)); }
static uint16_t get16(const uint8_t *p) { return (uint16_t)(p[0] | p[1] << 8); }
static uint32_t get32(const uint8_t *p) { return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24; }
static uint64_t get64(const uint8_t *p) { uint64_t v = 0; for (int i = 7; i >= 0; i--) v = v << 8 | p[i]; return v; }

/* ---- derivations ---- */

static void blake_kdf(const uint8_t *key, size_t klen, const char *ctx, const uint8_t *salt, size_t slen, uint8_t out[32]) {
    crypto_blake2b_ctx c;
    crypto_blake2b_keyed_init(&c, 32, key, klen);
    crypto_blake2b_update(&c, (const uint8_t *)ctx, strlen(ctx));
    if (salt && slen) crypto_blake2b_update(&c, salt, slen);
    crypto_blake2b_final(&c, out);
}

int vf_argon2id(const uint8_t *pass, size_t plen, const uint8_t salt[16], const VKdf *p, uint8_t out[32]) {
    uint32_t blocks = p->m_kib;
    uint32_t lanes = p->lanes ? p->lanes : 1;
    if (blocks < 8 * lanes || blocks > (4u << 20) || p->t_cost < 1 || p->t_cost > 64 || lanes > 8) return -1;
    void *work = sm_alloc((size_t)blocks * 1024);
    if (!work) return -1;
    crypto_argon2_config cfg = { .algorithm = CRYPTO_ARGON2_ID, .nb_blocks = blocks, .nb_passes = p->t_cost, .nb_lanes = lanes };
    crypto_argon2_inputs in = { .pass = pass, .salt = salt, .pass_size = (uint32_t)plen, .salt_size = 16 };
    crypto_argon2(out, 32, work, cfg, in, crypto_argon2_no_extras);
    sm_free(work);                         /* wiped (the blocks derive from the password) */
    return 0;
}

void vf_keyfile_hash(const uint8_t *data, size_t len, uint8_t out[32]) {
    crypto_blake2b_ctx c;
    crypto_blake2b_keyed_init(&c, 32, (const uint8_t *)"lordos-vault:keyfile-hash", 25);
    crypto_blake2b_update(&c, data, len);
    crypto_blake2b_final(&c, out);
}

static const char CROCK[] = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

void vf_recovery_new(char out[48]) {
    uint8_t r[25];
    vc_random(r, sizeof r);
    int o = 0, bits = 0;
    uint32_t acc = 0;
    int chars = 0;
    for (int i = 0; i < 25; i++) {
        acc = acc << 8 | r[i];
        bits += 8;
        while (bits >= 5) {
            bits -= 5;
            if (chars && chars % 5 == 0) out[o++] = '-';
            out[o++] = CROCK[(acc >> bits) & 31];
            chars++;
        }
    }
    out[o] = 0;
    sm_wipe(r, sizeof r);
}

/* normalizes what was typed (lowercase, O→0, I/L→1, no dashes or spaces) to 25 bytes */
static int recovery_bytes(const char *s, uint8_t out[25]) {
    uint32_t acc = 0;
    int bits = 0, n = 0;
    for (; *s; s++) {
        int c = (unsigned char)*s;
        if (c == '-' || c == ' ' || c == '\t' || c == '\n') continue;
        if (c >= 'a' && c <= 'z') c -= 32;
        if (c == 'O') c = '0';
        if (c == 'I' || c == 'L') c = '1';
        const char *p = c ? strchr(CROCK, c) : NULL;
        if (!p) return -1;
        acc = acc << 5 | (uint32_t)(p - CROCK);
        bits += 5;
        if (bits >= 8) { bits -= 8; if (n >= 25) return -1; out[n++] = (uint8_t)(acc >> bits); }
    }
    return n == 25 ? 0 : -1;
}

/* KEK of a slot from the factors; 0 ok, 1 if factors are missing for this type, -1 error */
static int slot_kek(const VSlot *s, const VFactors *f, uint8_t kek[32]) {
    uint8_t a[32];
    switch (s->type) {
    case VS_PASSWORD:
    case VS_SESSION:
        if (!f->password) return 1;
        return vf_argon2id((const uint8_t *)f->password, strlen(f->password), s->salt, &s->p, kek) ? -1 : 0;
    case VS_PASSKEY:
        if (!f->password || !f->keyfile) return 1;
        if (vf_argon2id((const uint8_t *)f->password, strlen(f->password), s->salt, &s->p, a)) return -1;
        blake_kdf(a, 32, "lordos-vault:passkey", f->keyfile, 32, kek);
        sm_wipe(a, sizeof a);
        return 0;
    case VS_KEYFILE:
        if (!f->keyfile) return 1;
        blake_kdf(f->keyfile, 32, "lordos-vault:keyfile", s->salt, 16, kek);
        return 0;
    case VS_FIDO2:
        if (!f->fido) return 1;
        blake_kdf(f->fido, 32, "lordos-vault:fido2", s->salt, 16, kek);
        return 0;
    case VS_RECOVERY: {
        uint8_t r[25];
        if (!f->recovery || recovery_bytes(f->recovery, r)) return 1;
        blake_kdf(r, 25, "lordos-vault:recovery", s->salt, 16, kek);
        sm_wipe(r, sizeof r);
        return 0;
    }
    }
    return -1;
}

/* ---- slot serialization ---- */

static size_t slot_body(const VSlot *s, uint8_t *p, int with_wrap) {
    size_t o = 0;
    memcpy(p + o, s->id, 8); o += 8;
    put64(p + o, s->created); o += 8;
    size_t ll = strnlen(s->label, sizeof s->label - 1);
    p[o++] = (uint8_t)ll;
    memcpy(p + o, s->label, ll); o += ll;
    if (s->kdf == VK_ARGON2ID) { put32(p + o, s->p.m_kib); put32(p + o + 4, s->p.t_cost); put32(p + o + 8, s->p.lanes); o += 12; }
    memcpy(p + o, s->salt, 16); o += 16;
    if (s->type == VS_FIDO2) {
        p[o++] = s->cred_len;
        memcpy(p + o, s->cred, s->cred_len); o += s->cred_len;
        memcpy(p + o, s->hmac_salt, 32); o += 32;
        memcpy(p + o, s->aaguid, 16); o += 16;
        p[o++] = s->fido_pin;
    }
    if (with_wrap) { memcpy(p + o, s->nonce, 24); memcpy(p + o + 24, s->wrapped, 32); memcpy(p + o + 56, s->mac, 16); o += 72; }
    return o;
}

/* AD of the wrapper: magic + version + vault id + type/kdf + the slot without the wrapper */
static size_t wrap_ad(const VHeader *h, const VSlot *s, uint8_t *ad) {
    size_t o = 0;
    memcpy(ad, VF_MAGIC, 8); o += 8;
    put16(ad + o, VF_VERSION); o += 2;
    memcpy(ad + o, h->vault_id, 16); o += 16;
    ad[o++] = s->type;
    ad[o++] = s->kdf;
    o += slot_body(s, ad + o, 0);
    return o;
}

static size_t header_bytes(const VHeader *h, uint8_t *out) {
    size_t o = VF_HDR_FIXED;
    for (int i = 0; i < h->nslots; i++) {
        const VSlot *s = &h->slots[i];
        out[o] = s->type;
        out[o + 1] = s->kdf;
        size_t bl = slot_body(s, out + o + 4, 1);
        put16(out + o + 2, (uint16_t)bl);
        o += 4 + bl;
    }
    memcpy(out, VF_MAGIC, 8);
    put16(out + 8, VF_VERSION);
    put16(out + 10, 0);
    put32(out + 12, (uint32_t)o);
    memcpy(out + 16, h->vault_id, 16);
    put64(out + 32, h->created);
    put64(out + 40, h->saved);
    put32(out + 48, h->counter);
    put16(out + 52, h->flags);
    put16(out + 54, (uint16_t)h->nslots);
    return o;
}

#define HDR_MAX (VF_HDR_FIXED + VF_MAX_SLOTS * (4 + 8 + 8 + 1 + 64 + 12 + 16 + 1 + 255 + 32 + 16 + 1 + 72))

void vf_header_new(VHeader *h) {
    memset(h, 0, sizeof *h);
    vc_random(h->vault_id, 16);
    h->created = (uint64_t)time(NULL);
}

void vf_header_free(VHeader *h) { free(h->atts); h->atts = NULL; h->natts = 0; }

int vf_parse(const uint8_t *d, size_t len, VHeader *h) {
    memset(h, 0, sizeof *h);
    if (len < 8 || memcmp(d, VF_MAGIC, 8)) return VF_EFORMAT;
    if (len < VF_HDR_FIXED) return VF_ETRUNC;
    if (get16(d + 8) != VF_VERSION) return VF_EVERSION;
    size_t hlen = get32(d + 12);
    if (hlen < VF_HDR_FIXED || hlen > HDR_MAX) return VF_EFORMAT;
    if (hlen > len) return VF_ETRUNC;
    memcpy(h->vault_id, d + 16, 16);
    h->created = get64(d + 32);
    h->saved = get64(d + 40);
    h->counter = get32(d + 48);
    h->flags = get16(d + 52);
    int ns = get16(d + 54);
    if (ns > VF_MAX_SLOTS) return VF_EFORMAT;
    size_t o = VF_HDR_FIXED;
    for (int i = 0; i < ns; i++) {
        if (o + 4 > hlen) return VF_EFORMAT;
        VSlot *s = &h->slots[i];
        s->type = d[o];
        s->kdf = d[o + 1];
        size_t bl = get16(d + o + 2), b = o + 4, e = b + bl, q = b;
        if (e > hlen) return VF_EFORMAT;
        if (s->type < VS_PASSWORD || s->type > VS_SESSION || (s->kdf != VK_ARGON2ID && s->kdf != VK_BLAKE2B)) return VF_EFORMAT;
#define NEED(n) do { if (q + (n) > e) return VF_EFORMAT; } while (0)
        NEED(17);
        memcpy(s->id, d + q, 8); q += 8;
        s->created = get64(d + q); q += 8;
        size_t ll = d[q++];
        NEED(ll);
        if (ll >= sizeof s->label) return VF_EFORMAT;
        memcpy(s->label, d + q, ll); s->label[ll] = 0; q += ll;
        if (s->kdf == VK_ARGON2ID) {
            NEED(12);
            s->p.m_kib = get32(d + q); s->p.t_cost = get32(d + q + 4); s->p.lanes = get32(d + q + 8); q += 12;
        }
        NEED(16);
        memcpy(s->salt, d + q, 16); q += 16;
        if (s->type == VS_FIDO2) {
            NEED(1);
            s->cred_len = d[q++];
            NEED((size_t)s->cred_len + 49);
            memcpy(s->cred, d + q, s->cred_len); q += s->cred_len;
            memcpy(s->hmac_salt, d + q, 32); q += 32;
            memcpy(s->aaguid, d + q, 16); q += 16;
            s->fido_pin = d[q++];
        }
        NEED(72);
        memcpy(s->nonce, d + q, 24); memcpy(s->wrapped, d + q + 24, 32); memcpy(s->mac, d + q + 56, 16); q += 72;
        if (q != e) return VF_EFORMAT;
#undef NEED
        o = e;
    }
    if (o != hlen) return VF_EFORMAT;
    h->nslots = ns;
    h->hlen = hlen;
    /* body */
    if (o + 32 > len) return VF_ETRUNC;
    uint64_t bl = get64(d + o + 24);
    if (bl > VF_MAX_BODY) return VF_EFORMAT;
    if (o + 32 + bl + 16 > len) return VF_ETRUNC;
    h->body_off = o;
    h->body_len = bl;
    o += 32 + bl + 16;
    /* attachments */
    if (o + 4 > len) return VF_ETRUNC;
    uint32_t na = get32(d + o);
    o += 4;
    if (na > 100000) return VF_EFORMAT;
    if (na) {
        h->atts = calloc(na, sizeof *h->atts);
        if (!h->atts) return VF_ENOMEM;
    }
    for (uint32_t i = 0; i < na; i++) {
        if (o + 48 > len) { vf_header_free(h); return VF_ETRUNC; }
        VAttLoc *a = &h->atts[i];
        memcpy(a->id, d + o, 16);
        memcpy(a->nonce, d + o + 16, 24);
        a->len = get64(d + o + 40);
        if (a->len > VF_MAX_ATT) { vf_header_free(h); return VF_EFORMAT; }
        a->off = o + 48;
        if (a->off + a->len + 16 > len) { vf_header_free(h); return VF_ETRUNC; }
        memcpy(a->mac, d + a->off + a->len, 16);
        o = a->off + a->len + 16;
        h->natts++;
    }
    if (o + 16 > len) { vf_header_free(h); return VF_ETRUNC; }
    if (memcmp(d + o, TRAILER, 8) || get64(d + o + 8) != o + 16) { vf_header_free(h); return VF_EFORMAT; }
    if (o + 16 != len) { vf_header_free(h); return VF_EFORMAT; }
    return VF_OK;
}

int vf_open_slot(const VHeader *h, int type, const VFactors *f, uint8_t vk[32], int *slot) {
    uint8_t ad[HDR_MAX], kek[32];
    int tried = 0;
    for (int i = 0; i < h->nslots; i++) {
        const VSlot *s = &h->slots[i];
        if (type && s->type != type) continue;
        int r = slot_kek(s, f, kek);
        if (r == 1) continue;
        if (r < 0) return VF_ENOMEM;
        tried++;
        size_t adl = wrap_ad(h, s, ad);
        int ok = crypto_aead_unlock(vk, s->mac, kek, s->nonce, ad, adl, s->wrapped, 32) == 0;
        sm_wipe(kek, sizeof kek);
        if (ok) { if (slot) *slot = i; return VF_OK; }
    }
    return tried ? VF_EBADKEY : VF_ENOSLOT;
}

int vf_make_slot(const VHeader *h, VSlot *s, int type, const VFactors *f, const uint8_t vk[32], const VKdf *kdf, const char *label) {
    VSlot keep = *s;                         /* keeps FIDO2 cred/hmac_salt/aaguid if already present */
    memset(s, 0, sizeof *s);
    s->type = (uint8_t)type;
    s->kdf = (type == VS_PASSWORD || type == VS_PASSKEY || type == VS_SESSION) ? VK_ARGON2ID : VK_BLAKE2B;
    vc_random(s->id, 8);
    vc_random(s->salt, 16);
    s->created = (uint64_t)time(NULL);
    snprintf(s->label, sizeof s->label, "%s", label ? label : "");
    if (s->kdf == VK_ARGON2ID) {
        s->p = *kdf;
        if (!s->p.lanes) s->p.lanes = 1;
    }
    if (type == VS_FIDO2) {
        s->cred_len = keep.cred_len;
        memcpy(s->cred, keep.cred, keep.cred_len);
        memcpy(s->hmac_salt, keep.hmac_salt, 32);
        memcpy(s->aaguid, keep.aaguid, 16);
        s->fido_pin = keep.fido_pin;
    }
    uint8_t kek[32], ad[HDR_MAX];
    int r = slot_kek(s, f, kek);
    if (r) return r > 0 ? VF_ENOSLOT : VF_ENOMEM;
    vc_random(s->nonce, 24);
    size_t adl = wrap_ad(h, s, ad);
    crypto_aead_lock(s->wrapped, s->mac, kek, s->nonce, ad, adl, vk, 32);
    sm_wipe(kek, sizeof kek);
    return VF_OK;
}

static void subkey(const uint8_t vk[32], const char *ctx, uint8_t out[32]) { blake_kdf(vk, 32, ctx, NULL, 0, out); }

int vf_read_body(const uint8_t *d, size_t len, const VHeader *h, const uint8_t vk[32], uint8_t **out, size_t *outlen) {
    (void)len;
    const uint8_t *nonce = d + h->body_off, *ct = d + h->body_off + 32, *mac = ct + h->body_len;
    uint8_t *pt = sm_alloc(h->body_len + 1);
    if (!pt) return VF_ENOMEM;
    uint8_t bk[32];
    subkey(vk, "lordos-vault:body", bk);
    int r = crypto_aead_unlock(pt, mac, bk, nonce, d, h->hlen, ct, h->body_len);
    sm_wipe(bk, sizeof bk);
    if (r) { sm_free(pt); return VF_ECORRUPT; }
    pt[h->body_len] = 0;
    *out = pt;
    *outlen = h->body_len;
    return VF_OK;
}

static void att_ad(const uint8_t vault_id[16], const uint8_t id[16], uint8_t ad[40]) {
    memcpy(ad, "LVLTATT1", 8);
    memcpy(ad + 8, vault_id, 16);
    memcpy(ad + 24, id, 16);
}

int vf_att_seal(const uint8_t vk[32], const uint8_t vault_id[16], const uint8_t id[16], const uint8_t *plain, size_t len,
                uint8_t *cipher, uint8_t nonce[24], uint8_t mac[16]) {
    uint8_t ak[32], ad[40];
    subkey(vk, "lordos-vault:attachment", ak);
    att_ad(vault_id, id, ad);
    vc_random(nonce, 24);
    crypto_aead_lock(cipher, mac, ak, nonce, ad, 40, plain, len);
    sm_wipe(ak, sizeof ak);
    return VF_OK;
}

int vf_att_open(const uint8_t vk[32], const uint8_t vault_id[16], const uint8_t id[16], const uint8_t *cipher, size_t len,
                const uint8_t nonce[24], const uint8_t mac[16], uint8_t *plain) {
    uint8_t ak[32], ad[40];
    subkey(vk, "lordos-vault:attachment", ak);
    att_ad(vault_id, id, ad);
    int r = crypto_aead_unlock(plain, mac, ak, nonce, ad, 40, cipher, len);
    sm_wipe(ak, sizeof ak);
    return r ? VF_ECORRUPT : VF_OK;
}

/* ---- writing ---- */

static int write_all(int fd, const void *buf, size_t n) {
    const uint8_t *p = buf;
    while (n) {
        ssize_t w = write(fd, p, n);
        if (w < 0) { if (errno == EINTR) continue; return -1; }
        p += w; n -= (size_t)w;
    }
    return 0;
}

static int copy_range(int out, int in, uint64_t off, uint64_t len) {
    uint8_t buf[65536];
    while (len) {
        size_t k = len < sizeof buf ? (size_t)len : sizeof buf;
        ssize_t r = pread(in, buf, k, (off_t)off);
        if (r <= 0) { if (r < 0 && errno == EINTR) continue; return -1; }
        if (write_all(out, buf, (size_t)r)) return -1;
        off += (uint64_t)r; len -= (uint64_t)r;
    }
    return 0;
}

int vf_write_fd(int fd, VHeader *h, const uint8_t vk[32], const uint8_t *plain, size_t plen, const VAttOut *atts, int natts) {
    if (plen > VF_MAX_BODY) return VF_EFORMAT;
    h->saved = (uint64_t)time(NULL);
    h->counter++;
    uint8_t hdr[HDR_MAX];
    size_t hl = header_bytes(h, hdr);
    uint8_t *ct = malloc(plen ? plen : 1);
    if (!ct) return VF_ENOMEM;
    uint8_t nonce[24], mac[16], bk[32], lenb[8];
    vc_random(nonce, 24);
    subkey(vk, "lordos-vault:body", bk);
    crypto_aead_lock(ct, mac, bk, nonce, hdr, hl, plain, plen);
    sm_wipe(bk, sizeof bk);
    put64(lenb, plen);
    uint64_t total = hl + 32 + plen + 16;
    int err = write_all(fd, hdr, hl) || write_all(fd, nonce, 24) || write_all(fd, lenb, 8) || write_all(fd, ct, plen) || write_all(fd, mac, 16);
    free(ct);
    uint8_t cnt[4];
    put32(cnt, (uint32_t)natts);
    err = err || write_all(fd, cnt, 4);
    total += 4;
    for (int i = 0; !err && i < natts; i++) {
        const VAttOut *a = &atts[i];
        uint8_t ah[48];
        memcpy(ah, a->id, 16);
        memcpy(ah + 16, a->nonce, 24);
        put64(ah + 40, a->len);
        err = write_all(fd, ah, 48);
        if (!err) err = a->cipher ? write_all(fd, a->cipher, a->len) : copy_range(fd, a->srcfd, a->off, a->len);
        if (!err) err = write_all(fd, a->mac, 16);
        total += 48 + a->len + 16;
    }
    uint8_t tr[16];
    memcpy(tr, TRAILER, 8);
    put64(tr + 8, total + 16);
    err = err || write_all(fd, tr, 16);
    if (!err && fsync(fd) && errno != EINVAL) err = 1;
    return err ? VF_EIO : VF_OK;
}

int vf_save(int dirfd, const char *name, VHeader *h, const uint8_t vk[32], const uint8_t *plain, size_t plen,
            const VAttOut *atts, int natts) {
    char tmp[300], bak[300], baknew[300];
    if (strlen(name) > 200 || strchr(name, '/')) return VF_EIO;
    snprintf(tmp, sizeof tmp, ".%s.tmp-%d", name, (int)getpid());
    snprintf(bak, sizeof bak, "%s.bak", name);
    snprintf(baknew, sizeof baknew, ".%s.bak-%d", name, (int)getpid());
    unlinkat(dirfd, tmp, 0);
    int fd = openat(dirfd, tmp, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (fd < 0) return VF_EIO;
    VHeader saved = *h;
    int r = vf_write_fd(fd, h, vk, plain, plen, atts, natts);
    close(fd);
    if (r != VF_OK) { unlinkat(dirfd, tmp, 0); h->saved = saved.saved; h->counter = saved.counter; return r; }
    /* the previous version stays as .bak: hard link (the current file remains intact) and
     * atomic replacement of the old .bak; only then does the new one take the name */
    struct stat st;
    if (fstatat(dirfd, name, &st, 0) == 0) {
        unlinkat(dirfd, baknew, 0);
        if (linkat(dirfd, name, dirfd, baknew, 0) == 0) {
            if (renameat(dirfd, baknew, dirfd, bak)) unlinkat(dirfd, baknew, 0);
        }
    }
    if (renameat(dirfd, tmp, dirfd, name)) { unlinkat(dirfd, tmp, 0); return VF_EIO; }
    fsync(dirfd);
    return VF_OK;
}

/* ---- calibrate ---- */

static double now_ms(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1e3 + t.tv_nsec / 1e6; }

void vf_calibrate(int target_ms, VKdf *out, int *measured_ms) {
    if (target_ms < 100) target_ms = 100;
    uint8_t salt[16] = { 0 }, o[32];
    VKdf p = { 64 * 1024, 1, 1 };
    double t0 = now_ms();
    vf_argon2id((const uint8_t *)"calibrar", 8, salt, &p, o);
    double t1 = now_ms() - t0;                      /* one pass with 64 MiB */
    if (t1 < 1) t1 = 1;
    /* memory cap: one eighth of RAM, between 19 MiB and 256 MiB */
    struct sysinfo si;
    uint64_t cap = 256 * 1024;
    if (sysinfo(&si) == 0) {
        uint64_t ram_kib = (uint64_t)si.totalram * si.mem_unit / 1024;
        if (ram_kib / 8 < cap) cap = ram_kib / 8;
    }
    if (cap < 19 * 1024) cap = 19 * 1024;
    uint32_t m = 64 * 1024;
    double per = t1;                                /* ms per pass with m */
    while (m * 2 <= cap && per * 2 * 3 <= target_ms) { m *= 2; per *= 2; }
    while (m / 2 >= 19 * 1024 && per * 2 > target_ms) { m /= 2; per /= 2; }
    if (m > cap) { per = per * cap / m; m = (uint32_t)cap; }
    uint32_t t = (uint32_t)(target_ms / per + 0.5);
    if (t < 2) t = 2;
    if (t > 12) t = 12;
    out->m_kib = m;
    out->t_cost = t;
    out->lanes = 1;
    if (measured_ms) *measured_ms = (int)(per * t + 0.5);
    sm_wipe(o, sizeof o);
}
