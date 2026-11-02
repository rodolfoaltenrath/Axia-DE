/* secretsvc.c — the freedesktop Secret Service API over our own D-Bus code (see secretsvc.h and
 * docs/PASSWORDS.md, "Secret Service").
 *
 * A single collection, the session keyring: /org/freedesktop/secrets/collection/login, with the
 * alias "default" (/org/freedesktop/secrets/aliases/default is accepted too). CreateCollection
 * returns that same collection (no others are created: so any app that asks for "a new
 * collection" ends up in the session keyring). Deleting the collection: NotSupported.
 *
 * Sessions: "plain" and "dh-ietf1024-sha256-aes128-cbc-pkcs7" (the 1024-bit MODP group of RFC 2409,
 * HKDF-SHA256 without salt or info → AES-128 key, AES-128-CBC with PKCS#7 and the IV in the
 * Secret's parameters). The 1024-bit arithmetic is our own (Montgomery with 32-bit words).
 *
 * Access control: as in gnome-keyring, every app of the session that reaches the bus can use the
 * collection while it is unlocked; the backend receives the caller for the log and the
 * notices. Isolated .lpk apps do not reach the bus (they cannot open sockets). */
#include "secretsvc.h"
#include "dbus.h"
#include "smem.h"
#include "vcrypto.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define BASE      "/org/freedesktop/secrets"
#define COLL      BASE "/collection/login"
#define I_SVC     "org.freedesktop.Secret.Service"
#define I_COLL    "org.freedesktop.Secret.Collection"
#define I_ITEM    "org.freedesktop.Secret.Item"
#define I_SESS    "org.freedesktop.Secret.Session"
#define I_PROMPT  "org.freedesktop.Secret.Prompt"
#define I_PROPS   "org.freedesktop.DBus.Properties"
#define I_INTRO   "org.freedesktop.DBus.Introspectable"
#define I_PEER    "org.freedesktop.DBus.Peer"
#define E_LOCKED  "org.freedesktop.Secret.Error.IsLocked"
#define E_NOSESS  "org.freedesktop.Secret.Error.NoSession"
#define E_NOOBJ   "org.freedesktop.Secret.Error.NoSuchObject"
#define E_METHOD  "org.freedesktop.DBus.Error.UnknownMethod"
#define E_ARGS    "org.freedesktop.DBus.Error.InvalidArgs"
#define E_NOTSUP  "org.freedesktop.DBus.Error.NotSupported"
#define E_FAILED  "org.freedesktop.DBus.Error.Failed"
#define E_UNKOBJ  "org.freedesktop.DBus.Error.UnknownObject"
#define E_UNKPROP "org.freedesktop.DBus.Error.UnknownProperty"
#define E_RO      "org.freedesktop.DBus.Error.PropertyReadOnly"
#define ALG_DH    "dh-ietf1024-sha256-aes128-cbc-pkcs7"
#define MAX_ATTRS 64
#define MAX_SECRET (1u << 20)

static DConn C = { .fd = -1 };
static int running, debug;
static const SsBackend *B;
static void *U;
static uint64_t created_at;

#define LOG(...) do { if (debug) fprintf(stderr, "secretsvc: " __VA_ARGS__); } while (0)

/* ================= 1024-bit arithmetic (DH) ================= */

#define BN 32
typedef uint32_t Bn[BN];

static void bn_from_be(Bn r, const uint8_t *b, size_t n) {
    memset(r, 0, sizeof(Bn));
    for (size_t i = 0; i < n && i < 128; i++) {
        size_t bit = (n - 1 - i);           /* least significant byte = the last one */
        r[bit / 4] |= (uint32_t)b[i] << (8 * (bit % 4));
    }
}
static void bn_to_be(const Bn a, uint8_t out[128]) {
    for (int i = 0; i < 128; i++) { int bit = 127 - i; out[i] = (uint8_t)(a[bit / 4] >> (8 * (bit % 4))); }
}
static int bn_cmp(const Bn a, const Bn b) {
    for (int i = BN - 1; i >= 0; i--) if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
    return 0;
}
static uint32_t bn_sub(Bn r, const Bn a, const Bn b) {
    uint64_t br = 0;
    for (int i = 0; i < BN; i++) { uint64_t d = (uint64_t)a[i] - b[i] - br; r[i] = (uint32_t)d; br = (d >> 63) & 1; }
    return (uint32_t)br;
}
static int bn_zero(const Bn a) { uint32_t x = 0; for (int i = 0; i < BN; i++) x |= a[i]; return x == 0; }

/* r = a*b*R^-1 mod m (CIOS); a, b < m */
static void mont_mul(Bn r, const Bn a, const Bn b, const Bn m, uint32_t minv) {
    uint32_t t[BN + 2] = { 0 };
    for (int i = 0; i < BN; i++) {
        uint64_t c = 0, s;
        for (int j = 0; j < BN; j++) { s = (uint64_t)t[j] + (uint64_t)a[j] * b[i] + c; t[j] = (uint32_t)s; c = s >> 32; }
        s = (uint64_t)t[BN] + c; t[BN] = (uint32_t)s; t[BN + 1] = (uint32_t)(s >> 32);
        uint32_t q = t[0] * minv;
        s = (uint64_t)t[0] + (uint64_t)q * m[0]; c = s >> 32;
        for (int j = 1; j < BN; j++) { s = (uint64_t)t[j] + (uint64_t)q * m[j] + c; t[j - 1] = (uint32_t)s; c = s >> 32; }
        s = (uint64_t)t[BN] + c; t[BN - 1] = (uint32_t)s; c = s >> 32;
        t[BN] = t[BN + 1] + (uint32_t)c;
    }
    Bn d;
    uint32_t br = bn_sub(d, t, m);
    /* t >= m if there was a carry out or the subtraction did not borrow */
    uint32_t use_d = (uint32_t)(t[BN] != 0) | (br ^ 1);
    uint32_t mask = (uint32_t)0 - use_d;
    for (int i = 0; i < BN; i++) r[i] = (d[i] & mask) | (t[i] & ~mask);
    explicit_bzero(t, sizeof t);
    explicit_bzero(d, sizeof d);
}

int ss_dh_modexp(const uint8_t base[128], const uint8_t exp[128], const uint8_t mod[128], uint8_t out[128]) {
    Bn m, a, x, y, r2, one;
    bn_from_be(m, mod, 128);
    bn_from_be(a, base, 128);
    if (!(m[0] & 1) || bn_cmp(a, m) >= 0) return -1;
    /* -m^-1 mod 2^32 (Newton) */
    uint32_t inv = 1;
    for (int i = 0; i < 5; i++) inv *= 2 - m[0] * inv;
    uint32_t minv = (uint32_t)0 - inv;
    /* R^2 mod m with 2048 doublings */
    memset(r2, 0, sizeof r2);
    r2[0] = 1;
    for (int i = 0; i < 2 * 32 * BN; i++) {
        uint32_t carry = r2[BN - 1] >> 31;
        for (int k = BN - 1; k > 0; k--) r2[k] = r2[k] << 1 | r2[k - 1] >> 31;
        r2[0] <<= 1;
        Bn d;
        uint32_t br = bn_sub(d, r2, m);
        if (carry || !br) memcpy(r2, d, sizeof d);
    }
    memset(one, 0, sizeof one);
    one[0] = 1;
    mont_mul(x, one, r2, m, minv);           /* 1 in Montgomery form */
    mont_mul(a, a, r2, m, minv);
    for (int i = 0; i < 1024; i++) {
        int byte = i / 8, bit = 7 - i % 8;
        uint32_t e = (uint32_t)(exp[byte] >> bit) & 1, mask = (uint32_t)0 - e;
        mont_mul(x, x, x, m, minv);
        mont_mul(y, x, a, m, minv);
        for (int k = 0; k < BN; k++) x[k] = (y[k] & mask) | (x[k] & ~mask);
    }
    mont_mul(x, x, one, m, minv);
    bn_to_be(x, out);
    explicit_bzero(a, sizeof a); explicit_bzero(x, sizeof x); explicit_bzero(y, sizeof y);
    return 0;
}

