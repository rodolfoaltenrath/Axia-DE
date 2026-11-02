/* rtcwire.c — WebRTC wire formats for the remote desktop's video path (see rtcwire.h and
 * docs/REMOTE.md, "Video (WebRTC)"). Plain libc: unit tests in tests/remote_sec_test.c and the
 * ASan+UBSan fuzz run in tests/remote_fuzz.c. */
#include "rtcwire.h"

#include <arpa/inet.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint16_t be16(const uint8_t *p) { return (uint16_t)(p[0] << 8 | p[1]); }
static uint32_t be32(const uint8_t *p) { return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; }
static void put16(uint8_t *p, uint16_t v) { p[0] = (uint8_t)(v >> 8); p[1] = (uint8_t)v; }
static void put32(uint8_t *p, uint32_t v) { p[0] = (uint8_t)(v >> 24); p[1] = (uint8_t)(v >> 16); p[2] = (uint8_t)(v >> 8); p[3] = (uint8_t)v; }

/* ---- addresses ---- */

int rw_addr_text(const char *s, RwAddr *a) {
    uint8_t b[16];
    size_t l = strlen(s);
    if (!l || l > 45) return -1;
    for (size_t i = 0; i < l; i++) if (!strchr("0123456789abcdefABCDEF:.", s[i])) return -1;
    if (inet_pton(AF_INET, s, b) == 1) { a->family = 4; memset(a->a, 0, 16); memcpy(a->a, b, 4); return 0; }
    if (inet_pton(AF_INET6, s, b) == 1) {
        static const uint8_t mapped[12] = { 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff };
        if (!memcmp(b, mapped, 12)) { a->family = 4; memset(a->a, 0, 16); memcpy(a->a, b + 12, 4); return 0; }
        a->family = 6;
        memcpy(a->a, b, 16);
        return 0;
    }
    return -1;
}

void rw_addr_format(const RwAddr *a, char *out, size_t n) {
    if (!n) return;
    out[0] = 0;
    if (a->family == 4) inet_ntop(AF_INET, a->a, out, (socklen_t)n);
    else if (a->family == 6) inet_ntop(AF_INET6, a->a, out, (socklen_t)n);
}

int rw_addr_equal(const RwAddr *x, const RwAddr *y) {
    if (x->family != y->family || x->port != y->port || !x->family) return 0;
    return !memcmp(x->a, y->a, x->family == 4 ? 4 : 16);
}

int rw_ice_server_valid(const char *s) {
    size_t n = strlen(s);
    const char *colon = strrchr(s, ':');
    if (!n || n > 95 || !colon || colon == s) return 0;
    size_t hl = (size_t)(colon - s), pl = n - hl - 1;
    if (!pl || pl > 5 || (pl > 1 && colon[1] == '0')) return 0;
    long port = 0;
    for (size_t i = 0; i < pl; i++) { if (colon[1 + i] < '0' || colon[1 + i] > '9') return 0; port = port * 10 + (colon[1 + i] - '0'); }
    if (port < 1 || port > 65535) return 0;
    char host[96];
    memcpy(host, s, hl);
    host[hl] = 0;
    RwAddr a;
    if (host[0] == '[') {
        if (hl < 4 || host[hl - 1] != ']') return 0;
        host[hl - 1] = 0;
        return !rw_addr_text(host + 1, &a) && a.family == 6;
    }
    if (!rw_addr_text(host, &a)) return a.family == 4;
    if (hl > 63) return 0;
    for (size_t i = 0; i < hl; i++) {
        char c = host[i];
        int ok = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || ((c == '-' || c == '.') && i > 0 && i < hl - 1);
        if (!ok || (c == '.' && host[i - 1] == '.')) return 0;
    }
    return 1;
}

int rw_turn_user_valid(const char *s) {
    size_t n = strlen(s);
    if (!n || n > 128) return 0;
    for (size_t i = 0; i < n; i++) if (s[i] <= 0x20 || s[i] > 0x7e || s[i] == '"' || s[i] == '\\') return 0;
    return 1;
}

int rw_turn_pass_valid(const char *s) {
    size_t n = strlen(s);
    if (!n || n > 128) return 0;
    for (size_t i = 0; i < n; i++) if (s[i] < 0x20 || s[i] > 0x7e) return 0;
    return 1;
}

int rw_turn_transport(const char *s) {
    if (!strcmp(s, "udp")) return RW_TURN_UDP;
    if (!strcmp(s, "tcp")) return RW_TURN_TCP;
    if (!strcmp(s, "tls")) return RW_TURN_TLS;
    return -1;
}
const char *rw_turn_transport_name(int t) { return t == RW_TURN_TLS ? "tls" : t == RW_TURN_TCP ? "tcp" : "udp"; }

int rw_fp_parse(const char *s, uint8_t out[32]) {
    size_t n = strlen(s), k = 0;
    int colons = n == 95;
    if (n != 64 && n != 95) return -1;
    for (size_t i = 0; i < n;) {
        if (colons && i % 3 == 2) { if (s[i] != ':') return -1; i++; continue; }
        int hi = -1, lo = -1;
        char c = s[i], d = i + 1 < n ? s[i + 1] : 0;
        hi = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1;
        lo = d >= '0' && d <= '9' ? d - '0' : d >= 'a' && d <= 'f' ? d - 'a' + 10 : d >= 'A' && d <= 'F' ? d - 'A' + 10 : -1;
        if (hi < 0 || lo < 0 || k >= 32) return -1;
        out[k++] = (uint8_t)(hi << 4 | lo);
        i += 2;
    }
    return k == 32 ? 0 : -1;
}

/* ---- small text helpers ---- */

static int ice_char(char c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '+' || c == '/';
}
static int all_ice(const char *s, size_t n) { for (size_t i = 0; i < n; i++) if (!ice_char(s[i])) return 0; return 1; }
static int tok_char(char c) { return c > 0x20 && c < 0x7f; }

