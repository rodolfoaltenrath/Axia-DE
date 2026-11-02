/* vimport.c — import and export (see vimport.h): format detection, the CSVs of KeePassXC,
 * Bitwarden, Chrome, Firefox, the generic password-manager CSV and our own, the Bitwarden JSON,
 * and the export CSV. KDBX lives in kdbx.c. Everything in the secrets arena. */
#include "vimport.h"
#include "csvp.h"
#include "kdbx.h"
#include "smem.h"
#include "vjson.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

#define MAX_INPUT (256u * 1024 * 1024)
#define MAX_ENTRIES 1000000

static int fail(ImpList *l, const char *code) { snprintf(l->err, sizeof l->err, "%s", code); return -1; }

char *imp_strdup(const char *s) { return s ? sm_strdup(s) : NULL; }

ImpEntry *imp_add(ImpList *l) {
    if (l->n >= MAX_ENTRIES) return NULL;
    if (l->n == l->cap) {
        int nc = l->cap ? l->cap * 2 : 64;
        ImpEntry *v = sm_realloc(l->v, sizeof(ImpEntry) * (size_t)nc);
        if (!v) return NULL;
        l->v = v;
        l->cap = nc;
    }
    ImpEntry *e = &l->v[l->n++];
    memset(e, 0, sizeof *e);
    return e;
}

static void free_strs(char **a, int n) { for (int i = 0; i < n; i++) sm_free(a[i]); sm_free(a); }

static void free_entry(ImpEntry *e) {
    sm_free(e->title); sm_free(e->username); sm_free(e->password); sm_free(e->url); sm_free(e->notes);
    sm_free(e->otp); sm_free(e->group); sm_free(e->kind);
    free_strs(e->tags, e->ntags);
    free_strs(e->urls, e->nurls);
    for (int k = 0; k < e->nfields; k++) { sm_free(e->fields[k].key); sm_free(e->fields[k].value); }
    sm_free(e->fields);
    for (int k = 0; k < e->natts; k++) { sm_free(e->atts[k].name); sm_free(e->atts[k].data); }
    sm_free(e->atts);
    memset(e, 0, sizeof *e);
}

/* removes the last entry (a row that is not imported) */
static void drop_last(ImpList *l) { if (l->n) free_entry(&l->v[--l->n]); }

void imp_free(ImpList *l) {
    for (int i = 0; i < l->n; i++) free_entry(&l->v[i]);
    sm_free(l->v);
    free_strs(l->groups, l->ngroups);
    char err[sizeof l->err];
    memcpy(err, l->err, sizeof err);
    memset(l, 0, sizeof *l);
    memcpy(l->err, err, sizeof err);              /* the error code survives imp_free */
}

/* ---- utilities ---- */

static int64_t days_from_civil(int64_t y, unsigned m, unsigned d) {
    y -= m <= 2;
    int64_t era = (y >= 0 ? y : y - 399) / 400;
    unsigned yoe = (unsigned)(y - era * 400);
    unsigned doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
    unsigned doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + (int64_t)doe - 719468;
}

int64_t imp_parse_iso(const char *s) {
    if (!s) return 0;
    while (*s == ' ') s++;
    int y, mo, d, h = 0, mi = 0, se = 0, n = 0;
    if (sscanf(s, "%4d-%2d-%2d%n", &y, &mo, &d, &n) != 3) return 0;
    if (s[n] == 'T' || s[n] == ' ') {
        int k = 0;
        if (sscanf(s + n + 1, "%2d:%2d%n", &h, &mi, &k) == 2) {
            n += 1 + k;
            if (s[n] == ':') sscanf(s + n + 1, "%2d", &se);
        }
    }
    if (y < 1970 || y > 9999 || mo < 1 || mo > 12 || d < 1 || d > 31 || h > 23 || mi > 59 || se > 60) return 0;
    return days_from_civil(y, (unsigned)mo, (unsigned)d) * 86400 + h * 3600 + mi * 60 + se;
}

static int set(char **dst, const char *v) {
    sm_free(*dst);
    *dst = NULL;
    if (!v || !*v) return 0;
    *dst = sm_strdup(v);
    return *dst ? 0 : -1;
}

static int add_str(char ***arr, int *n, const char *s) {
    if (*n >= 10000) return 0;
    char **a = sm_realloc(*arr, sizeof(char *) * (size_t)(*n + 1));
    if (!a) return -1;
    *arr = a;
    if (!(a[*n] = sm_strdup(s))) return -1;
    (*n)++;
    return 0;
}