static const uint8_t P1024[128] = {
    0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xC9,0x0F,0xDA,0xA2,0x21,0x68,0xC2,0x34,0xC4,0xC6,0x62,0x8B,0x80,0xDC,0x1C,0xD1,
    0x29,0x02,0x4E,0x08,0x8A,0x67,0xCC,0x74,0x02,0x0B,0xBE,0xA6,0x3B,0x13,0x9B,0x22,0x51,0x4A,0x08,0x79,0x8E,0x34,0x04,0xDD,
    0xEF,0x95,0x19,0xB3,0xCD,0x3A,0x43,0x1B,0x30,0x2B,0x0A,0x6D,0xF2,0x5F,0x14,0x37,0x4F,0xE1,0x35,0x6D,0x6D,0x51,0xC2,0x45,
    0xE4,0x85,0xB5,0x76,0x62,0x5E,0x7E,0xC6,0xF4,0x4C,0x42,0xE9,0xA6,0x37,0xED,0x6B,0x0B,0xFF,0x5C,0xB6,0xF4,0x06,0xB7,0xED,
    0xEE,0x38,0x6B,0xFB,0x5A,0x89,0x9F,0xA5,0xAE,0x9F,0x24,0x11,0x7C,0x4B,0x1F,0xE6,0x49,0x28,0x66,0x51,0xEC,0xE6,0x53,0x81,
    0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,0xFF,
};
const uint8_t *ss_dh_prime(void) { return P1024; }

/* 1 < y < p-1 */
static int dh_pub_ok(const uint8_t y[128]) {
    Bn a, p, pm1, one;
    bn_from_be(a, y, 128);
    bn_from_be(p, P1024, 128);
    memset(one, 0, sizeof one);
    one[0] = 1;
    bn_sub(pm1, p, one);
    return !bn_zero(a) && bn_cmp(a, one) != 0 && bn_cmp(a, pm1) < 0;
}

/* ================= sessions and notices ================= */

#define NSESS 64
typedef struct { unsigned id; int used, dh; uint8_t key[16]; char owner[256]; } Sess;
static Sess sess[NSESS];
static unsigned next_sess = 1;

#define NPROMPT 16
#define PROMPT_OBJS 32
typedef struct { unsigned id; int used, prompted, done, unlocked, nobj; char obj[PROMPT_OBJS][128]; } Prompt;
static Prompt prompts[NPROMPT];
static unsigned next_prompt = 1;

static struct { char name[256]; uint32_t pid, serial; } pidc[32];
static int pidc_next;

static void sess_free(Sess *s) { explicit_bzero(s, sizeof *s); }

static Sess *sess_new(const char *owner) {
    Sess *s = NULL;
    for (int i = 0; i < NSESS; i++) if (!sess[i].used) { s = &sess[i]; break; }
    if (!s) {                                     /* full: the oldest one is dropped */
        s = &sess[0];
        for (int i = 1; i < NSESS; i++) if (sess[i].id < s->id) s = &sess[i];
        sess_free(s);
    }
    s->used = 1;
    s->id = next_sess++;
    snprintf(s->owner, sizeof s->owner, "%s", owner ? owner : "");
    return s;
}

static Sess *sess_find(const char *path, const char *sender) {
    unsigned n;
    char tail;
    if (!path || sscanf(path, BASE "/session/%u%c", &n, &tail) != 1) return NULL;
    for (int i = 0; i < NSESS; i++)
        if (sess[i].used && sess[i].id == n && !strcmp(sess[i].owner, sender ? sender : "")) return &sess[i];
    return NULL;
}

static Prompt *prompt_new(void) {
    Prompt *p = NULL;
    for (int i = 0; i < NPROMPT; i++) if (!prompts[i].used) { p = &prompts[i]; break; }
    if (!p) { p = &prompts[0]; for (int i = 1; i < NPROMPT; i++) if (prompts[i].id < p->id) p = &prompts[i]; }
    memset(p, 0, sizeof *p);
    p->used = 1;
    p->id = next_prompt++;
    return p;
}
static Prompt *prompt_by_id(unsigned id) { for (int i = 0; i < NPROMPT; i++) if (prompts[i].used && prompts[i].id == id) return &prompts[i]; return NULL; }

/* ================= paths ================= */

enum { O_NONE, O_NODE, O_SERVICE, O_COLL, O_ITEM, O_SESS, O_PROMPT };