/* An unsigned decimal of at most `digits` digits, no leading zeros (except "0"), up to max. */
static int parse_uint(const char *s, size_t n, unsigned long long max, unsigned long long *v) {
    if (!n || n > 20 || (n > 1 && s[0] == '0')) return -1;
    unsigned long long x = 0;
    for (size_t i = 0; i < n; i++) {
        if (s[i] < '0' || s[i] > '9') return -1;
        x = x * 10 + (unsigned)(s[i] - '0');
        if (x > max) return -1;
    }
    *v = x;
    return 0;
}

/* Splits s[0..n) on single spaces into at most max tokens; -1 on an empty token or too many. */
static int split(const char *s, size_t n, const char **tok, size_t *len, int max) {
    int k = 0;
    size_t i = 0;
    while (i < n) {
        size_t j = i;
        while (j < n && s[j] != ' ') j++;
        if (j == i || k >= max) return -1;
        tok[k] = s + i;
        len[k] = j - i;
        k++;
        i = j + 1;
        if (j < n && i == n) return -1;          /* trailing space */
    }
    return k;
}

static int eq(const char *s, size_t n, const char *lit) { return strlen(lit) == n && !memcmp(s, lit, n); }
static int ieq(const char *s, size_t n, const char *lit) {
    if (strlen(lit) != n) return 0;
    for (size_t i = 0; i < n; i++) {
        char a = s[i], b = lit[i];
        if (a >= 'A' && a <= 'Z') a = (char)(a + 32);
        if (b >= 'A' && b <= 'Z') b = (char)(b + 32);
        if (a != b) return 0;
    }
    return 1;
}
static int prefix(const char *s, size_t n, const char *lit, size_t *after) {
    size_t l = strlen(lit);
    if (n < l || memcmp(s, lit, l)) return 0;
    *after = l;
    return 1;
}