static int add_field(ImpEntry *e, const char *k, const char *v, int type) {
    if (!v || !*v) return 0;
    if (e->nfields >= 10000) return 0;
    ImpField *f = sm_realloc(e->fields, sizeof(ImpField) * (size_t)(e->nfields + 1));
    if (!f) return -1;
    e->fields = f;
    f[e->nfields].key = sm_strdup(k && *k ? k : "?");
    f[e->nfields].value = sm_strdup(v);
    f[e->nfields].type = type;
    if (!f[e->nfields].key || !f[e->nfields].value) { sm_free(f[e->nfields].key); sm_free(f[e->nfields].value); return -1; }
    e->nfields++;
    return 0;
}

/* splits on any of seps, trims spaces, skips empties */
static int split_add(char ***arr, int *n, const char *s, const char *seps) {
    while (s && *s) {
        while (*s && (strchr(seps, *s) || *s == ' ')) s++;
        const char *a = s;
        while (*s && !strchr(seps, *s)) s++;
        const char *b = s;
        while (b > a && (b[-1] == ' ' || b[-1] == '\t' || b[-1] == '\r')) b--;
        if (b > a) {
            char *t = sm_strndup(a, (size_t)(b - a));
            if (!t) return -1;
            int r = add_str(arr, n, t);
            sm_free(t);
            if (r) return -1;
        }
    }
    return 0;
}

/* "https://www.example.com:8080/x" → "example.com" (title for the browser CSVs) */
static char *host_of(const char *url) {
    if (!url || !*url) return NULL;
    const char *p = strstr(url, "://");
    p = p ? p + 3 : url;
    const char *at = NULL;
    for (const char *q = p; *q && *q != '/' && *q != '?' && *q != '#'; q++) if (*q == '@') at = q;
    if (at) p = at + 1;
    if (!strncasecmp(p, "www.", 4)) p += 4;
    size_t n = 0;
    while (p[n] && p[n] != '/' && p[n] != ':' && p[n] != '?' && p[n] != '#') n++;
    return n ? sm_strndup(p, n) : sm_strdup(url);
}

static int istrue(const char *s) { return s && (!strcmp(s, "1") || !strcasecmp(s, "true") || !strcasecmp(s, "yes")); }

/* ---- detection ---- */

static int csv_kind(const CsvRow *h) {
#define HAS(x) (csv_col(h, x) >= 0)
    if (HAS("category") && HAS("title") && HAS("username") && HAS("password") && HAS("fields")) return IMP_LORD_CSV;
    if (HAS("group") && HAS("title") && HAS("username") && HAS("password") && (HAS("totp") || HAS("last modified"))) return IMP_KEEPASSXC_CSV;
    if ((HAS("folder") || HAS("collections")) && HAS("login_uri") && HAS("login_password")) return IMP_BITWARDEN_CSV;
    if (HAS("httprealm") && HAS("formactionorigin") && HAS("password")) return IMP_FIREFOX_CSV;
    if (HAS("otpauth") || (HAS("title") && HAS("url") && HAS("username") && HAS("password"))) return IMP_APPLE_CSV;
    if (HAS("name") && HAS("url") && HAS("username") && HAS("password")) return IMP_CHROME_CSV;
#undef HAS
    return IMP_UNKNOWN;
}

int imp_detect(const uint8_t *data, size_t len, const char *name_hint) {
    (void)name_hint;
    if (!data || !len) return IMP_UNKNOWN;
    if (kdbx_is(data, len)) return IMP_KDBX;
    size_t p = len >= 3 && !memcmp(data, "\xef\xbb\xbf", 3) ? 3 : 0;
    while (p < len && (data[p] == ' ' || data[p] == '\n' || data[p] == '\r' || data[p] == '\t')) p++;
    if (p < len && data[p] == '{') {
        size_t lim = len < 1 << 20 ? len : 1 << 20;
        if (memmem(data, lim, "\"items\"", 7) || memmem(data, lim, "\"encrypted\"", 11)) return IMP_BITWARDEN_JSON;
        return IMP_UNKNOWN;
    }
    Csv c;
    CsvRow h = { 0 };
    csv_init(&c, (const char *)data, len);
    int k = csv_next(&c, &h) == 1 ? csv_kind(&h) : IMP_UNKNOWN;
    csv_row_free(&h);
    return k;
}