static int id_ok(const char *s, size_t n) {
    if (!n || n >= 40) return 0;
    for (size_t i = 0; i < n; i++) {
        char c = s[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_')) return 0;
    }
    return 1;
}

static int classify(const char *p, char id[40], unsigned *num) {
    static const char *const nodes[] = { "/", "/org", "/org/freedesktop", BASE "/collection", BASE "/aliases", BASE "/session", BASE "/prompt", NULL };
    if (!p) return O_NONE;
    for (int i = 0; nodes[i]; i++) if (!strcmp(p, nodes[i])) return O_NODE;
    if (!strcmp(p, BASE)) return O_SERVICE;
    static const char *const colls[] = { COLL, BASE "/collection/default", BASE "/aliases/default", NULL };
    for (int i = 0; colls[i]; i++) {
        size_t n = strlen(colls[i]);
        if (!strncmp(p, colls[i], n)) {
            if (!p[n]) return O_COLL;
            if (p[n] == '/' && id_ok(p + n + 1, strlen(p + n + 1))) { snprintf(id, 40, "%s", p + n + 1); return O_ITEM; }
            return O_NONE;
        }
    }
    unsigned n;
    char tail;
    if (sscanf(p, BASE "/session/%u%c", &n, &tail) == 1) { *num = n; return O_SESS; }
    if (sscanf(p, BASE "/prompt/%u%c", &n, &tail) == 1) { *num = n; return O_PROMPT; }
    return O_NONE;
}

static void item_path(const char *id, char out[128]) { snprintf(out, 128, COLL "/%s", id); }
static void prompt_path(unsigned id, char out[64]) { snprintf(out, 64, BASE "/prompt/%u", id); }

static int locked(void) { return B->locked ? B->locked(U) != 0 : 0; }

/* ids of the items that match attrs (malloc; release with free) */
static int search_ids(const SsAttr *attrs, int n, char (**out)[40]) {
    *out = NULL;
    for (int max = 256; max <= 65536; max *= 4) {
        char (*ids)[40] = calloc((size_t)max, 40);
        if (!ids) return -1;
        int k = B->search(U, attrs, n, ids, max);
        if (k < 0) { free(ids); return -1; }
        if (k < max) { *out = ids; return k; }
        free(ids);
    }
    return -1;
}

/* a{ss} → array of SsAttr pointing into the message */
static int read_attrs(DRd *r, SsAttr *a, int max) {
    int n = 0;
    size_t end = dr_arr(r, 8);
    while (dr_more(r, end)) {
        dr_struct(r);
        const char *k = dr_str(r, NULL), *v = dr_str(r, NULL);
        if (n >= max) return -1;
        a[n].name = k;
        a[n].value = v;
        n++;
    }
    return r->err ? -1 : n;
}

static void write_attrs(DBuf *w, const SsAttr *a, int n) {
    size_t mk = dw_arr_begin(w, 8);
    for (int i = 0; i < n; i++) { dw_struct(w); dw_str(w, a[i].name ? a[i].name : ""); dw_str(w, a[i].value ? a[i].value : ""); }
    dw_arr_end(w, mk);
}

static void write_paths(DBuf *w, char (*ids)[40], int n) {
    size_t mk = dw_arr_begin(w, 4);
    char p[128];
    for (int i = 0; i < n; i++) { item_path(ids[i], p); dw_str(w, p); }
    dw_arr_end(w, mk);
}

/* ================= secrets ================= */

static void encode_secret(DBuf *w, Sess *s, const uint8_t *sec, size_t len, const char *ctype) {
    char sp[64];
    snprintf(sp, sizeof sp, BASE "/session/%u", s->id);
    dw_struct(w);
    dw_str(w, sp);
    if (s->dh) {
        uint8_t iv[16], iv2[16];
        vc_random(iv, 16);
        memcpy(iv2, iv, 16);
        size_t pl = len / 16 * 16 + 16;
        uint8_t *buf = sm_alloc(pl);
        if (!buf) { w->oom = 1; return; }
        memcpy(buf, sec, len);
        memset(buf + len, (int)(pl - len), pl - len);
        Aes a;
        aes_init(&a, s->key, 128);
        aes_cbc_encrypt(&a, iv2, buf, buf, pl);
        aes_wipe(&a);
        dw_bytes(w, iv, 16);
        dw_bytes(w, buf, pl);
        sm_free(buf);
    } else {
        dw_bytes(w, NULL, 0);
        dw_bytes(w, sec, len);
    }
    dw_str(w, ctype && ctype[0] ? ctype : "text/plain");
}

/* (oayays) → secret in smem. 0 ok, -2 no session, -1 invalid */
static int decode_secret(DRd *r, const char *sender, uint8_t **out, size_t *outlen, char ctype[64]) {
    *out = NULL;
    *outlen = 0;
    dr_struct(r);
    const char *sp = dr_str(r, NULL);
    uint32_t plen, vlen;
    const uint8_t *params = dr_bytes(r, &plen), *val = dr_bytes(r, &vlen);
    const char *ct = dr_str(r, NULL);
    if (r->err) return -1;
    snprintf(ctype, 64, "%s", ct);
    Sess *s = sess_find(sp, sender);
    if (!s) return -2;
    if (vlen > MAX_SECRET + 16) return -1;
    uint8_t *buf = sm_alloc(vlen + 1);
    if (!buf) return -1;
    memcpy(buf, val, vlen);
    if (s->dh) {
        if (plen != 16 || !vlen || vlen % 16) { sm_free(buf); return -1; }
        uint8_t iv[16];
        memcpy(iv, params, 16);
        Aes a;
        aes_init(&a, s->key, 128);
        aes_cbc_decrypt(&a, iv, buf, buf, vlen);
        aes_wipe(&a);
        long n = pkcs7_unpad(buf, vlen);
        if (n < 0) { sm_free(buf); return -1; }
        vlen = (uint32_t)n;
    }
    *out = buf;
    *outlen = vlen;
    return 0;
}

/* ================= introspection ================= */

static const char X_HEAD[] = "<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>";
static const char X_STD[] =
    "<interface name=\"org.freedesktop.DBus.Introspectable\"><method name=\"Introspect\"><arg name=\"xml\" type=\"s\" direction=\"out\"/></method></interface>"
    "<interface name=\"org.freedesktop.DBus.Peer\"><method name=\"Ping\"/></interface>"
    "<interface name=\"org.freedesktop.DBus.Properties\">"
    "<method name=\"Get\"><arg name=\"interface\" type=\"s\" direction=\"in\"/><arg name=\"property\" type=\"s\" direction=\"in\"/><arg name=\"value\" type=\"v\" direction=\"out\"/></method>"
    "<method name=\"GetAll\"><arg name=\"interface\" type=\"s\" direction=\"in\"/><arg name=\"properties\" type=\"a{sv}\" direction=\"out\"/></method>"
    "<method name=\"Set\"><arg name=\"interface\" type=\"s\" direction=\"in\"/><arg name=\"property\" type=\"s\" direction=\"in\"/><arg name=\"value\" type=\"v\" direction=\"in\"/></method>"
    "<signal name=\"PropertiesChanged\"><arg name=\"interface\" type=\"s\"/><arg name=\"changed\" type=\"a{sv}\"/><arg name=\"invalidated\" type=\"as\"/></signal></interface>";
static const char X_SVC[] =
    "<interface name=\"org.freedesktop.Secret.Service\">"
    "<method name=\"OpenSession\"><arg name=\"algorithm\" type=\"s\" direction=\"in\"/><arg name=\"input\" type=\"v\" direction=\"in\"/><arg name=\"output\" type=\"v\" direction=\"out\"/><arg name=\"result\" type=\"o\" direction=\"out\"/></method>"
    "<method name=\"CreateCollection\"><arg name=\"properties\" type=\"a{sv}\" direction=\"in\"/><arg name=\"alias\" type=\"s\" direction=\"in\"/><arg name=\"collection\" type=\"o\" direction=\"out\"/><arg name=\"prompt\" type=\"o\" direction=\"out\"/></method>"
    "<method name=\"SearchItems\"><arg name=\"attributes\" type=\"a{ss}\" direction=\"in\"/><arg name=\"unlocked\" type=\"ao\" direction=\"out\"/><arg name=\"locked\" type=\"ao\" direction=\"out\"/></method>"
    "<method name=\"Unlock\"><arg name=\"objects\" type=\"ao\" direction=\"in\"/><arg name=\"unlocked\" type=\"ao\" direction=\"out\"/><arg name=\"prompt\" type=\"o\" direction=\"out\"/></method>"
    "<method name=\"Lock\"><arg name=\"objects\" type=\"ao\" direction=\"in\"/><arg name=\"locked\" type=\"ao\" direction=\"out\"/><arg name=\"Prompt\" type=\"o\" direction=\"out\"/></method>"
    "<method name=\"GetSecrets\"><arg name=\"items\" type=\"ao\" direction=\"in\"/><arg name=\"session\" type=\"o\" direction=\"in\"/><arg name=\"secrets\" type=\"a{o(oayays)}\" direction=\"out\"/></method>"
    "<method name=\"ReadAlias\"><arg name=\"name\" type=\"s\" direction=\"in\"/><arg name=\"collection\" type=\"o\" direction=\"out\"/></method>"
    "<method name=\"SetAlias\"><arg name=\"name\" type=\"s\" direction=\"in\"/><arg name=\"collection\" type=\"o\" direction=\"in\"/></method>"
    "<signal name=\"CollectionCreated\"><arg name=\"collection\" type=\"o\"/></signal>"
    "<signal name=\"CollectionDeleted\"><arg name=\"collection\" type=\"o\"/></signal>"
    "<signal name=\"CollectionChanged\"><arg name=\"collection\" type=\"o\"/></signal>"
    "<property name=\"Collections\" type=\"ao\" access=\"read\"/></interface>"
    "<node name=\"collection\"/><node name=\"aliases\"/><node name=\"session\"/><node name=\"prompt\"/>";
static const char X_COLL[] =
    "<interface name=\"org.freedesktop.Secret.Collection\">"
    "<method name=\"Delete\"><arg name=\"prompt\" type=\"o\" direction=\"out\"/></method>"
    "<method name=\"SearchItems\"><arg name=\"attributes\" type=\"a{ss}\" direction=\"in\"/><arg name=\"results\" type=\"ao\" direction=\"out\"/></method>"
    "<method name=\"CreateItem\"><arg name=\"properties\" type=\"a{sv}\" direction=\"in\"/><arg name=\"secret\" type=\"(oayays)\" direction=\"in\"/><arg name=\"replace\" type=\"b\" direction=\"in\"/><arg name=\"item\" type=\"o\" direction=\"out\"/><arg name=\"prompt\" type=\"o\" direction=\"out\"/></method>"
    "<signal name=\"ItemCreated\"><arg name=\"item\" type=\"o\"/></signal>"
    "<signal name=\"ItemDeleted\"><arg name=\"item\" type=\"o\"/></signal>"
    "<signal name=\"ItemChanged\"><arg name=\"item\" type=\"o\"/></signal>"
    "<property name=\"Items\" type=\"ao\" access=\"read\"/><property name=\"Label\" type=\"s\" access=\"readwrite\"/>"
    "<property name=\"Locked\" type=\"b\" access=\"read\"/><property name=\"Created\" type=\"t\" access=\"read\"/><property name=\"Modified\" type=\"t\" access=\"read\"/></interface>";
static const char X_ITEM[] =
    "<interface name=\"org.freedesktop.Secret.Item\">"
    "<method name=\"Delete\"><arg name=\"Prompt\" type=\"o\" direction=\"out\"/></method>"
    "<method name=\"GetSecret\"><arg name=\"session\" type=\"o\" direction=\"in\"/><arg name=\"secret\" type=\"(oayays)\" direction=\"out\"/></method>"
    "<method name=\"SetSecret\"><arg name=\"secret\" type=\"(oayays)\" direction=\"in\"/></method>"
    "<property name=\"Locked\" type=\"b\" access=\"read\"/><property name=\"Attributes\" type=\"a{ss}\" access=\"readwrite\"/>"
    "<property name=\"Label\" type=\"s\" access=\"readwrite\"/><property name=\"Type\" type=\"s\" access=\"read\"/>"
    "<property name=\"Created\" type=\"t\" access=\"read\"/><property name=\"Modified\" type=\"t\" access=\"read\"/></interface>";
static const char X_SESS[] = "<interface name=\"org.freedesktop.Secret.Session\"><method name=\"Close\"/></interface>";
static const char X_PROMPT[] =
    "<interface name=\"org.freedesktop.Secret.Prompt\">"
    "<method name=\"Prompt\"><arg name=\"window-id\" type=\"s\" direction=\"in\"/></method><method name=\"Dismiss\"/>"
    "<signal name=\"Completed\"><arg name=\"dismissed\" type=\"b\"/><arg name=\"result\" type=\"v\"/></signal></interface>";

static void introspect(const DMsg *m, int kind) {
    size_t cap = 16384;
    char *x = malloc(cap);
    if (!x) { dbus_error(&C, m, E_FAILED, "memory"); return; }
    size_t n = (size_t)snprintf(x, cap, "%s", X_HEAD);
#define ADD(s) do { size_t l_ = strlen(s); if (n + l_ + 32 < cap) { memcpy(x + n, s, l_); n += l_; x[n] = 0; } } while (0)
    if (kind != O_NODE) ADD(X_STD);
    const char *p = m->path;
    if (kind == O_SERVICE) ADD(X_SVC);
    else if (kind == O_COLL) {
        ADD(X_COLL);
        if (!locked()) {
            char (*ids)[40];
            int k = search_ids(NULL, 0, &ids);
            for (int i = 0; i < k && n + 80 < cap; i++) n += (size_t)snprintf(x + n, cap - n, "<node name=\"%s\"/>", ids[i]);
            free(ids);
        }
    } else if (kind == O_ITEM) ADD(X_ITEM);
    else if (kind == O_SESS) ADD(X_SESS);
    else if (kind == O_PROMPT) ADD(X_PROMPT);
    else if (!strcmp(p, "/")) ADD("<node name=\"org\"/>");
    else if (!strcmp(p, "/org")) ADD("<node name=\"freedesktop\"/>");
    else if (!strcmp(p, "/org/freedesktop")) ADD("<node name=\"secrets\"/>");
    else if (!strcmp(p, BASE "/collection")) ADD("<node name=\"login\"/>");
    else if (!strcmp(p, BASE "/aliases")) ADD("<node name=\"default\"/>");
    ADD("</node>\n");
#undef ADD
    DBuf b;
    dw_init(&b);
    dw_str(&b, x);
    dbus_reply(&C, m, "s", &b);
    dw_free(&b);
    free(x);
}

/* ================= properties ================= */

static const char *item_type(const SsItemInfo *in) {
    for (int i = 0; in && i < in->nattrs; i++) if (in->attrs[i].name && !strcmp(in->attrs[i].name, "xdg:schema")) return in->attrs[i].value;
    return "org.freedesktop.Secret.Generic";
}

/* Writes the value (variant) of a property; -1 if it does not exist. */
static int put_prop(DBuf *w, int kind, const SsItemInfo *in, const char *name) {
    if (kind == O_SERVICE && !strcmp(name, "Collections")) {
        dw_variant(w, "ao");
        size_t mk = dw_arr_begin(w, 4);
        dw_str(w, COLL);
        dw_arr_end(w, mk);
        return 0;
    }
    if (kind == O_COLL) {
        if (!strcmp(name, "Items")) {
            dw_variant(w, "ao");
            char (*ids)[40] = NULL;
            int k = search_ids(NULL, 0, &ids);
            write_paths(w, ids, k > 0 ? k : 0);
            free(ids);
            return 0;
        }
        if (!strcmp(name, "Label")) { dw_variant(w, "s"); dw_str(w, "Login"); return 0; }
        if (!strcmp(name, "Locked")) { dw_variant(w, "b"); dw_bool(w, locked()); return 0; }
        if (!strcmp(name, "Created") || !strcmp(name, "Modified")) { dw_variant(w, "t"); dw_u64(w, created_at); return 0; }
        return -1;
    }
    if (kind == O_ITEM) {
        if (!strcmp(name, "Locked")) { dw_variant(w, "b"); dw_bool(w, locked() || !in); return 0; }
        if (!strcmp(name, "Attributes")) { dw_variant(w, "a{ss}"); write_attrs(w, in ? in->attrs : NULL, in ? in->nattrs : 0); return 0; }
        if (!strcmp(name, "Label")) { dw_variant(w, "s"); dw_str(w, in && in->label ? in->label : ""); return 0; }
        if (!strcmp(name, "Type")) { dw_variant(w, "s"); dw_str(w, item_type(in)); return 0; }
        if (!strcmp(name, "Created")) { dw_variant(w, "t"); dw_u64(w, in ? in->created : 0); return 0; }
        if (!strcmp(name, "Modified")) { dw_variant(w, "t"); dw_u64(w, in ? in->modified : 0); return 0; }
        return -1;
    }
    return -1;
}

static const char *const svc_props[] = { "Collections", NULL };
static const char *const coll_props[] = { "Items", "Label", "Locked", "Created", "Modified", NULL };
static const char *const item_props[] = { "Locked", "Attributes", "Label", "Type", "Created", "Modified", NULL };

static const char *iface_of(int kind) { return kind == O_SERVICE ? I_SVC : kind == O_COLL ? I_COLL : kind == O_ITEM ? I_ITEM : NULL; }

static void props_changed(const char *path, const char *iface, int kind, const char *prop) {
    DBuf b;
    dw_init(&b);
    dw_str(&b, iface);
    size_t mk = dw_arr_begin(&b, 8);
    dw_struct(&b);
    dw_str(&b, prop);
    put_prop(&b, kind, NULL, prop);
    dw_arr_end(&b, mk);
    mk = dw_arr_begin(&b, 4);
    dw_arr_end(&b, mk);
    dbus_signal(&C, path, I_PROPS, "PropertiesChanged", "sa{sv}as", &b);
    dw_free(&b);
}

static void sig_path(const char *path, const char *iface, const char *member, const char *arg) {
    DBuf b;
    dw_init(&b);
    dw_str(&b, arg);
    dbus_signal(&C, path, iface, member, "o", &b);
    dw_free(&b);
}

static int item_info(const char *id, SsItemInfo *in) {
    memset(in, 0, sizeof *in);
    if (locked()) return -1;
    if (B->info(U, id, in)) return -1;
    return 0;
}
static void info_free(SsItemInfo *in) { if (B->free_info) B->free_info(U, in); memset(in, 0, sizeof *in); }

static void handle_props(const DMsg *m, int kind, const char *id) {
    DRd r;
    dr_init(&r, m);
    const char *want = iface_of(kind);
    if (!strcmp(m->member, "Get") && !strcmp(m->sig, "ss")) {
        const char *ifc = dr_str(&r, NULL), *name = dr_str(&r, NULL);
        if (!want || (ifc[0] && strcmp(ifc, want))) { dbus_error(&C, m, E_UNKPROP, "no such interface"); return; }
        SsItemInfo in;
        int have = kind == O_ITEM && !item_info(id, &in);
        if (kind == O_ITEM && !have && !locked()) { dbus_error(&C, m, E_NOOBJ, "no such item"); return; }
        DBuf b;
        dw_init(&b);
        int ok = put_prop(&b, kind, have ? &in : NULL, name);
        if (have) info_free(&in);
        if (ok) dbus_error(&C, m, E_UNKPROP, name); else dbus_reply(&C, m, "v", &b);
        dw_free(&b);
        return;
    }
    if (!strcmp(m->member, "GetAll") && !strcmp(m->sig, "s")) {
        const char *ifc = dr_str(&r, NULL);
        DBuf b;
        dw_init(&b);
        size_t mk = dw_arr_begin(&b, 8);
        if (want && (!ifc[0] || !strcmp(ifc, want))) {
            const char *const *list = kind == O_SERVICE ? svc_props : kind == O_COLL ? coll_props : item_props;
            SsItemInfo in;
            int have = kind == O_ITEM && !item_info(id, &in);
            if (kind == O_ITEM && !have && !locked()) { dw_free(&b); dbus_error(&C, m, E_NOOBJ, "no such item"); return; }
            for (int i = 0; list[i]; i++) { dw_struct(&b); dw_str(&b, list[i]); put_prop(&b, kind, have ? &in : NULL, list[i]); }
            if (have) info_free(&in);
        }
        dw_arr_end(&b, mk);
        dbus_reply(&C, m, "a{sv}", &b);
        dw_free(&b);
        return;
    }
    if (!strcmp(m->member, "Set") && !strcmp(m->sig, "ssv")) {
        const char *ifc = dr_str(&r, NULL), *name = dr_str(&r, NULL), *vs = dr_variant(&r);
        if (kind != O_ITEM || (ifc[0] && strcmp(ifc, I_ITEM))) { dbus_error(&C, m, E_RO, name); return; }
        if (locked()) { dbus_error(&C, m, E_LOCKED, "the collection is locked"); return; }
        int rc;
        if (!strcmp(name, "Label") && !strcmp(vs, "s")) rc = B->set_label ? B->set_label(U, id, dr_str(&r, NULL)) : -1;
        else if (!strcmp(name, "Attributes") && !strcmp(vs, "a{ss}")) {
            SsAttr a[MAX_ATTRS];
            int n = read_attrs(&r, a, MAX_ATTRS);
            if (n < 0) { dbus_error(&C, m, E_ARGS, "too many attributes"); return; }
            rc = B->set_attrs ? B->set_attrs(U, id, a, n) : -1;
        } else { dbus_error(&C, m, E_RO, name); return; }
        if (rc) { dbus_error(&C, m, E_NOOBJ, "no such item"); return; }
        dbus_reply(&C, m, "", NULL);
        char p[128];
        item_path(id, p);
        sig_path(COLL, I_COLL, "ItemChanged", p);
        return;
    }
    dbus_error(&C, m, E_ARGS, "bad arguments");
}

/* ================= Service ================= */

static void open_session(const DMsg *m) {
    if (strcmp(m->sig, "sv")) { dbus_error(&C, m, E_ARGS, "sv"); return; }
    DRd r;
    dr_init(&r, m);
    const char *alg = dr_str(&r, NULL), *vs = dr_variant(&r);
    char path[64];
    DBuf b;
    dw_init(&b);
    if (!strcmp(alg, "plain")) {
        Sess *s = sess_new(m->sender);
        snprintf(path, sizeof path, BASE "/session/%u", s->id);
        dw_variant(&b, "s");
        dw_str(&b, "");
        dw_str(&b, path);
        LOG("OpenSession plain → %s (%s)\n", path, m->sender ? m->sender : "?");
        dbus_reply(&C, m, "vo", &b);
    } else if (!strcmp(alg, ALG_DH) && !strcmp(vs, "ay")) {
        uint32_t n;
        const uint8_t *pub = dr_bytes(&r, &n);
        uint8_t y[128] = { 0 }, x[128], gy[128], shared[128], g[128] = { 0 };
        if (r.err || !n || n > 128) { dw_free(&b); dbus_error(&C, m, E_ARGS, "bad public key"); return; }
        memcpy(y + 128 - n, pub, n);
        if (!dh_pub_ok(y)) { dw_free(&b); dbus_error(&C, m, E_ARGS, "bad public key"); return; }
        vc_random(x, sizeof x);
        x[0] &= 0x7f;
        g[127] = 2;
        if (ss_dh_modexp(g, x, P1024, gy) || ss_dh_modexp(y, x, P1024, shared)) { dw_free(&b); dbus_error(&C, m, E_FAILED, "dh"); return; }
        Sess *s = sess_new(m->sender);
        s->dh = 1;
        hkdf_sha256(NULL, 0, shared, sizeof shared, NULL, 0, s->key, 16);
        explicit_bzero(x, sizeof x);
        explicit_bzero(shared, sizeof shared);
        snprintf(path, sizeof path, BASE "/session/%u", s->id);
        dw_variant(&b, "ay");
        dw_bytes(&b, gy, 128);
        dw_str(&b, path);
        LOG("OpenSession %s → %s (%s)\n", ALG_DH, path, m->sender ? m->sender : "?");
        dbus_reply(&C, m, "vo", &b);
    } else {
        LOG("OpenSession %s: not supported\n", alg);
        dbus_error(&C, m, E_NOTSUP, "algorithm not supported");
    }
    dw_free(&b);
}

static void reply_oo(const DMsg *m, const char *a, const char *b2) {
    DBuf b;
    dw_init(&b);
    dw_str(&b, a);
    dw_str(&b, b2);
    dbus_reply(&C, m, "oo", &b);
    dw_free(&b);
}

static void search_items(const DMsg *m, int service) {
    if (strcmp(m->sig, "a{ss}")) { dbus_error(&C, m, E_ARGS, "a{ss}"); return; }
    DRd r;
    dr_init(&r, m);
    SsAttr a[MAX_ATTRS];
    int n = read_attrs(&r, a, MAX_ATTRS);
    if (n < 0) { dbus_error(&C, m, E_ARGS, "too many attributes"); return; }
    char (*ids)[40] = NULL;
    int k = search_ids(a, n, &ids);
    if (k < 0) k = 0;
    int lk = locked();
    DBuf b;
    dw_init(&b);
    if (service) {
        write_paths(&b, ids, lk ? 0 : k);
        write_paths(&b, ids, lk ? k : 0);
        dbus_reply(&C, m, "aoao", &b);
    } else {
        write_paths(&b, ids, k);
        dbus_reply(&C, m, "ao", &b);
    }
    LOG("SearchItems (%d attributes) → %d%s\n", n, k, lk ? " (locked)" : "");
    dw_free(&b);
    free(ids);
}

/* requested objects (ao) that are ours: the collection or items */
static int read_objs(DRd *r, char out[][128], int max) {
    int n = 0;
    size_t end = dr_arr(r, 4);
    while (dr_more(r, end)) {
        const char *p = dr_str(r, NULL);
        char id[40];
        unsigned num;
        int k = classify(p, id, &num);
        if ((k == O_COLL || k == O_ITEM) && n < max) snprintf(out[n++], 128, "%s", p);
    }
    return n;
}

static void reply_objs_o(const DMsg *m, char objs[][128], int n, const char *o) {
    DBuf b;
    dw_init(&b);
    size_t mk = dw_arr_begin(&b, 4);
    for (int i = 0; i < n; i++) dw_str(&b, objs[i]);
    dw_arr_end(&b, mk);
    dw_str(&b, o);
    dbus_reply(&C, m, "aoo", &b);
    dw_free(&b);
}

static void unlock(const DMsg *m) {
    if (strcmp(m->sig, "ao")) { dbus_error(&C, m, E_ARGS, "ao"); return; }
    DRd r;
    dr_init(&r, m);
    char objs[PROMPT_OBJS][128];
    int n = read_objs(&r, objs, PROMPT_OBJS);
    if (!locked()) { reply_objs_o(m, objs, n, "/"); return; }
    Prompt *p = prompt_new();
    int rc = B->request_unlock ? B->request_unlock(U, p->id) : -1;
    if (rc == 0 && !locked()) { p->used = 0; reply_objs_o(m, objs, n, "/"); return; }
    if (rc != 1) { p->used = 0; reply_objs_o(m, objs, 0, "/"); return; }
    for (int i = 0; i < n; i++) memcpy(p->obj[i], objs[i], 128);
    p->nobj = n;
    char pp[64];
    prompt_path(p->id, pp);
    LOG("Unlock → prompt %s\n", pp);
    reply_objs_o(m, objs, 0, pp);
}

static void lock(const DMsg *m) {
    if (strcmp(m->sig, "ao")) { dbus_error(&C, m, E_ARGS, "ao"); return; }
    DRd r;
    dr_init(&r, m);
    char objs[PROMPT_OBJS][128];
    int n = read_objs(&r, objs, PROMPT_OBJS);
    int was = locked();
    if (B->lock && B->lock(U)) { dbus_error(&C, m, E_FAILED, "cannot lock"); return; }
    reply_objs_o(m, objs, n, "/");
    if (!was && locked()) ss_notify_locked(1);
}

static void get_secrets(const DMsg *m) {
    if (strcmp(m->sig, "aoo")) { dbus_error(&C, m, E_ARGS, "aoo"); return; }
    DRd r;
    dr_init(&r, m);
    char objs[256][128];
    int n = 0;
    size_t end = dr_arr(&r, 4);
    while (dr_more(&r, end)) { const char *p = dr_str(&r, NULL); if (n < 256) snprintf(objs[n++], 128, "%s", p); }
    const char *sp = dr_str(&r, NULL);
    Sess *s = sess_find(sp, m->sender);
    if (!s) { dbus_error(&C, m, E_NOSESS, "no such session"); return; }
    DBuf b;
    dw_init(&b);
    size_t mk = dw_arr_begin(&b, 8);
    if (!locked()) {
        for (int i = 0; i < n; i++) {
            char id[40];
            unsigned num;
            if (classify(objs[i], id, &num) != O_ITEM) continue;
            uint8_t *sec = NULL;
            size_t len = 0;
            char ct[64] = "";
            if (B->get_secret(U, id, &sec, &len, ct)) continue;
            dw_struct(&b);
            dw_str(&b, objs[i]);
            encode_secret(&b, s, sec, len, ct);
            if (B->free_secret) B->free_secret(U, sec, len);
        }
    }
    dw_arr_end(&b, mk);
    dbus_reply(&C, m, "a{o(oayays)}", &b);
    dw_free(&b);
}

static void service_call(const DMsg *m) {
    const char *mb = m->member;
    if (!strcmp(mb, "OpenSession")) open_session(m);
    else if (!strcmp(mb, "SearchItems")) search_items(m, 1);
    else if (!strcmp(mb, "Unlock")) unlock(m);
    else if (!strcmp(mb, "Lock")) lock(m);
    else if (!strcmp(mb, "GetSecrets")) get_secrets(m);
    else if (!strcmp(mb, "CreateCollection")) {
        if (strcmp(m->sig, "a{sv}s")) { dbus_error(&C, m, E_ARGS, "a{sv}s"); return; }
        reply_oo(m, COLL, "/");
    } else if (!strcmp(mb, "ReadAlias")) {
        if (strcmp(m->sig, "s")) { dbus_error(&C, m, E_ARGS, "s"); return; }
        DRd r;
        dr_init(&r, m);
        const char *name = dr_str(&r, NULL);
        DBuf b;
        dw_init(&b);
        dw_str(&b, !strcmp(name, "default") || !strcmp(name, "login") ? COLL : "/");
        dbus_reply(&C, m, "o", &b);
        dw_free(&b);
    } else if (!strcmp(mb, "SetAlias")) {
        if (strcmp(m->sig, "so")) { dbus_error(&C, m, E_ARGS, "so"); return; }
        DRd r;
        dr_init(&r, m);
        const char *name = dr_str(&r, NULL), *p = dr_str(&r, NULL);
        char id[40];
        unsigned num;
        if ((!strcmp(name, "default") || !strcmp(name, "login")) && classify(p, id, &num) == O_COLL) dbus_reply(&C, m, "", NULL);
        else dbus_error(&C, m, E_NOTSUP, "only the login collection exists");
    } else dbus_error(&C, m, E_METHOD, mb);
}

/* ================= Collection and Item ================= */

static void create_item(const DMsg *m) {
    if (strcmp(m->sig, "a{sv}(oayays)b")) { dbus_error(&C, m, E_ARGS, "a{sv}(oayays)b"); return; }
    if (locked()) { dbus_error(&C, m, E_LOCKED, "the collection is locked"); return; }
    DRd r;
    dr_init(&r, m);
    const char *label = "";
    SsAttr a[MAX_ATTRS];
    int na = 0;
    size_t end = dr_arr(&r, 8);
    while (dr_more(&r, end)) {
        dr_struct(&r);
        const char *k = dr_str(&r, NULL), *vs = dr_variant(&r);
        if (!strcmp(k, "org.freedesktop.Secret.Item.Label") && !strcmp(vs, "s")) label = dr_str(&r, NULL);
        else if (!strcmp(k, "org.freedesktop.Secret.Item.Attributes") && !strcmp(vs, "a{ss}")) {
            na = read_attrs(&r, a, MAX_ATTRS);
            if (na < 0) { dbus_error(&C, m, E_ARGS, "too many attributes"); return; }
        } else dr_skip(&r, vs);
    }
    uint8_t *sec;
    size_t len;
    char ct[64];
    int rc = decode_secret(&r, m->sender, &sec, &len, ct);
    if (rc == -2) { dbus_error(&C, m, E_NOSESS, "no such session"); return; }
    if (rc) { dbus_error(&C, m, E_ARGS, "bad secret"); return; }
    int replace = dr_bool(&r);
    char id[40] = "";
    rc = B->create(U, label, a, na, sec, len, ct, replace, id);
    sm_free(sec);
    if (rc || !id_ok(id, strlen(id))) { dbus_error(&C, m, E_FAILED, "cannot create item"); return; }
    char p[128];
    item_path(id, p);
    LOG("CreateItem \"%s\" (%d attributes) → %s\n", label, na, p);
    reply_oo(m, p, "/");
    sig_path(COLL, I_COLL, "ItemCreated", p);
}

static void coll_call(const DMsg *m) {
    if (!strcmp(m->member, "SearchItems")) search_items(m, 0);
    else if (!strcmp(m->member, "CreateItem")) create_item(m);
    else if (!strcmp(m->member, "Delete")) dbus_error(&C, m, E_NOTSUP, "the login collection cannot be deleted");
    else dbus_error(&C, m, E_METHOD, m->member);
}

static void item_call(const DMsg *m, const char *id) {
    if (locked()) {
        if (!strcmp(m->member, "Delete") || !strcmp(m->member, "GetSecret") || !strcmp(m->member, "SetSecret")) { dbus_error(&C, m, E_LOCKED, "the collection is locked"); return; }
    }
    SsItemInfo in;
    if (item_info(id, &in)) { dbus_error(&C, m, E_NOOBJ, "no such item"); return; }
    info_free(&in);
    char p[128];
    item_path(id, p);
    if (!strcmp(m->member, "GetSecret")) {
        if (strcmp(m->sig, "o")) { dbus_error(&C, m, E_ARGS, "o"); return; }
        DRd r;
        dr_init(&r, m);
        Sess *s = sess_find(dr_str(&r, NULL), m->sender);
        if (!s) { dbus_error(&C, m, E_NOSESS, "no such session"); return; }
        uint8_t *sec = NULL;
        size_t len = 0;
        char ct[64] = "";
        if (B->get_secret(U, id, &sec, &len, ct)) { dbus_error(&C, m, E_NOOBJ, "no such item"); return; }
        DBuf b;
        dw_init(&b);
        encode_secret(&b, s, sec, len, ct);
        if (B->free_secret) B->free_secret(U, sec, len);
        dbus_reply(&C, m, "(oayays)", &b);
        dw_free(&b);
    } else if (!strcmp(m->member, "SetSecret")) {
        if (strcmp(m->sig, "(oayays)")) { dbus_error(&C, m, E_ARGS, "(oayays)"); return; }
        DRd r;
        dr_init(&r, m);
        uint8_t *sec;
        size_t len;
        char ct[64];
        int rc = decode_secret(&r, m->sender, &sec, &len, ct);
        if (rc == -2) { dbus_error(&C, m, E_NOSESS, "no such session"); return; }
        if (rc) { dbus_error(&C, m, E_ARGS, "bad secret"); return; }
        rc = B->set_secret ? B->set_secret(U, id, sec, len, ct) : -1;
        sm_free(sec);
        if (rc) { dbus_error(&C, m, E_FAILED, "cannot set secret"); return; }
        dbus_reply(&C, m, "", NULL);
        sig_path(COLL, I_COLL, "ItemChanged", p);
    } else if (!strcmp(m->member, "Delete")) {
        if (B->remove(U, id)) { dbus_error(&C, m, E_FAILED, "cannot delete"); return; }
        DBuf b;
        dw_init(&b);
        dw_str(&b, "/");
        dbus_reply(&C, m, "o", &b);
        dw_free(&b);
        sig_path(COLL, I_COLL, "ItemDeleted", p);
    } else dbus_error(&C, m, E_METHOD, m->member);
}

static void prompt_complete(Prompt *p) {
    char pp[64];
    prompt_path(p->id, pp);
    DBuf b;
    dw_init(&b);
    dw_bool(&b, !p->unlocked);
    dw_variant(&b, "ao");
    size_t mk = dw_arr_begin(&b, 4);
    if (p->unlocked) for (int i = 0; i < p->nobj; i++) dw_str(&b, p->obj[i]);
    dw_arr_end(&b, mk);
    dbus_signal(&C, pp, I_PROMPT, "Completed", "bv", &b);
    dw_free(&b);
    LOG("prompt %s: %s\n", pp, p->unlocked ? "unlocked" : "cancelled");
    p->used = 0;
}

/* ================= dispatch ================= */

static void note_caller(const DMsg *m) {
    if (!m->sender || !B->caller) return;
    for (int i = 0; i < 32; i++) if (!strcmp(pidc[i].name, m->sender)) { B->caller(U, m->sender, pidc[i].pid); return; }
    int k = pidc_next++ % 32;
    snprintf(pidc[k].name, sizeof pidc[k].name, "%s", m->sender);
    pidc[k].pid = 0;
    DBuf b;
    dw_init(&b);
    dw_str(&b, m->sender);
    DHdr h = { .type = DBUS_CALL, .path = "/org/freedesktop/DBus", .iface = "org.freedesktop.DBus",
               .member = "GetConnectionUnixProcessID", .dest = "org.freedesktop.DBus", .sig = "s" };
    pidc[k].serial = dbus_send(&C, &h, &b);
    dw_free(&b);
    B->caller(U, m->sender, 0);
}

static void handle_reply(const DMsg *m) {
    for (int i = 0; i < 32; i++) {
        if (pidc[i].serial && pidc[i].serial == m->reply_serial) {
            pidc[i].serial = 0;
            if (m->type == DBUS_RETURN && !strcmp(m->sig, "u")) {
                DRd r;
                dr_init(&r, m);
                pidc[i].pid = dr_u32(&r);
                if (B->caller) B->caller(U, pidc[i].name, pidc[i].pid);
            }
            return;
        }
    }
}

static void handle_call(const DMsg *m) {
    char id[40] = "";
    unsigned num = 0;
    int kind = classify(m->path, id, &num);
    const char *ifc = m->iface;
    LOG("%s %s.%s (%s)\n", m->path, ifc ? ifc : "", m->member, m->sender ? m->sender : "?");
    if (kind == O_NONE) { dbus_error(&C, m, E_UNKOBJ, m->path); return; }
    if ((!ifc || !strcmp(ifc, I_INTRO)) && !strcmp(m->member, "Introspect")) { introspect(m, kind); return; }
    if ((!ifc || !strcmp(ifc, I_PEER)) && !strcmp(m->member, "Ping")) { dbus_reply(&C, m, "", NULL); return; }
    if (ifc && !strcmp(ifc, I_PEER)) { dbus_error(&C, m, E_NOTSUP, m->member); return; }
    if (kind == O_NODE) { dbus_error(&C, m, E_UNKOBJ, m->path); return; }
    note_caller(m);
    if (ifc && !strcmp(ifc, I_PROPS)) {
        if (kind == O_ITEM && locked() && !strcmp(m->member, "Set")) { dbus_error(&C, m, E_LOCKED, "the collection is locked"); return; }
        handle_props(m, kind, id);
        return;
    }
    switch (kind) {
    case O_SERVICE: if (!ifc || !strcmp(ifc, I_SVC)) { service_call(m); return; } break;
    case O_COLL: if (!ifc || !strcmp(ifc, I_COLL)) { coll_call(m); return; } break;
    case O_ITEM: if (!ifc || !strcmp(ifc, I_ITEM)) { item_call(m, id); return; } break;
    case O_SESS:
        if ((!ifc || !strcmp(ifc, I_SESS)) && !strcmp(m->member, "Close")) {
            Sess *s = sess_find(m->path, m->sender);
            if (!s) { dbus_error(&C, m, E_NOOBJ, "no such session"); return; }
            sess_free(s);
            dbus_reply(&C, m, "", NULL);
            return;
        }
        break;
    case O_PROMPT: {
        Prompt *p = prompt_by_id(num);
        if (!p) { dbus_error(&C, m, E_NOOBJ, "no such prompt"); return; }
        if (!strcmp(m->member, "Prompt")) {
            dbus_reply(&C, m, "", NULL);
            p->prompted = 1;
            if (p->done) prompt_complete(p);
            return;
        }
        if (!strcmp(m->member, "Dismiss")) {
            dbus_reply(&C, m, "", NULL);
            p->unlocked = 0;
            prompt_complete(p);
            return;
        }
        break;
    }
    }
    dbus_error(&C, m, E_METHOD, m->member);
}

int ss_start(int busfd, const SsBackend *be, void *ud) {
    debug = getenv("LORD_SS_DEBUG") != NULL;
    B = be;
    U = ud;
    if (dbus_open(&C, busfd, 5000)) { LOG("could not authenticate with the bus\n"); dbus_close(&C); return -1; }
    DBuf b;
    dw_init(&b);
    dw_str(&b, "org.freedesktop.secrets");
    dw_u32(&b, 4);                               /* DBUS_NAME_FLAG_DO_NOT_QUEUE */
    DHdr h = { .type = DBUS_CALL, .path = "/org/freedesktop/DBus", .iface = "org.freedesktop.DBus",
               .member = "RequestName", .dest = "org.freedesktop.DBus", .sig = "su" };
    uint32_t s = dbus_send(&C, &h, &b);
    dw_free(&b);
    DMsg m;
    if (!s || dbus_wait_reply(&C, s, &m, 5000) != 1 || strcmp(m.sig, "u")) { dbus_msg_free(&m); dbus_close(&C); return -1; }
    DRd r;
    dr_init(&r, &m);
    uint32_t res = dr_u32(&r);
    dbus_msg_free(&m);
    if (res != 1 && res != 4) { LOG("org.freedesktop.secrets already has an owner (%u)\n", res); dbus_close(&C); return 1; }
    created_at = (uint64_t)time(NULL);
    running = 1;
    LOG("serving org.freedesktop.secrets as %s\n", C.unique);
    return 0;
}

int ss_fd(void) { return running ? C.fd : -1; }
int ss_pending_out(void) { return running && dbus_pending_out(&C); }
const char *ss_unique_name(void) { return running ? C.unique : ""; }

int ss_dispatch(void) {
    if (!running) return -1;
    for (int i = 0; i < 256; i++) {
        DMsg m;
        int r = dbus_next(&C, &m);
        if (r < 0) { LOG("the bus closed\n"); ss_stop(); return -1; }
        if (r == 0) break;
        if (m.type == DBUS_CALL) handle_call(&m);
        else if (m.type == DBUS_RETURN || m.type == DBUS_ERROR) handle_reply(&m);
        else if (m.type == DBUS_SIGNAL && m.member && !strcmp(m.member, "NameLost")) LOG("the name was lost\n");
        dbus_msg_free(&m);
    }
    if (dbus_flush(&C) < 0) { ss_stop(); return -1; }
    return 0;
}

void ss_prompt_done(unsigned prompt_id, int unlocked) {
    Prompt *p = prompt_by_id(prompt_id);
    if (!running || !p) return;
    p->done = 1;
    p->unlocked = unlocked != 0;
    if (p->prompted) prompt_complete(p);
    if (unlocked) ss_notify_locked(0);
}

void ss_notify_locked(int lk) {
    if (!running) return;
    (void)lk;
    props_changed(COLL, I_COLL, O_COLL, "Locked");
    sig_path(BASE, I_SVC, "CollectionChanged", COLL);
}

void ss_notify_item(const char *id, int what) {
    if (!running || !id || !id_ok(id, strlen(id))) return;
    char p[128];
    item_path(id, p);
    sig_path(COLL, I_COLL, what == 0 ? "ItemCreated" : what == 1 ? "ItemChanged" : "ItemDeleted", p);
}

void ss_stop(void) {
    if (C.fd >= 0 || C.in || C.out) dbus_close(&C);
    for (int i = 0; i < NSESS; i++) sess_free(&sess[i]);
    memset(prompts, 0, sizeof prompts);
    running = 0;
}