static int hexval(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

/* ---- ICE candidates ---- */

int rw_cand_parse(const char *s, size_t n, RwCand *c) {
    memset(c, 0, sizeof *c);
    size_t off;
    if (prefix(s, n, "a=", &off)) { s += off; n -= off; }
    if (n > 512 || !prefix(s, n, "candidate:", &off)) return RW_ESYNTAX;
    for (size_t i = 0; i < n; i++) if (s[i] < 0x20 || s[i] > 0x7e) return RW_ESYNTAX;
    s += off;
    n -= off;
    const char *t[24];
    size_t l[24];
    int k = split(s, n, t, l, 24);
    if (k < 8 || (k - 8) % 2) return RW_ESYNTAX;
    if (l[0] > 32 || !all_ice(t[0], l[0])) return RW_ESYNTAX;            /* foundation */
    unsigned long long v;
    if (parse_uint(t[1], l[1], 256, &v) || v < 1) return RW_ESYNTAX;
    c->component = (int)v;
    if (ieq(t[2], l[2], "tcp")) c->tcp = 1;
    else if (!ieq(t[2], l[2], "udp")) return RW_ESYNTAX;
    if (parse_uint(t[3], l[3], 0xffffffffull, &v)) return RW_ESYNTAX;
    c->prio = (uint32_t)v;
    char host[64];
    if (l[4] >= sizeof host) return RW_ESYNTAX;
    memcpy(host, t[4], l[4]);
    host[l[4]] = 0;
    if (rw_addr_text(host, &c->addr)) {
        /* an mDNS name: <uuid>.local (browsers hide local addresses this way) */
        if (l[4] < 7 || l[4] > 63 || memcmp(host + l[4] - 6, ".local", 6)) return RW_ESYNTAX;
        for (size_t i = 0; i < l[4] - 6; i++) if (!((host[i] >= '0' && host[i] <= '9') || (host[i] >= 'a' && host[i] <= 'f') || host[i] == '-')) return RW_ESYNTAX;
        c->mdns = 1;
    }
    if (parse_uint(t[5], l[5], 65535, &v)) return RW_ESYNTAX;
    if (v == 0 && !c->tcp) return RW_ESYNTAX;
    c->addr.port = (uint16_t)v;
    if (!eq(t[6], l[6], "typ")) return RW_ESYNTAX;
    if (eq(t[7], l[7], "host")) c->type = RW_CAND_HOST;
    else if (eq(t[7], l[7], "srflx")) c->type = RW_CAND_SRFLX;
    else if (eq(t[7], l[7], "prflx")) c->type = RW_CAND_PRFLX;
    else if (eq(t[7], l[7], "relay")) c->type = RW_CAND_RELAY;
    else return RW_ESYNTAX;
    /* extensions: name value pairs of printable tokens (raddr, rport, generation, ufrag, network-id…) */
    for (int i = 8; i < k; i += 2) {
        if (l[i] > 32 || l[i + 1] > 64) return RW_ESYNTAX;
        for (size_t j = 0; j < l[i]; j++) if (!((t[i][j] >= 'a' && t[i][j] <= 'z') || t[i][j] == '-')) return RW_ESYNTAX;
        for (size_t j = 0; j < l[i + 1]; j++) if (!tok_char(t[i + 1][j])) return RW_ESYNTAX;
    }
    return RW_OK;
}

/* ---- SDP offer ---- */

typedef struct { const char *p; size_t n; } Line;

static RwCodec *find_pt(RwMedia *m, unsigned pt) {
    for (int i = 0; i < m->npt; i++) if (m->pt[i].pt == (int)pt) return &m->pt[i];
    return NULL;
}

static int set_str(char *dst, size_t cap, const char *s, size_t n) {
    if (dst[0]) return strlen(dst) == n && !memcmp(dst, s, n) ? 0 : -1;    /* the same everywhere */
    if (n >= cap) return -1;
    memcpy(dst, s, n);
    dst[n] = 0;
    return 0;
}

static int parse_fp(const char *s, size_t n, uint8_t out[32]) {
    if (n != 32 * 3 - 1) return -1;
    for (int i = 0; i < 32; i++) {
        int h = hexval(s[i * 3]), l = hexval(s[i * 3 + 1]);
        if (h < 0 || l < 0 || (i < 31 && s[i * 3 + 2] != ':')) return -1;
        out[i] = (uint8_t)(h << 4 | l);
    }
    return 0;
}

static int parse_mline(const char *s, size_t n, RwMedia *m) {
    const char *t[RW_MAX_PT + 3];
    size_t l[RW_MAX_PT + 3];
    int k = split(s, n, t, l, RW_MAX_PT + 3);
    if (k < 4) return k == -1 && n > 64 ? RW_ETOOBIG : RW_ESYNTAX;
    unsigned long long port;
    if (parse_uint(t[1], l[1], 65535, &port)) return RW_ESYNTAX;
    if (eq(t[0], l[0], "audio")) m->kind = RW_KIND_AUDIO;
    else if (eq(t[0], l[0], "video")) m->kind = RW_KIND_VIDEO;
    else if (eq(t[0], l[0], "application")) m->kind = RW_KIND_APP;
    else { m->kind = 0; m->rejected = 1; return RW_OK; }          /* something else: answered with port 0 */
    if (port == 0) m->rejected = 1;
    if (m->kind == RW_KIND_APP) {
        if (!eq(t[2], l[2], "UDP/DTLS/SCTP") || k != 4 || !eq(t[3], l[3], "webrtc-datachannel")) m->rejected = 1;
        return RW_OK;
    }
    if (!eq(t[2], l[2], "UDP/TLS/RTP/SAVPF")) m->rejected = 1;
    for (int i = 3; i < k; i++) {
        unsigned long long pt;
        if (parse_uint(t[i], l[i], 127, &pt)) return RW_ESYNTAX;
        if (find_pt(m, (unsigned)pt)) return RW_ESYNTAX;            /* listed twice */
        RwCodec *c = &m->pt[m->npt++];
        memset(c, 0, sizeof *c);
        c->pt = (int)pt;
    }
    return RW_OK;
}

/* "<pt> rest": the payload type (listed on the m= line) and what follows */
static RwCodec *pt_attr(RwMedia *m, const char *s, size_t n, const char **rest, size_t *rl) {
    size_t i = 0;
    while (i < n && s[i] != ' ') i++;
    unsigned long long pt;
    if (i == n || parse_uint(s, i, 127, &pt)) return NULL;
    *rest = s + i + 1;
    *rl = n - i - 1;
    return find_pt(m, (unsigned)pt);
}

static int parse_fmtp(RwCodec *c, const char *s, size_t n) {
    if (!n) return RW_ESYNTAX;
    c->has_fmtp = 1;
    size_t i = 0;
    while (i < n) {
        size_t j = i;
        while (j < n && s[j] != ';') j++;
        const char *kv = s + i;
        size_t kl = j - i;
        while (kl && kv[0] == ' ') { kv++; kl--; }
        size_t a;
        unsigned long long v;
        if (prefix(kv, kl, "packetization-mode=", &a)) {
            if (parse_uint(kv + a, kl - a, 2, &v)) return RW_ESYNTAX;
            c->pmode = (int)v;
        } else if (prefix(kv, kl, "profile-level-id=", &a)) {
            if (kl - a != 6) return RW_ESYNTAX;
            uint32_t x = 0;
            for (size_t q = a; q < kl; q++) { int h = hexval(kv[q]); if (h < 0) return RW_ESYNTAX; x = x << 4 | (uint32_t)h; }
            c->plid = x;
        }
        i = j + 1;
    }
    return RW_OK;
}

static int parse_attr(RwOffer *o, RwMedia *m, const char *s, size_t n, int *bundle, int *mux) {
    size_t a;
    unsigned long long v;
    if (prefix(s, n, "ice-ufrag:", &a)) {
        if (n - a < 4 || n - a > 256 || !all_ice(s + a, n - a) || set_str(o->ufrag, sizeof o->ufrag, s + a, n - a)) return RW_EICE;
    } else if (prefix(s, n, "ice-pwd:", &a)) {
        if (n - a < 22 || n - a > 256 || !all_ice(s + a, n - a) || set_str(o->pwd, sizeof o->pwd, s + a, n - a)) return RW_EICE;
    } else if (prefix(s, n, "fingerprint:", &a)) {
        const char *f = s + a;
        size_t fl = n - a, sp;
        if (prefix(f, fl, "sha-256 ", &sp) || prefix(f, fl, "SHA-256 ", &sp)) {
            uint8_t fp[32];
            static const uint8_t zero[32];
            if (parse_fp(f + sp, fl - sp, fp)) return RW_EFP;
            if (memcmp(o->fp, zero, 32) && memcmp(o->fp, fp, 32)) return RW_EFP;       /* two different ones */
            memcpy(o->fp, fp, 32);
        }
        /* other hash functions are not used */
    } else if (prefix(s, n, "setup:", &a)) {
        int st = eq(s + a, n - a, "actpass") ? RW_SETUP_ACTPASS : eq(s + a, n - a, "active") ? RW_SETUP_ACTIVE : 0;
        if (!st || (o->setup && o->setup != st)) return RW_ESYNTAX;      /* we are the passive side */
        o->setup = st;
    } else if (prefix(s, n, "group:BUNDLE ", &a)) {
        if (m) return RW_ESYNTAX;
        *bundle = 1;
    } else if (prefix(s, n, "candidate:", &a)) {
        RwCand c;
        if (rw_cand_parse(s, n, &c)) return RW_ESYNTAX;
        if (o->ncand < RW_MAX_CAND) o->cand[o->ncand++] = c;
    } else if (!m) {
        return RW_OK;                                                  /* other session attributes */
    } else if (prefix(s, n, "mid:", &a)) {
        if (m->mid[0] || n - a < 1 || n - a > 32) return RW_ESYNTAX;
        for (size_t i = a; i < n; i++) if (!((s[i] >= 'a' && s[i] <= 'z') || (s[i] >= 'A' && s[i] <= 'Z') || (s[i] >= '0' && s[i] <= '9') || s[i] == '_' || s[i] == '-')) return RW_ESYNTAX;
        memcpy(m->mid, s + a, n - a);
        m->mid[n - a] = 0;
    } else if (eq(s, n, "recvonly")) m->dir = RW_DIR_RECVONLY;
    else if (eq(s, n, "sendrecv")) m->dir = RW_DIR_SENDRECV;
    else if (eq(s, n, "sendonly")) m->dir = RW_DIR_SENDONLY;
    else if (eq(s, n, "inactive")) m->dir = RW_DIR_INACTIVE;
    else if (eq(s, n, "rtcp-mux")) *mux = 1;
    else if (prefix(s, n, "rtpmap:", &a)) {
        const char *r;
        size_t rl;
        RwCodec *c = pt_attr(m, s + a, n - a, &r, &rl);
        if (!c) return RW_ESYNTAX;
        if (c->name[0]) return RW_ESYNTAX;                             /* twice */
        const char *t[3];
        size_t l[3];
        size_t i = 0;
        int k = 0;
        while (k < 3) {                                                /* name/clock[/channels] */
            size_t j = i;
            while (j < rl && r[j] != '/') j++;
            if (j == i) return RW_ESYNTAX;
            t[k] = r + i;
            l[k] = j - i;
            k++;
            if (j == rl) break;
            i = j + 1;
            if (i == rl) return RW_ESYNTAX;
        }
        if (k < 2 || l[0] >= sizeof c->name || t[k - 1] + l[k - 1] != r + rl) return RW_ESYNTAX;
        for (size_t q = 0; q < l[0]; q++) if (!((t[0][q] >= 'a' && t[0][q] <= 'z') || (t[0][q] >= 'A' && t[0][q] <= 'Z') || (t[0][q] >= '0' && t[0][q] <= '9') || t[0][q] == '-' || t[0][q] == '_')) return RW_ESYNTAX;
        if (parse_uint(t[1], l[1], 1000000, &v) || !v) return RW_ESYNTAX;
        c->clock = (int)v;
        c->channels = 1;
        if (k == 3) { if (parse_uint(t[2], l[2], 16, &v) || !v) return RW_ESYNTAX; c->channels = (int)v; }
        memcpy(c->name, t[0], l[0]);
        c->name[l[0]] = 0;
    } else if (prefix(s, n, "fmtp:", &a)) {
        const char *r;
        size_t rl;
        RwCodec *c = pt_attr(m, s + a, n - a, &r, &rl);
        if (!c || c->has_fmtp) return RW_ESYNTAX;
        return parse_fmtp(c, r, rl);
    } else if (prefix(s, n, "rtcp-fb:", &a)) {
        const char *r = NULL;
        size_t rl = 0;
        RwCodec *c = NULL;
        int all = n - a >= 2 && s[a] == '*' && s[a + 1] == ' ';
        if (all) { r = s + a + 2; rl = n - a - 2; }
        else if (!(c = pt_attr(m, s + a, n - a, &r, &rl))) return RW_ESYNTAX;
        int nack = eq(r, rl, "nack"), pli = eq(r, rl, "nack pli"), fir = eq(r, rl, "ccm fir");
        for (int i = 0; i < m->npt; i++) {
            RwCodec *x = &m->pt[i];
            if (!all && x != c) continue;
            x->fb_nack |= nack;
            x->fb_pli |= pli;
            x->fb_fir |= fir;
        }
    } else if (prefix(s, n, "sctp-port:", &a)) {
        if (parse_uint(s + a, n - a, 65535, &v) || !v) return RW_ESYNTAX;
        m->sctp_port = (int)v;
    } else if (prefix(s, n, "max-message-size:", &a)) {
        if (parse_uint(s + a, n - a, 1ull << 40, &v)) return RW_ESYNTAX;
        m->max_msg = (long)v;
    }
    return RW_OK;
}

/* constrained baseline: profile_idc 66 with constraint_set1 */
static int h264_ok(const RwCodec *c) {
    return ieq(c->name, strlen(c->name), "H264") && c->clock == 90000 && c->pmode == 1 && (c->plid >> 16) == 0x42 && (c->plid & 0x4000);
}

int rw_sdp_parse_offer(const char *sdp, size_t n, int allow_h264, RwOffer *o) {
    memset(o, 0, sizeof *o);
    o->vm = o->am = o->dm = -1;
    if (n > RW_SDP_MAX) return RW_ETOOBIG;
    if (n < 4 || memcmp(sdp, "v=0", 3)) return RW_ESYNTAX;
    int cur = -1, lines = 0, bundle = 0;
    int mux[RW_MAX_MEDIA] = { 0 }, smux = 0;
    size_t i = 0;
    while (i < n) {
        size_t j = i;
        while (j < n && sdp[j] != '\n') j++;
        size_t e = j;
        if (e > i && sdp[e - 1] == '\r') e--;
        const char *s = sdp + i;
        size_t l = e - i;
        i = j < n ? j + 1 : j;
        if (++lines > RW_SDP_LINES || l > RW_SDP_LINE) return RW_ETOOBIG;
        if (l == 0 && i >= n) break;                                   /* the final line break */
        if (l < 2 || s[1] != '=' || s[0] < 'a' || s[0] > 'z') return RW_ESYNTAX;
        for (size_t q = 0; q < l; q++) if (s[q] < 0x20 || s[q] > 0x7e) return RW_ESYNTAX;
        if (lines == 1 && !eq(s, l, "v=0")) return RW_ESYNTAX;
        if (s[0] == 'm') {
            if (o->nmedia >= RW_MAX_MEDIA) return RW_ETOOBIG;
            cur = o->nmedia++;
            int r = parse_mline(s + 2, l - 2, &o->m[cur]);
            if (r) return r;
        } else if (s[0] == 'a') {
            int r = parse_attr(o, cur >= 0 ? &o->m[cur] : NULL, s + 2, l - 2, &bundle, cur >= 0 ? &mux[cur] : &smux);
            if (r) return r;
        }
    }
    if (!o->ufrag[0] || !o->pwd[0]) return RW_EICE;
    static const uint8_t zero[32];
    if (!memcmp(o->fp, zero, 32)) return RW_EFP;
    if (!o->setup || !bundle || !o->nmedia) return RW_ESYNTAX;
    for (int k = 0; k < o->nmedia; k++) {
        RwMedia *m = &o->m[k];
        if (!m->mid[0]) return RW_ESYNTAX;
        for (int q = 0; q < k; q++) if (!strcmp(o->m[q].mid, m->mid)) return RW_ESYNTAX;
        if (m->rejected) continue;
        if (m->kind != RW_KIND_APP && !mux[k] && !smux) return RW_ESYNTAX;
        if (m->kind == RW_KIND_VIDEO) {
            if (o->vm >= 0) { m->rejected = 1; continue; }
            if (m->dir != RW_DIR_RECVONLY && m->dir != RW_DIR_SENDRECV) { m->rejected = 1; continue; }
            int best = -1, vp8 = -1;
            for (int q = 0; q < m->npt; q++) {
                const RwCodec *c = &m->pt[q];
                if (allow_h264 && h264_ok(c) && (best < 0 || c->plid == 0x42e01f)) best = q;
                if (vp8 < 0 && ieq(c->name, strlen(c->name), "VP8") && c->clock == 90000) vp8 = q;
            }
            if (best >= 0) { o->vcodec = RW_CODEC_H264; o->vpt = m->pt[best].pt; }
            else if (vp8 >= 0) { o->vcodec = RW_CODEC_VP8; o->vpt = m->pt[vp8].pt; }
            else { m->rejected = 1; continue; }
            o->vm = k;
        } else if (m->kind == RW_KIND_AUDIO) {
            if (o->am >= 0 || (m->dir != RW_DIR_RECVONLY && m->dir != RW_DIR_SENDRECV)) { m->rejected = 1; continue; }
            for (int q = 0; q < m->npt && o->am < 0; q++) {
                const RwCodec *c = &m->pt[q];
                if (ieq(c->name, strlen(c->name), "opus") && c->clock == 48000 && c->channels == 2) { o->am = k; o->apt = c->pt; }
            }
            if (o->am < 0) m->rejected = 1;
        } else if (m->kind == RW_KIND_APP) {
            if (o->dm >= 0 || !m->sctp_port) { m->rejected = 1; continue; }
            o->dm = k;
        }
    }
    if (o->vm < 0) return RW_ECODEC;
    return RW_OK;
}

/* ---- SDP answer ---- */

typedef struct { char *p; size_t n, cap; int bad; } Out;

static void outf(Out *o, const char *fmt, ...) __attribute__((format(printf, 2, 3)));
static void outf(Out *o, const char *fmt, ...) {
    if (o->bad) return;
    va_list ap;
    va_start(ap, fmt);
    int k = vsnprintf(o->p + o->n, o->cap - o->n, fmt, ap);
    va_end(ap);
    if (k < 0 || (size_t)k >= o->cap - o->n) { o->bad = 1; return; }
    o->n += (size_t)k;
}

static void out_fp(Out *o, const uint8_t *fp) {
    outf(o, "a=fingerprint:sha-256 ");
    for (int i = 0; i < 32; i++) outf(o, "%02X%s", fp[i], i < 31 ? ":" : "\r\n");
}

static const char *kind_name(int k) { return k == RW_KIND_AUDIO ? "audio" : k == RW_KIND_VIDEO ? "video" : "application"; }

int rw_sdp_answer(const RwOffer *of, const RwAnswer *a, char *buf, size_t cap) {
    Out o = { buf, 0, cap, 0 };
    if (!cap) return -1;
    buf[0] = 0;
    outf(&o, "v=0\r\no=- %u %u IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=ice-lite\r\na=group:BUNDLE", a->sess_id, a->version ? a->version : 2);
    for (int i = 0; i < of->nmedia; i++) if (!of->m[i].rejected) outf(&o, " %s", of->m[i].mid);
    outf(&o, "\r\na=msid-semantic: WMS lordos\r\n");
    int first = 1;
    for (int i = 0; i < of->nmedia; i++) {
        const RwMedia *m = &of->m[i];
        if (m->rejected) {
            if (m->kind == RW_KIND_APP) outf(&o, "m=application 0 UDP/DTLS/SCTP webrtc-datachannel\r\n");
            else if (m->kind) outf(&o, "m=%s 0 UDP/TLS/RTP/SAVPF %d\r\n", kind_name(m->kind), m->npt ? m->pt[0].pt : 0);
            else outf(&o, "m=audio 0 UDP/TLS/RTP/SAVPF 0\r\n");
            outf(&o, "c=IN IP4 0.0.0.0\r\na=mid:%s\r\na=inactive\r\n", m->mid);
            continue;
        }
        if (m->kind == RW_KIND_APP) outf(&o, "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n");
        else outf(&o, "m=%s 9 UDP/TLS/RTP/SAVPF %d\r\n", kind_name(m->kind), i == of->vm ? of->vpt : of->apt);
        outf(&o, "c=IN IP4 0.0.0.0\r\n");
        if (m->kind != RW_KIND_APP) outf(&o, "a=rtcp:9 IN IP4 0.0.0.0\r\n");
        outf(&o, "a=ice-ufrag:%s\r\na=ice-pwd:%s\r\n", a->ufrag, a->pwd);
        out_fp(&o, a->fp);
        outf(&o, "a=setup:passive\r\na=mid:%s\r\n", m->mid);
        if (first) {
            for (int c = 0; c < a->ncand; c++) {
                const RwLocalCand *lc = &a->cand[c];
                const char *typ = lc->type == RW_CAND_RELAY ? "relay" : lc->type == RW_CAND_SRFLX ? "srflx" : "host";
                outf(&o, "a=candidate:%d 1 %s %u %s %u typ %s", lc->type * 10 + c, lc->tcp ? "tcp" : "udp", lc->prio, lc->ip, lc->port, typ);
                if (lc->tcp) outf(&o, " tcptype passive");
                else if (lc->type != RW_CAND_HOST) outf(&o, " raddr %s rport %u", lc->raddr[0] ? lc->raddr : "0.0.0.0", lc->rport);
                outf(&o, "\r\n");
            }
            outf(&o, "a=end-of-candidates\r\n");
            first = 0;
        }
        if (m->kind == RW_KIND_APP) {
            outf(&o, "a=sctp-port:5000\r\na=max-message-size:262144\r\n");
            continue;
        }
        outf(&o, "a=rtcp-mux\r\n");
        if (i == of->vm) {
            const RwCodec *c = NULL;
            for (int q = 0; q < m->npt; q++) if (m->pt[q].pt == of->vpt) c = &m->pt[q];
            outf(&o, "a=sendonly\r\na=msid:lordos video\r\n");
            if (of->vcodec == RW_CODEC_H264) {
                outf(&o, "a=rtpmap:%d H264/90000\r\n", of->vpt);
                outf(&o, "a=fmtp:%d level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=%06x\r\n", of->vpt, c ? c->plid : 0x42e01fu);
            } else outf(&o, "a=rtpmap:%d VP8/90000\r\n", of->vpt);
            if (c && c->fb_nack) outf(&o, "a=rtcp-fb:%d nack\r\n", of->vpt);
            if (c && c->fb_pli) outf(&o, "a=rtcp-fb:%d nack pli\r\n", of->vpt);
            if (c && c->fb_fir) outf(&o, "a=rtcp-fb:%d ccm fir\r\n", of->vpt);
            outf(&o, "a=ssrc:%u cname:lordos\r\na=ssrc:%u msid:lordos video\r\n", a->vssrc, a->vssrc);
        } else {
            outf(&o, "%s\r\na=msid:lordos audio\r\n", a->audio ? "a=sendonly" : "a=inactive");
            outf(&o, "a=rtpmap:%d opus/48000/2\r\na=fmtp:%d minptime=10;useinbandfec=1\r\n", of->apt, of->apt);
            outf(&o, "a=ssrc:%u cname:lordos\r\na=ssrc:%u msid:lordos audio\r\n", a->assrc, a->assrc);
        }
    }
    return o.bad ? -1 : (int)o.n;
}

/* ---- STUN ---- */

int rw_is_stun(const uint8_t *p, size_t n) { return n >= 20 && p[0] < 4 && be32(p + 4) == 0x2112A442u; }

int rw_tcp_frame(const uint8_t *p, size_t n, size_t *len) {
    if (n < 2) return 0;
    size_t l = be16(p);
    if (!l || l > RW_TCP_FRAME_MAX) return -1;
    if (n < 2 + l) return 0;
    *len = l;
    return 1;
}

int rw_stun_stream(const uint8_t *p, size_t n, size_t *len) {
    if (n < 1) return 0;
    if (p[0] > 3) return -1;                                       /* ChannelData or garbage */
    if (n < 20) return 0;
    size_t l = be16(p + 2);
    if (l % 4 || 20 + l > RW_STUN_MAX || be32(p + 4) != 0x2112A442u) return -1;
    if (n < 20 + l) return 0;
    *len = 20 + l;
    return 1;
}

int rw_ice_route(const uint8_t *p, size_t n, char route[9]) {
    RwStun s;
    if (!rw_is_stun(p, n) || rw_stun_parse(p, n, &s) || s.type != RW_STUN_BINDING_REQ || !s.username || s.ulen < 10 || !s.mi_off) return -1;
    for (int i = 0; i < 8; i++) {
        char c = (char)s.username[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9'))) return -1;
        route[i] = c;
    }
    route[8] = 0;
    if (!memchr(s.username, ':', s.ulen)) return -1;
    return 0;
}

uint32_t rw_crc32(const uint8_t *p, size_t n) {
    uint32_t c = 0xffffffffu;
    for (size_t i = 0; i < n; i++) {
        c ^= p[i];
        for (int k = 0; k < 8; k++) c = (c >> 1) ^ (0xEDB88320u & (0u - (c & 1)));
    }
    return c ^ 0xffffffffu;
}

static int xaddr(const uint8_t *v, size_t l, const uint8_t *hdr, RwAddr *a) {
    if (l < 4 || v[0] != 0) return -1;
    if (v[1] == 1 && l == 8) {
        a->family = 4;
        memset(a->a, 0, 16);
        for (int i = 0; i < 4; i++) a->a[i] = v[4 + i] ^ hdr[4 + i];
    } else if (v[1] == 2 && l == 20) {
        a->family = 6;
        for (int i = 0; i < 16; i++) a->a[i] = v[4 + i] ^ hdr[4 + i];          /* cookie + transaction id */
    } else return -1;
    a->port = (uint16_t)(be16(v + 2) ^ 0x2112);
    return 0;
}

int rw_stun_parse(const uint8_t *p, size_t n, RwStun *s) {
    memset(s, 0, sizeof *s);
    if (n < 20 || n > RW_STUN_MAX || (p[0] & 0xc0) || be32(p + 4) != 0x2112A442u) return -1;
    size_t len = be16(p + 2);
    if ((len & 3) || 20 + len != n) return -1;
    s->type = be16(p);
    memcpy(s->tid, p + 8, 12);
    size_t off = 20;
    uint32_t seen = 0;
    while (off < n) {
        if (n - off < 4) return -1;
        if (s->has_fp) return -1;                                      /* nothing after FINGERPRINT */
        uint16_t t = be16(p + off), l = be16(p + off + 2);
        size_t padded = ((size_t)l + 3) & ~(size_t)3;
        if (padded > n - off - 4) return -1;
        const uint8_t *v = p + off + 4;
        if (s->mi_off && t != RW_ATTR_FINGERPRINT) return -1;          /* only FINGERPRINT after MESSAGE-INTEGRITY */
        int bit = -1;
        switch (t) {
        case RW_ATTR_USERNAME: bit = 0; if (!l || l > 513) return -1; s->username = v; s->ulen = l; break;
        case RW_ATTR_MI: bit = 1; if (l != 20) return -1; s->mi_off = off; s->mi = v; break;
        case RW_ATTR_FINGERPRINT:
            bit = 2;
            if (l != 4 || be32(v) != (rw_crc32(p, off) ^ 0x5354554eu)) return -1;
            /* the length in the header must already include it: it does (20 + len == n) */
            s->has_fp = 1;
            break;
        case RW_ATTR_PRIORITY: bit = 3; if (l != 4) return -1; s->priority = be32(v); break;
        case RW_ATTR_USE_CAND: bit = 4; if (l != 0) return -1; s->use_candidate = 1; break;
        case RW_ATTR_CONTROLLING: bit = 5; if (l != 8) return -1; s->controlling = 1; break;
        case RW_ATTR_CONTROLLED: bit = 6; if (l != 8) return -1; s->controlled = 1; break;
        case RW_ATTR_XOR_MAPPED: bit = 7; if (xaddr(v, l, p, &s->xor_mapped)) return -1; break;
        case RW_ATTR_XOR_RELAYED: bit = 8; if (xaddr(v, l, p, &s->xor_relayed)) return -1; break;
        case RW_ATTR_XOR_PEER: bit = 9; if (xaddr(v, l, p, &s->xor_peer)) return -1; break;
        case RW_ATTR_ERROR: {
            bit = 10;
            if (l < 4 || l > 763 + 4 || v[0] || v[1]) return -1;
            int cls = v[2] & 7, num = v[3];
            if (cls < 3 || cls > 6 || num > 99) return -1;
            s->error = cls * 100 + num;
            break;
        }
        case RW_ATTR_REALM: bit = 11; if (!l || l > 763) return -1; s->realm = v; s->rlen = l; break;
        case RW_ATTR_NONCE: bit = 12; if (!l || l > 763) return -1; s->nonce = v; s->nlen = l; break;
        case RW_ATTR_LIFETIME: bit = 13; if (l != 4) return -1; s->lifetime = be32(v); break;
        case RW_ATTR_DATA: bit = 14; s->data = v; s->dlen = l; break;
        case RW_ATTR_MAPPED: case RW_ATTR_SOFTWARE: case RW_ATTR_TRANSPORT: case RW_ATTR_CHANNEL: break;
        default:
            if (t < 0x8000) return -1;                                 /* unknown comprehension-required */
            break;
        }
        if (bit >= 0) { if (seen & (1u << bit)) return -1; seen |= 1u << bit; }
        off += 4 + padded;
    }
    return 0;
}

int rw_stun_check_mi(const uint8_t *p, size_t n, const RwStun *s, const uint8_t *key, size_t klen, RwHmac hmac) {
    if (!s->mi_off || s->mi_off + 24 > n || s->mi_off > RW_STUN_MAX) return 0;
    uint8_t buf[RW_STUN_MAX], mac[20];
    memcpy(buf, p, s->mi_off);
    put16(buf + 2, (uint16_t)(s->mi_off + 24 - 20));                  /* the length as if MI were last */
    hmac(key, klen, buf, s->mi_off, mac);
    unsigned d = 0;
    for (int i = 0; i < 20; i++) d |= (unsigned)(mac[i] ^ s->mi[i]);
    return d == 0;
}

void rw_so_init(RwStunOut *o, uint16_t type, const uint8_t tid[12]) {
    memset(o, 0, sizeof *o);
    put16(o->b, type);
    put32(o->b + 4, 0x2112A442u);
    memcpy(o->b + 8, tid, 12);
    o->n = 20;
}

void rw_so_attr(RwStunOut *o, uint16_t type, const void *v, size_t len) {
    size_t padded = (len + 3) & ~(size_t)3;
    if (o->bad || len > 0xffff || o->n + 4 + padded > sizeof o->b) { o->bad = 1; return; }
    put16(o->b + o->n, type);
    put16(o->b + o->n + 2, (uint16_t)len);
    if (len) memcpy(o->b + o->n + 4, v, len);
    memset(o->b + o->n + 4 + len, 0, padded - len);
    o->n += 4 + padded;
    put16(o->b + 2, (uint16_t)(o->n - 20));
}

void rw_so_u32(RwStunOut *o, uint16_t type, uint32_t v) {
    uint8_t b[4];
    put32(b, v);
    rw_so_attr(o, type, b, 4);
}

void rw_so_xaddr(RwStunOut *o, uint16_t type, const RwAddr *a) {
    uint8_t v[20] = { 0 };
    v[1] = a->family == 6 ? 2 : 1;
    put16(v + 2, (uint16_t)(a->port ^ 0x2112));
    int k = a->family == 6 ? 16 : 4;
    for (int i = 0; i < k; i++) v[4 + i] = a->a[i] ^ o->b[4 + i];
    rw_so_attr(o, type, v, (size_t)(4 + k));
}

void rw_so_mi(RwStunOut *o, const uint8_t *key, size_t klen, RwHmac hmac) {
    if (o->bad || o->n + 24 > sizeof o->b) { o->bad = 1; return; }
    put16(o->b + 2, (uint16_t)(o->n + 24 - 20));
    uint8_t mac[20];
    hmac(key, klen, o->b, o->n, mac);
    rw_so_attr(o, RW_ATTR_MI, mac, 20);
}

void rw_so_fingerprint(RwStunOut *o) {
    if (o->bad || o->n + 8 > sizeof o->b) { o->bad = 1; return; }
    put16(o->b + 2, (uint16_t)(o->n + 8 - 20));
    uint32_t c = rw_crc32(o->b, o->n) ^ 0x5354554eu;
    rw_so_u32(o, RW_ATTR_FINGERPRINT, c);
}

/* ---- RTCP ---- */

#define FB_MAX 64

int rw_rtcp_parse(const uint8_t *p, size_t n, RwFbOn on, void *ud) {
    RwFb ev[FB_MAX];
    int nev = 0;
    size_t off = 0;
    if (n < 4 || n > 1500) return -1;
    while (off < n) {
        if (n - off < 4) return -1;
        const uint8_t *h = p + off;
        if ((h[0] >> 6) != 2) return -1;
        int padding = (h[0] >> 5) & 1, count = h[0] & 31, pt = h[1];
        size_t len = ((size_t)be16(h + 2) + 1) * 4;
        if (len > n - off || pt < 192 || pt > 223) return -1;
        size_t body = len;
        if (padding) {
            if (off + len != n) return -1;                             /* only the last part may pad */
            size_t pad = h[len - 1];
            if (!pad || pad > len - 4) return -1;
            body = len - pad;
        }
#define ADD(...) do { if (nev < FB_MAX) ev[nev++] = (RwFb){ __VA_ARGS__ }; } while (0)
        if (pt == 200 || pt == 201) {                                  /* SR, RR: report blocks */
            size_t base = pt == 200 ? 28 : 8;
            if (body < base + (size_t)count * 24) return -1;
            for (int i = 0; i < count; i++) {
                const uint8_t *b = h + base + (size_t)i * 24;
                ADD(.kind = RW_FB_RR, .ssrc = be32(b), .lost_frac = b[4], .jitter = be32(b + 12), .lsr = be32(b + 16), .dlsr = be32(b + 20));
            }
        } else if (pt == 205) {                                        /* transport feedback */
            if (body < 12) return -1;
            if (count == 1) {                                          /* generic NACK */
                if ((body - 12) % 4 || body == 12) return -1;
                for (size_t q = 12; q < body; q += 4) ADD(.kind = RW_FB_NACK, .ssrc = be32(h + 8), .pid = be16(h + q), .blp = be16(h + q + 2));
            }
        } else if (pt == 206) {                                        /* payload-specific feedback */
            if (body < 12) return -1;
            if (count == 1) ADD(.kind = RW_FB_PLI, .ssrc = be32(h + 8));
            else if (count == 4) {                                     /* FIR: 8 bytes per entry */
                if ((body - 12) % 8 || body == 12) return -1;
                for (size_t q = 12; q < body; q += 8) ADD(.kind = RW_FB_FIR, .ssrc = be32(h + q));
            } else if (count == 15 && body >= 20 && !memcmp(h + 12, "REMB", 4)) {
                int ns = h[16];
                if (body < 20 + (size_t)ns * 4 || !ns) return -1;
                int exp = h[17] >> 2;
                uint64_t mant = (uint64_t)(h[17] & 3) << 16 | (uint64_t)h[18] << 8 | h[19];
                uint64_t br = exp > 46 ? UINT64_C(1) << 63 : mant << exp;
                for (int i = 0; i < ns; i++) ADD(.kind = RW_FB_REMB, .ssrc = be32(h + 20 + (size_t)i * 4), .bitrate = br);
            }
        } else if (pt == 202 || pt == 203 || pt == 204 || pt == 207) {
            /* SDES, BYE, APP, XR: nothing we use */
        } else return -1;
#undef ADD
        off += len;
    }
    for (int i = 0; i < nev; i++) on(ud, &ev[i]);
    return 0;
}

/* ---- data channel open ---- */

int rw_dcep_open(const uint8_t *p, size_t n, RwDcOpen *o) {
    memset(o, 0, sizeof *o);
    if (n < 12 || n > 12 + 64 || p[0] != 0x03) return -1;
    if (p[1] != 0x00 && p[1] != 0x80) return -1;
    size_t ll = be16(p + 8), pl = be16(p + 10);
    if (ll < 1 || ll > 64 || pl != 0 || 12 + ll != n) return -1;
    for (size_t i = 0; i < ll; i++) {
        uint8_t c = p[12 + i];
        if (!((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_' || c == '-')) return -1;
    }
    o->chan_type = p[1];
    o->prio = be16(p + 2);
    o->rel = be32(p + 4);
    memcpy(o->label, p + 12, ll);
    o->label[ll] = 0;
    return 0;
}

/* ---- RTP payloads ---- */

/* Next NAL unit in an Annex B stream from *pos: start and length, or 0 at the end. */
static int next_nal(const uint8_t *b, size_t n, size_t *pos, const uint8_t **nal, size_t *len) {
    size_t i = *pos;
    while (i + 3 <= n && !(b[i] == 0 && b[i + 1] == 0 && b[i + 2] == 1)) i++;
    if (i + 3 > n) return 0;
    size_t s = i + 3, j = s;
    while (j + 3 <= n && !(b[j] == 0 && b[j + 1] == 0 && (b[j + 2] == 1 || (b[j + 2] == 0 && j + 3 < n && b[j + 3] == 1)))) j++;
    if (j + 3 > n) j = n;
    size_t e = j;
    while (e > s && b[e - 1] == 0 && e < n) e--;                       /* zero bytes before the next start code */
    *nal = b + s;
    *len = e - s;
    *pos = j;
    return 1;
}

int rw_h264_is_idr(const uint8_t *au, size_t n) {
    size_t pos = 0;
    const uint8_t *nal;
    size_t l;
    while (next_nal(au, n, &pos, &nal, &l)) if (l && (nal[0] & 0x1f) == 5) return 1;
    return 0;
}

int rw_h264_packetize(const uint8_t *au, size_t n, size_t mtu, RwPktOut out, void *ud) {
    if (mtu < 64) return -1;
    const uint8_t *nals[256];
    size_t lens[256];
    int k = 0;
    size_t pos = 0;
    const uint8_t *nal;
    size_t l;
    while (next_nal(au, n, &pos, &nal, &l)) {
        if (!l) continue;
        if (k >= 256 || (nal[0] & 0x80)) return -1;                    /* forbidden_zero_bit */
        nals[k] = nal;
        lens[k] = l;
        k++;
    }
    if (!k) return -1;
    int pk = 0;
    for (int i = 0; i < k; i++) {
        int last = i == k - 1;
        if (lens[i] <= mtu) { out(ud, NULL, 0, nals[i], lens[i], last); pk++; continue; }
        uint8_t fu[2] = { (uint8_t)((nals[i][0] & 0x60) | 28), (uint8_t)(nals[i][0] & 0x1f) };
        const uint8_t *d = nals[i] + 1;
        size_t rem = lens[i] - 1, chunk = mtu - 2;
        int start = 1;
        while (rem) {
            size_t c = rem > chunk ? chunk : rem;
            uint8_t h[2] = { fu[0], (uint8_t)(fu[1] | (start ? 0x80 : 0) | (c == rem ? 0x40 : 0)) };
            out(ud, h, 2, d, c, last && c == rem);
            pk++;
            d += c;
            rem -= c;
            start = 0;
        }
    }
    return pk;
}

int rw_vp8_packetize(const uint8_t *f, size_t n, size_t mtu, RwPktOut out, void *ud) {
    if (mtu < 64 || !n) return -1;
    size_t chunk = mtu - 1;
    int pk = 0;
    for (size_t off = 0; off < n; off += chunk) {
        size_t c = n - off > chunk ? chunk : n - off;
        uint8_t d = off == 0 ? 0x10 : 0x00;
        out(ud, &d, 1, f + off, c, off + c == n);
        pk++;
    }
    return pk;
}