/* ---- CSV ---- */

typedef struct { int group, title, user, pass, url, notes, totp, created, modified, tags, fav, fields, urls, kind, type, folder, name, note2; } Cols;

static int parse_csv(int kind, const uint8_t *data, size_t len, ImpList *l) {
    Csv c;
    CsvRow h = { 0 }, r = { 0 };
    csv_init(&c, (const char *)data, len);
    if (csv_next(&c, &h) != 1) { csv_row_free(&h); return fail(l, c.bad ? "corrupt" : "empty"); }
    if (csv_kind(&h) == IMP_UNKNOWN) { csv_row_free(&h); return fail(l, "corrupt"); }
    Cols k;
    memset(&k, -1, sizeof k);
    switch (kind) {
    case IMP_KEEPASSXC_CSV:
        k.group = csv_col(&h, "group"); k.title = csv_col(&h, "title"); k.user = csv_col(&h, "username");
        k.pass = csv_col(&h, "password"); k.url = csv_col(&h, "url"); k.notes = csv_col(&h, "notes");
        k.totp = csv_col(&h, "totp"); k.modified = csv_col(&h, "last modified"); k.created = csv_col(&h, "created");
        break;
    case IMP_BITWARDEN_CSV:
        k.folder = csv_col(&h, "folder"); if (k.folder < 0) k.folder = csv_col(&h, "collections");
        k.fav = csv_col(&h, "favorite"); k.type = csv_col(&h, "type"); k.title = csv_col(&h, "name");
        k.notes = csv_col(&h, "notes"); k.fields = csv_col(&h, "fields"); k.url = csv_col(&h, "login_uri");
        k.user = csv_col(&h, "login_username"); k.pass = csv_col(&h, "login_password"); k.totp = csv_col(&h, "login_totp");
        break;
    case IMP_CHROME_CSV:
        k.title = csv_col(&h, "name"); k.url = csv_col(&h, "url"); k.user = csv_col(&h, "username");
        k.pass = csv_col(&h, "password"); k.notes = csv_col(&h, "note"); if (k.notes < 0) k.notes = csv_col(&h, "notes");
        break;
    case IMP_FIREFOX_CSV:
        k.url = csv_col(&h, "url"); k.user = csv_col(&h, "username"); k.pass = csv_col(&h, "password");
        k.created = csv_col(&h, "timecreated"); k.modified = csv_col(&h, "timepasswordchanged");
        break;
    case IMP_APPLE_CSV:
        k.title = csv_col(&h, "title"); k.url = csv_col(&h, "url"); k.user = csv_col(&h, "username");
        k.pass = csv_col(&h, "password"); k.notes = csv_col(&h, "notes"); k.totp = csv_col(&h, "otpauth");
        break;
    case IMP_LORD_CSV:
        k.group = csv_col(&h, "category"); k.title = csv_col(&h, "title"); k.user = csv_col(&h, "username");
        k.pass = csv_col(&h, "password"); k.url = csv_col(&h, "url"); k.notes = csv_col(&h, "notes");
        k.totp = csv_col(&h, "totp"); k.tags = csv_col(&h, "tags"); k.fav = csv_col(&h, "favorite");
        k.fields = csv_col(&h, "fields"); k.urls = csv_col(&h, "urls"); k.kind = csv_col(&h, "kind");
        k.created = csv_col(&h, "created"); k.modified = csv_col(&h, "modified");
        break;
    default:
        csv_row_free(&h);
        return fail(l, "corrupt");
    }
    int rc = 0, nr;
    while ((nr = csv_next(&c, &r)) == 1) {
        /* a row with everything empty is not an entry */
        int any = 0;
        for (int i = 0; i < r.n; i++) if (r.f[i][0]) any = 1;
        if (!any) continue;
        ImpEntry *e = imp_add(l);
        if (!e) { rc = fail(l, l->n >= MAX_ENTRIES ? "too-big" : "memory"); break; }
        int bad = 0;
        bad |= set(&e->title, csv_get(&r, k.title));
        bad |= set(&e->username, csv_get(&r, k.user));
        bad |= set(&e->password, csv_get(&r, k.pass));
        bad |= set(&e->notes, csv_get(&r, k.notes));
        bad |= set(&e->otp, csv_get(&r, k.totp));
        if (kind == IMP_BITWARDEN_CSV) {
            /* several URIs separated by commas; the fields are "name: value" lines */
            char **u = NULL;
            int nu = 0;
            bad |= split_add(&u, &nu, csv_get(&r, k.url), ",\n");
            if (nu) { bad |= set(&e->url, u[0]); for (int i = 1; i < nu; i++) bad |= add_str(&e->urls, &e->nurls, u[i]); }
            free_strs(u, nu);
            const char *f = csv_get(&r, k.fields);
            while (*f) {
                const char *eol = strchr(f, '\n');
                size_t n = eol ? (size_t)(eol - f) : strlen(f);
                char *line = sm_strndup(f, n);
                if (!line) { bad = 1; break; }
                if (n && line[n - 1] == '\r') line[n - 1] = 0;
                char *colon = strstr(line, ": ");
                if (colon) { *colon = 0; bad |= add_field(e, line, colon + 2, IMPF_TEXT); }
                else if (*line) bad |= add_field(e, line, "", IMPF_TEXT);
                sm_free(line);
                f += n + (eol ? 1 : 0);
            }
            bad |= set(&e->group, csv_get(&r, k.folder));
            e->favorite = istrue(csv_get(&r, k.fav));
            const char *type = csv_get(&r, k.type);
            bad |= set(&e->kind, !strcasecmp(type, "note") ? "note" : !strcasecmp(type, "card") ? "card" : !strcasecmp(type, "identity") ? "identity" : "login");
        } else {
            bad |= set(&e->url, csv_get(&r, k.url));
        }
        if (kind == IMP_KEEPASSXC_CSV) {
            /* "Root/Internet/Banks": the first component is the root folder of the database */
            const char *g = csv_get(&r, k.group);
            const char *slash = strchr(g, '/');
            g = slash ? slash + 1 : "";
            if (!strcmp(g, "Recycle Bin") || !strncmp(g, "Recycle Bin/", 12) || !strcmp(g, "Papelera") || !strncmp(g, "Papelera/", 9)) {
                drop_last(l);                     /* deleted items are not imported */
                continue;
            }
            bad |= set(&e->group, g);
            e->created = imp_parse_iso(csv_get(&r, k.created));
            e->modified = imp_parse_iso(csv_get(&r, k.modified));
        }
        if (kind == IMP_FIREFOX_CSV) {
            e->created = (int64_t)(strtod(csv_get(&r, k.created), NULL) / 1000);
            e->modified = (int64_t)(strtod(csv_get(&r, k.modified), NULL) / 1000);
            if (e->created < 0) e->created = 0;
            if (e->modified < 0) e->modified = 0;
            /* Firefox "Never save" entries come without a password: they are imported anyway */
        }
        if (kind == IMP_LORD_CSV) {
            bad |= set(&e->group, csv_get(&r, k.group));
            bad |= split_add(&e->tags, &e->ntags, csv_get(&r, k.tags), ";");
            e->favorite = istrue(csv_get(&r, k.fav));
            bad |= split_add(&e->urls, &e->nurls, csv_get(&r, k.urls), "\n");
            bad |= set(&e->kind, csv_get(&r, k.kind));
            e->created = strtoll(csv_get(&r, k.created), NULL, 10);
            e->modified = strtoll(csv_get(&r, k.modified), NULL, 10);
            const char *fj = csv_get(&r, k.fields);
            if (*fj) {
                Vj j;
                if (vj_parse(&j, fj, strlen(fj)) == 0) {
                    for (int i = 0; vj_type(&j, 0) == VJ_ARR && i < j.t[0].count; i++) {
                        int o = vj_at(&j, 0, i);
                        char *fk = vj_gets(&j, o, "k"), *fv = vj_gets(&j, o, "v");
                        int ft = (int)vj_getn(&j, o, "t", 0);
                        if (ft < IMPF_TEXT || ft > IMPF_TOTP) ft = IMPF_TEXT;
                        if (fk && fv) bad |= add_field(e, fk, fv, ft);
                        sm_free(fk); sm_free(fv);
                    }
                    vj_free(&j);
                }                                 /* unreadable fields: ignored */
            }
        }
        if (kind == IMP_CHROME_CSV || kind == IMP_FIREFOX_CSV) {
            if (!e->title) { e->title = host_of(e->url); }
            bad |= set(&e->kind, "login");
        }
        if (kind == IMP_APPLE_CSV || kind == IMP_KEEPASSXC_CSV) bad |= set(&e->kind, "login");
                if (bad) { rc = fail(l, "memory"); break; }
    }
    if (nr < 0 && !rc) rc = fail(l, "corrupt");
    csv_row_free(&h);
    csv_row_free(&r);
    if (!rc && !l->n) rc = fail(l, "empty");
    return rc;
}

/* ---- Bitwarden JSON ---- */

typedef struct { char id[64]; char *name; } Folder;

static int bw_folders(const Vj *j, const char *key, Folder **out, int *n) {
    int arr = vj_get(j, 0, key);
    if (vj_type(j, arr) != VJ_ARR) return 0;
    int cnt = j->t[arr].count;
    if (!cnt) return 0;
    if (cnt > 100000) return -1;
    Folder *f = sm_alloc(sizeof(Folder) * (size_t)cnt);
    if (!f) return -1;
    for (int i = 0; i < cnt; i++) {
        int o = vj_at(j, arr, i);
        vj_getsbuf(j, o, "id", f[i].id, sizeof f[i].id);
        f[i].name = vj_gets(j, o, "name");
    }
    *out = f;
    *n = cnt;
    return 0;
}

static const char *folder_name(const Folder *f, int n, const char *id) {
    for (int i = 0; id && *id && i < n; i++) if (!strcmp(f[i].id, id)) return f[i].name;
    return NULL;
}

/* field of a Bitwarden subobject (card/identity/sshKey) as an ImpField */
static int bw_sub(ImpEntry *e, const Vj *j, int o, const char *bw, const char *key, int type) {
    char *v = vj_gets(j, o, bw);
    int r = add_field(e, key, v, type);
    sm_free(v);
    return r;
}

static int parse_bitwarden(const uint8_t *data, size_t len, ImpList *l) {
    Vj j;
    if (vj_parse(&j, (const char *)data, len)) return fail(l, "corrupt");
    if (vj_type(&j, 0) != VJ_OBJ) { vj_free(&j); return fail(l, "corrupt"); }
    if (vj_getb(&j, 0, "encrypted", 0)) { vj_free(&j); return fail(l, "encrypted-export"); }
    int items = vj_get(&j, 0, "items");
    if (vj_type(&j, items) != VJ_ARR) { vj_free(&j); return fail(l, "corrupt"); }
    Folder *fol = NULL, *col = NULL;
    int nfol = 0, ncol = 0, rc = 0;
    if (bw_folders(&j, "folders", &fol, &nfol) || bw_folders(&j, "collections", &col, &ncol)) { rc = fail(l, "memory"); goto out; }
    for (int i = 0; i < nfol; i++) if (fol[i].name && add_str(&l->groups, &l->ngroups, fol[i].name)) { rc = fail(l, "memory"); goto out; }
    for (int i = 0; i < ncol; i++) if (col[i].name && add_str(&l->groups, &l->ngroups, col[i].name)) { rc = fail(l, "memory"); goto out; }
    for (int i = 0; i < j.t[items].count && !rc; i++) {
        int it = vj_at(&j, items, i);
        if (vj_type(&j, it) != VJ_OBJ) continue;
        ImpEntry *e = imp_add(l);
        if (!e) { rc = fail(l, l->n >= MAX_ENTRIES ? "too-big" : "memory"); break; }
        int bad = 0;
        e->title = vj_gets(&j, it, "name");
        e->notes = vj_gets(&j, it, "notes");
        e->favorite = vj_getb(&j, it, "favorite", 0);
        char tb[40];
        vj_getsbuf(&j, it, "creationDate", tb, sizeof tb);
        e->created = imp_parse_iso(tb);
        vj_getsbuf(&j, it, "revisionDate", tb, sizeof tb);
        e->modified = imp_parse_iso(tb);
        char fid[64];
        vj_getsbuf(&j, it, "folderId", fid, sizeof fid);
        const char *g = folder_name(fol, nfol, fid);
        if (!g) {
            int cids = vj_get(&j, it, "collectionIds");
            char cid[64] = "";
            if (vj_type(&j, cids) == VJ_ARR && j.t[cids].count) vj_strbuf(&j, vj_at(&j, cids, 0), cid, sizeof cid);
            g = folder_name(col, ncol, cid);
        }
        if (g) bad |= set(&e->group, g);
        int type = (int)vj_getn(&j, it, "type", 1);
        const char *kind = type == 2 ? "note" : type == 3 ? "card" : type == 4 ? "identity" : type == 5 ? "ssh" : "login";
        bad |= set(&e->kind, kind);
        int lo = vj_get(&j, it, "login");
        if (vj_type(&j, lo) == VJ_OBJ) {
            e->username = vj_gets(&j, lo, "username");
            e->password = vj_gets(&j, lo, "password");
            e->otp = vj_gets(&j, lo, "totp");
            int uris = vj_get(&j, lo, "uris");
            if (vj_type(&j, uris) == VJ_ARR) {
                for (int u = 0; u < j.t[uris].count; u++) {
                    char *uri = vj_gets(&j, vj_at(&j, uris, u), "uri");
                    if (uri && *uri) { if (!e->url) { e->url = uri; uri = NULL; } else bad |= add_str(&e->urls, &e->nurls, uri); }
                    sm_free(uri);
                }
            }
        }
        int card = vj_get(&j, it, "card");
        if (vj_type(&j, card) == VJ_OBJ) {
            bad |= bw_sub(e, &j, card, "cardholderName", "holder", IMPF_TEXT);
            bad |= bw_sub(e, &j, card, "brand", "brand", IMPF_TEXT);
            bad |= bw_sub(e, &j, card, "number", "number", IMPF_HIDDEN);
            char mm[8] = "", yy[8] = "";
            vj_getsbuf(&j, card, "expMonth", mm, sizeof mm);
            vj_getsbuf(&j, card, "expYear", yy, sizeof yy);
            if (*mm || *yy) {
                char ex[24];
                snprintf(ex, sizeof ex, "%s%s/%s", strlen(mm) == 1 ? "0" : "", mm, yy);
                bad |= add_field(e, "expiry", ex, IMPF_TEXT);
            }
            bad |= bw_sub(e, &j, card, "code", "cvv", IMPF_HIDDEN);
        }
        int id = vj_get(&j, it, "identity");
        if (vj_type(&j, id) == VJ_OBJ) {
            static const char *const keys[] = { "title", "firstName", "middleName", "lastName", "address1", "address2", "address3", "city",
                                                "state", "postalCode", "country", "company", "email", "phone", "username" };
            for (size_t k = 0; k < sizeof keys / sizeof *keys; k++) bad |= bw_sub(e, &j, id, keys[k], keys[k], IMPF_TEXT);
            bad |= bw_sub(e, &j, id, "ssn", "ssn", IMPF_HIDDEN);
            bad |= bw_sub(e, &j, id, "passportNumber", "passportNumber", IMPF_HIDDEN);
            bad |= bw_sub(e, &j, id, "licenseNumber", "licenseNumber", IMPF_HIDDEN);
        }
        int ssh = vj_get(&j, it, "sshKey");
        if (vj_type(&j, ssh) == VJ_OBJ) {
            bad |= bw_sub(e, &j, ssh, "privateKey", "privateKey", IMPF_HIDDEN);
            bad |= bw_sub(e, &j, ssh, "publicKey", "publicKey", IMPF_TEXT);
            bad |= bw_sub(e, &j, ssh, "keyFingerprint", "keyFingerprint", IMPF_TEXT);
        }
        int fields = vj_get(&j, it, "fields");
        if (vj_type(&j, fields) == VJ_ARR) {
            for (int f = 0; f < j.t[fields].count; f++) {
                int fo = vj_at(&j, fields, f);
                int ft = (int)vj_getn(&j, fo, "type", 0);
                if (ft == 3) continue;                         /* "linked": points to another field */
                char *fn = vj_gets(&j, fo, "name"), *fv = vj_gets(&j, fo, "value");
                if (!fv) { int vt = vj_get(&j, fo, "value"); if (vj_type(&j, vt) == VJ_TRUE) fv = sm_strdup("true"); else if (vj_type(&j, vt) == VJ_FALSE) fv = sm_strdup("false"); }
                bad |= add_field(e, fn, fv, ft == 1 ? IMPF_HIDDEN : IMPF_TEXT);
                sm_free(fn); sm_free(fv);
            }
        }
        /* empties → NULL, as in the CSVs */
        char **strs[] = { &e->title, &e->notes, &e->username, &e->password, &e->otp };
        for (size_t k = 0; k < sizeof strs / sizeof *strs; k++) if (*strs[k] && !**strs[k]) { sm_free(*strs[k]); *strs[k] = NULL; }
        if (bad) rc = fail(l, "memory");
    }
    if (!rc && !l->n) rc = fail(l, "empty");
out:
    for (int i = 0; i < nfol; i++) sm_free(fol[i].name);
    for (int i = 0; i < ncol; i++) sm_free(col[i].name);
    sm_free(fol);
    sm_free(col);
    vj_free(&j);
    return rc;
}

/* ---- entry point ---- */

int imp_parse(int kind, const uint8_t *data, size_t len, const char *password,
              const uint8_t *keyfile, size_t kflen, ImpList *l) {
    memset(l, 0, sizeof *l);
    if (!data || !len) return fail(l, "empty");
    if (len > MAX_INPUT) return fail(l, "too-big");
    if (kind == IMP_UNKNOWN) kind = imp_detect(data, len, NULL);
    int rc;
    switch (kind) {
    case IMP_KDBX: rc = kdbx_parse(data, len, password, keyfile, kflen, l); break;
    case IMP_BITWARDEN_JSON: rc = parse_bitwarden(data, len, l); break;
    case IMP_KEEPASSXC_CSV: case IMP_BITWARDEN_CSV: case IMP_CHROME_CSV: case IMP_FIREFOX_CSV:
    case IMP_APPLE_CSV: case IMP_LORD_CSV:
        rc = parse_csv(kind, data, len, l);
        break;
    default:
        return fail(l, "unknown-format");
    }
    if (rc) imp_free(l);                          /* nothing half-done; l->err stays */
    return rc;
}

/* ---- export ---- */

static void csv_field(VjB *b, const char *s, int last) {
    if (!s) s = "";
    if (strpbrk(s, ",\"\r\n") || (*s && (s[0] == ' ' || s[strlen(s) - 1] == ' '))) {
        vb_raw(b, "\"");
        for (const char *p = s; *p; p++) { if (*p == '"') vb_raw(b, "\"\""); else vb_rawn(b, p, 1); }
        vb_raw(b, "\"");
    } else vb_raw(b, s);
    vb_raw(b, last ? "\r\n" : ",");
}

int exp_csv(const ImpList *l, char **out, size_t *outlen) {
    VjB b, t;
    vb_init(&b);
    vb_raw(&b, "category,title,username,password,url,notes,totp,tags,favorite,fields,urls,kind,created,modified\r\n");
    for (int i = 0; i < l->n; i++) {
        const ImpEntry *e = &l->v[i];
        csv_field(&b, e->group, 0);
        csv_field(&b, e->title, 0);
        csv_field(&b, e->username, 0);
        csv_field(&b, e->password, 0);
        csv_field(&b, e->url, 0);
        csv_field(&b, e->notes, 0);
        csv_field(&b, e->otp, 0);
        vb_init(&t);
        for (int k = 0; k < e->ntags; k++) { if (k) vb_raw(&t, ";"); vb_raw(&t, e->tags[k]); }
        csv_field(&b, t.p, 0);
        vb_free(&t);
        csv_field(&b, e->favorite ? "1" : "0", 0);
        vb_init(&t);
        if (e->nfields) {
            vb_open(&t, '[');
            for (int k = 0; k < e->nfields; k++) {
                vb_sep(&t);
                vb_open(&t, '{');
                vb_ks(&t, "k", e->fields[k].key);
                vb_ki(&t, "t", e->fields[k].type);
                vb_ks(&t, "v", e->fields[k].value);
                vb_close(&t, '}');
            }
            vb_close(&t, ']');
        }
        if (t.oom) b.oom = 1;
        csv_field(&b, t.p, 0);
        vb_free(&t);
        vb_init(&t);
        for (int k = 0; k < e->nurls; k++) { if (k) vb_raw(&t, "\n"); vb_raw(&t, e->urls[k]); }
        csv_field(&b, t.p, 0);
        vb_free(&t);
        csv_field(&b, e->kind, 0);
        char num[24];
        snprintf(num, sizeof num, "%lld", (long long)e->created);
        csv_field(&b, num, 0);
        snprintf(num, sizeof num, "%lld", (long long)e->modified);
        csv_field(&b, num, 1);
    }
    if (b.oom) { vb_free(&b); return -1; }
    *out = b.p;
    *outlen = b.n;
    return 0;
}
