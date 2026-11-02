/* html.c — see html.h. Lenient tokenizer + tree building in a single pass. */
#include "html.h"
#include "html_entities.h"
#include "../doc/wire.h"

#include <ctype.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

/* ---- text: entities and UTF-8 ---- */

static int put_cp(char *o, uint32_t cp) {
    if (cp < 0x80) { o[0] = (char)cp; return 1; }
    if (cp < 0x800) { o[0] = (char)(0xc0 | (cp >> 6)); o[1] = (char)(0x80 | (cp & 0x3f)); return 2; }
    if (cp < 0x10000) { o[0] = (char)(0xe0 | (cp >> 12)); o[1] = (char)(0x80 | ((cp >> 6) & 0x3f)); o[2] = (char)(0x80 | (cp & 0x3f)); return 3; }
    o[0] = (char)(0xf0 | (cp >> 18)); o[1] = (char)(0x80 | ((cp >> 12) & 0x3f)); o[2] = (char)(0x80 | ((cp >> 6) & 0x3f)); o[3] = (char)(0x80 | (cp & 0x3f));
    return 4;
}

/* windows-1252 for numeric references 128-159 (as browsers do) */
static const uint16_t c1fix[32] = { 0x20ac, 0xfffd, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039, 0x0152, 0xfffd, 0x017d, 0xfffd,
                                    0xfffd, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0xfffd, 0x017e, 0x0178 };

static uint32_t ent_lookup(const char *name, size_t n) {
    char key[40];
    if (n == 0 || n >= sizeof key) return 0;
    memcpy(key, name, n);
    key[n] = 0;
    size_t lo = 0, hi = sizeof html_ents / sizeof html_ents[0];
    while (lo < hi) {
        size_t mid = (lo + hi) / 2;
        int c = strcmp(html_ents[mid].name, key);
        if (!c) return html_ents[mid].cp;
        if (c < 0) lo = mid + 1; else hi = mid;
    }
    return 0;
}

/* Decodes an entity at s[i] == '&'. Returns the code point (0 if not an entity) and stores in *used
 * the bytes consumed. */
static uint32_t entity(const char *s, size_t len, size_t i, size_t *used) {
    *used = 0;
    if (i + 1 >= len) return 0;
    if (s[i + 1] == '#') {
        size_t k = i + 2;
        int hex = 0;
        if (k < len && (s[k] == 'x' || s[k] == 'X')) { hex = 1; k++; }
        uint32_t cp = 0;
        size_t d0 = k;
        while (k < len && k - d0 < 8) {
            char c = s[k];
            int v = (c >= '0' && c <= '9') ? c - '0' : (hex && c >= 'a' && c <= 'f') ? c - 'a' + 10 : (hex && c >= 'A' && c <= 'F') ? c - 'A' + 10 : -1;
            if (v < 0) break;
            cp = cp * (hex ? 16u : 10u) + (uint32_t)v;
            k++;
        }
        if (k == d0) return 0;
        if (k < len && s[k] == ';') k++;
        *used = k - i;
        if (cp >= 0x80 && cp <= 0x9f) cp = c1fix[cp - 0x80];
        if (!cp || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) cp = 0xfffd;
        return cp;
    }
    size_t k = i + 1;
    while (k < len && k - i <= 32 && isalnum((unsigned char)s[k])) k++;
    size_t n = k - i - 1;
    /* longest matching name (legacy ones without ";", like &nbsp without a semicolon) */
    for (size_t m = n; m >= 2; m--) {
        uint32_t cp = ent_lookup(s + i + 1, m);
        if (cp) {
            size_t u = 1 + m;
            if (m == n && k < len && s[k] == ';') u++;
            else if (m != n) { if (m < 4) return 0; }     /* &notit → ¬it only for long names */
            *used = u;
            return cp;
        }
    }
    return 0;
}

/* Characters that are never shown (preheader padding, BOM, soft hyphens). */
static int invisible(uint32_t cp) {
    return cp == 0x034f || cp == 0x200b || cp == 0x200c || cp == 0x200d || cp == 0x2060 || cp == 0xfeff || cp == 0x00ad ||
           cp == 0x200e || cp == 0x200f || (cp >= 0x202a && cp <= 0x202e) || (cp >= 0x2066 && cp <= 0x2069);
}

static uint32_t next_cp(const char *s, size_t len, size_t *i) {
    unsigned char c = (unsigned char)s[*i];
    if (c < 0x80) { (*i)++; return c; }
    int k = 0;
    uint32_t cp = 0;
    if (c >= 0xc2 && c <= 0xdf) { k = 2; cp = c & 0x1f; }
    else if (c >= 0xe0 && c <= 0xef) { k = 3; cp = c & 0x0f; }
    else if (c >= 0xf0 && c <= 0xf4) { k = 4; cp = c & 0x07; }
    if (!k || *i + (size_t)k > len) { (*i)++; return 0xfffd; }
    for (int j = 1; j < k; j++) {
        unsigned char d = (unsigned char)s[*i + (size_t)j];
        if ((d & 0xc0) != 0x80) { (*i)++; return 0xfffd; }
        cp = (cp << 6) | (d & 0x3f);
    }
    if ((k == 3 && cp < 0x800) || (k == 4 && (cp < 0x10000 || cp > 0x10ffff)) || (cp >= 0xd800 && cp <= 0xdfff)) { (*i)++; return 0xfffd; }
    *i += (size_t)k;
    return cp;
}

/* Bounded integer from an attribute (never atoi: it overflows on hostile input). */
static int hnum(const char *v, long lo, long hi, long dflt) {
    if (!v) return (int)dflt;
    while (*v == ' ') v++;
    char *e;
    errno = 0;
    long x = strtol(v, &e, 10);
    if (e == v || errno == ERANGE) return (int)dflt;
    return (int)(x < lo ? lo : x > hi ? hi : x);
}

/* ---- buffers ---- */

typedef struct { char *p; size_t n, cap; } Buf;
static void bput(Buf *b, const char *d, size_t n) {
    if (b->n + n + 1 > b->cap) {
        size_t nc = b->cap ? b->cap * 2 : 128;
        while (nc < b->n + n + 1) nc *= 2;
        char *np = realloc(b->p, nc);
        if (!np) return;
        b->p = np; b->cap = nc;
    }
    memcpy(b->p + b->n, d, n);
    b->n += n;
    b->p[b->n] = 0;
}

/* ---- colors and lengths ---- */

typedef struct { const char *n; uint32_t rgb; } Named;
static const Named named[] = {
    { "aqua", 0x00ffff }, { "black", 0x000000 }, { "blue", 0x0000ff }, { "brown", 0xa52a2a }, { "crimson", 0xdc143c }, { "cyan", 0x00ffff },
    { "darkblue", 0x00008b }, { "darkgray", 0xa9a9a9 }, { "darkgreen", 0x006400 }, { "darkgrey", 0xa9a9a9 }, { "darkred", 0x8b0000 },
    { "dimgray", 0x696969 }, { "fuchsia", 0xff00ff }, { "gainsboro", 0xdcdcdc }, { "gold", 0xffd700 }, { "gray", 0x808080 }, { "green", 0x008000 },
    { "grey", 0x808080 }, { "indigo", 0x4b0082 }, { "lightblue", 0xadd8e6 }, { "lightgray", 0xd3d3d3 }, { "lightgrey", 0xd3d3d3 },
    { "lime", 0x00ff00 }, { "magenta", 0xff00ff }, { "maroon", 0x800000 }, { "navy", 0x000080 }, { "olive", 0x808000 }, { "orange", 0xffa500 },
    { "pink", 0xffc0cb }, { "purple", 0x800080 }, { "red", 0xff0000 }, { "silver", 0xc0c0c0 }, { "skyblue", 0x87ceeb }, { "steelblue", 0x4682b4 },
    { "teal", 0x008080 }, { "tomato", 0xff6347 }, { "violet", 0xee82ee }, { "white", 0xffffff }, { "whitesmoke", 0xf5f5f5 }, { "yellow", 0xffff00 },
};

/* "#abc", "#aabbcc", "rgb(1,2,3)", "rgba(...)", names. Returns 1 and stores 0xRRGGBBAA. */
static int parse_color(const char *v, uint32_t *out) {
    while (*v == ' ') v++;
    if (*v == '#') {
        v++;
        size_t n = 0;
        while (isxdigit((unsigned char)v[n])) n++;
        unsigned long x = strtoul(v, NULL, 16);
        if (n == 3) { unsigned r = (unsigned)(x >> 8) & 15, g = (unsigned)(x >> 4) & 15, b = (unsigned)x & 15; *out = ((r * 17u) << 24) | ((g * 17u) << 16) | ((b * 17u) << 8) | 0xff; return 1; }
        if (n == 6) { *out = ((uint32_t)x << 8) | 0xff; return 1; }
        if (n == 8) { *out = (uint32_t)x; return 1; }
        return 0;
    }
    if (!strncasecmp(v, "rgb", 3)) {
        const char *p = strchr(v, '(');
        if (!p) return 0;
        double c[4] = { 0, 0, 0, 1 };
        int k = 0;
        p++;
        while (k < 4 && *p) {
            while (*p == ' ' || *p == ',' || *p == '/') p++;
            char *e;
            double x = strtod(p, &e);
            if (e == p) break;
            if (*e == '%') { x = k < 3 ? x * 2.55 : x / 100.0; e++; }
            c[k++] = x;
            p = e;
        }
        if (k < 3) return 0;
        for (int i = 0; i < 3; i++) c[i] = c[i] < 0 ? 0 : c[i] > 255 ? 255 : c[i];
        double a = c[3] < 0 ? 0 : c[3] > 1 ? 1 : c[3];
        *out = ((uint32_t)c[0] << 24) | ((uint32_t)c[1] << 16) | ((uint32_t)c[2] << 8) | (uint32_t)(a * 255);
        return 1;
    }
    char low[24];
    size_t n = 0;
    while (v[n] && v[n] != ' ' && v[n] != ';' && n + 1 < sizeof low) { low[n] = (char)tolower((unsigned char)v[n]); n++; }
    low[n] = 0;
    if (!strcmp(low, "transparent")) { *out = 0; return 1; }
    for (size_t i = 0; i < sizeof named / sizeof named[0]; i++) if (!strcmp(named[i].n, low)) { *out = (named[i].rgb << 8) | 0xff; return 1; }
    return 0;
}

/* CSS length → px (em/rem relative to base). Returns 1 if there is a number; *pct if a percentage. */
static int parse_len(const char *v, double base, double *px, int *pct) {
    while (*v == ' ') v++;
    char *e;
    double x = strtod(v, &e);
    if (e == v) return 0;
    *pct = 0;
    if (*e == '%') *pct = 1;
    else if (!strncasecmp(e, "pt", 2)) x *= 4.0 / 3.0;
    else if (!strncasecmp(e, "em", 2) || !strncasecmp(e, "rem", 3)) x *= base;
    else if (!strncasecmp(e, "pc", 2)) x *= 16;
    else if (!strncasecmp(e, "in", 2)) x *= 96;
    else if (!strncasecmp(e, "cm", 2)) x *= 37.8;
    else if (!strncasecmp(e, "mm", 2)) x *= 3.78;
    if (x != x || x > 1e5 || x < -1e5) return 0;
    *px = x;
    return 1;
}

/* ---- styles ---- */

typedef struct {
    unsigned b : 1, i : 1, u : 1, st : 1, mono : 1, pre : 1;
    uint32_t color, bg;
    unsigned has_color : 1, has_bg : 1;
    double fs;          /* 0 = the app's default */
    int link;           /* -1 = no link */
} IStyle;

typedef struct {
    /* block */
    int align;                  /* 0, 's', 'c', 'e', 'j' */
    int self_center;
    uint32_t bg; int has_bg;
    double pad[4]; int has_pad;
    double mt, mb;
    double w; int wpct, has_w;
    double mw; int has_mw;
    double bw; uint32_t bc; int has_border;
    double bl; uint32_t blc; int has_bl;
    double radius;
    int valign;                 /* 't','m','b' */
    int hidden;
} BStyle;

static int sw(const char *a, const char *b) { return !strncasecmp(a, b, strlen(b)); }

/* Applies a style="" attribute to both styles. */
static void apply_css(const char *css, size_t n, IStyle *is, BStyle *bs) {
    char decl[512];
    size_t i = 0;
    int overflow_hidden = 0, zero_height = 0;
    while (i < n) {
        size_t a = i;
        int par = 0;
        while (i < n && (par || css[i] != ';')) { if (css[i] == '(') par++; else if (css[i] == ')' && par) par--; i++; }
        size_t dl = i - a;
        i++;
        if (dl >= sizeof decl) continue;
        memcpy(decl, css + a, dl);
        decl[dl] = 0;
        char *colon = strchr(decl, ':');
        if (!colon) continue;
        *colon = 0;
        char *prop = decl, *val = colon + 1;
        while (*prop == ' ' || *prop == '\t' || *prop == '\n' || *prop == '\r') prop++;
        for (char *p = prop + strlen(prop); p > prop && (p[-1] == ' ' || p[-1] == '\t'); ) *--p = 0;
        for (char *p = prop; *p; p++) *p = (char)tolower((unsigned char)*p);
        while (*val == ' ' || *val == '\t' || *val == '\n' || *val == '\r') val++;
        char *imp = strcasestr(val, "!important");
        if (imp) *imp = 0;
        for (char *p = val + strlen(val); p > val && (p[-1] == ' ' || p[-1] == '\t' || p[-1] == '\r' || p[-1] == '\n'); ) *--p = 0;
        /* no url(), expression() or @import: the whole declaration is ignored */
        if (strcasestr(val, "url(") || strcasestr(val, "expression") || strcasestr(val, "javascript")) continue;
        double px; int pct; uint32_t c;
        double base = is->fs > 0 ? is->fs : 14;
        if (!strcmp(prop, "display") && !strcasecmp(val, "none")) bs->hidden = 1;
        else if (!strcmp(prop, "visibility") && !strcasecmp(val, "hidden")) bs->hidden = 1;
        else if (!strcmp(prop, "mso-hide") && !strcasecmp(val, "all")) bs->hidden = 1;
        else if (!strcmp(prop, "opacity") && strtod(val, NULL) <= 0.01 && isdigit((unsigned char)val[0])) bs->hidden = 1;
        else if (!strcmp(prop, "overflow") && !strcasecmp(val, "hidden")) overflow_hidden = 1;
        else if ((!strcmp(prop, "max-height") || !strcmp(prop, "height")) && parse_len(val, base, &px, &pct) && px <= 0) zero_height = 1;
        else if (!strcmp(prop, "color") && parse_color(val, &c)) { is->color = c; is->has_color = 1; }
        else if ((!strcmp(prop, "background-color") || !strcmp(prop, "background")) && parse_color(val, &c)) { bs->bg = c; bs->has_bg = 1; is->bg = c; is->has_bg = 0; }
        else if (!strcmp(prop, "font-weight")) { is->b = (sw(val, "bold") || sw(val, "bolder") || hnum(val, 0, 1000, 400) >= 600) ? 1 : 0; }
        else if (!strcmp(prop, "font-style")) is->i = sw(val, "italic") || sw(val, "oblique");
        else if (!strcmp(prop, "text-decoration") || !strcmp(prop, "text-decoration-line")) {
            if (strcasestr(val, "none")) { is->u = 0; is->st = 0; }
            if (strcasestr(val, "underline")) is->u = 1;
            if (strcasestr(val, "line-through")) is->st = 1;
        } else if (!strcmp(prop, "font-family")) is->mono = strcasestr(val, "mono") || strcasestr(val, "courier") || strcasestr(val, "consolas");
        else if (!strcmp(prop, "font-size")) {
            if (parse_len(val, base, &px, &pct)) { double v = pct ? base * px / 100 : px; if (v <= 0.5) bs->hidden = 1; else is->fs = v < 8 ? 8 : v > 48 ? 48 : v; }
            else if (!strcasecmp(val, "small")) is->fs = 12; else if (!strcasecmp(val, "x-small")) is->fs = 10;
            else if (!strcasecmp(val, "large")) is->fs = 18; else if (!strcasecmp(val, "x-large")) is->fs = 24; else if (!strcasecmp(val, "xx-large")) is->fs = 32;
            else if (!strcasecmp(val, "medium")) is->fs = 15;
        } else if (!strcmp(prop, "font")) {
            /* shorthand: only weight and size */
            if (strcasestr(val, "bold")) is->b = 1;
            if (strcasestr(val, "italic")) is->i = 1;
            const char *p = val;
            while (*p) { if (isdigit((unsigned char)*p) && parse_len(p, base, &px, &pct) && !pct && px >= 6) { is->fs = px > 48 ? 48 : px; break; } while (*p && *p != ' ') p++; while (*p == ' ') p++; }
        } else if (!strcmp(prop, "text-align")) {
            bs->align = sw(val, "center") ? 'c' : (sw(val, "right") || sw(val, "end")) ? 'e' : sw(val, "justify") ? 'j' : 's';
        } else if (!strcmp(prop, "white-space")) is->pre = sw(val, "pre");
        else if (!strncmp(prop, "padding", 7)) {
            double v[4];
            int k = 0;
            const char *p = val;
            while (k < 4 && *p) { if (!parse_len(p, base, &v[k], &pct)) break; if (pct) v[k] = 0; k++; while (*p && *p != ' ') p++; while (*p == ' ') p++; }
            if (!k) continue;
            if (!strcmp(prop, "padding")) {
                double t = v[0], r = k > 1 ? v[1] : t, b = k > 2 ? v[2] : t, l = k > 3 ? v[3] : r;
                bs->pad[0] = t; bs->pad[1] = r; bs->pad[2] = b; bs->pad[3] = l;
            } else if (!strcmp(prop, "padding-top")) bs->pad[0] = v[0];
            else if (!strcmp(prop, "padding-right")) bs->pad[1] = v[0];
            else if (!strcmp(prop, "padding-bottom")) bs->pad[2] = v[0];
            else if (!strcmp(prop, "padding-left")) bs->pad[3] = v[0];
            bs->has_pad = 1;
        } else if (!strncmp(prop, "margin", 6)) {
            if (!strcmp(prop, "margin")) {
                char tmp[128];
                snprintf(tmp, sizeof tmp, "%s", val);
                char *tok[4]; int k = 0;
                for (char *t = strtok(tmp, " "); t && k < 4; t = strtok(NULL, " ")) tok[k++] = t;
                if (!k) continue;
                const char *top = tok[0], *right = k > 1 ? tok[1] : top, *bot = k > 2 ? tok[2] : top, *left = k > 3 ? tok[3] : right;
                if (!strcasecmp(right, "auto") && !strcasecmp(left, "auto")) bs->self_center = 1;
                if (parse_len(top, base, &px, &pct) && !pct && px > 0) bs->mt = px;
                if (parse_len(bot, base, &px, &pct) && !pct && px > 0) bs->mb = px;
            } else if (!strcmp(prop, "margin-top") && parse_len(val, base, &px, &pct) && !pct && px > 0) bs->mt = px;
            else if (!strcmp(prop, "margin-bottom") && parse_len(val, base, &px, &pct) && !pct && px > 0) bs->mb = px;
        } else if (!strcmp(prop, "width") && parse_len(val, base, &px, &pct) && px > 0) { bs->w = px; bs->wpct = pct; bs->has_w = 1; }
        else if (!strcmp(prop, "max-width") && parse_len(val, base, &px, &pct) && !pct && px > 0) { bs->mw = px; bs->has_mw = 1; }
        else if (!strcmp(prop, "border") || !strcmp(prop, "border-left") || !strcmp(prop, "border-top") || !strcmp(prop, "border-bottom")) {
            double w = 0; uint32_t col = 0x999999ff; int none = strcasestr(val, "none") != NULL;
            const char *p = val;
            while (*p) {
                if (isdigit((unsigned char)*p)) { parse_len(p, base, &w, &pct); }
                else if (*p == '#' || !strncasecmp(p, "rgb", 3) || isalpha((unsigned char)*p)) parse_color(p, &col);
                while (*p && *p != ' ') { if (*p == '(') { while (*p && *p != ')') p++; } if (*p) p++; }
                while (*p == ' ') p++;
            }
            if (none || w <= 0) continue;
            if (w > 8) w = 8;
            if (!strcmp(prop, "border-left")) { bs->bl = w; bs->blc = col; bs->has_bl = 1; }
            else if (!strcmp(prop, "border")) { bs->bw = w; bs->bc = col; bs->has_border = 1; }
        } else if (!strcmp(prop, "border-radius") && parse_len(val, base, &px, &pct) && !pct) bs->radius = px > 40 ? 40 : px;
        else if (!strcmp(prop, "vertical-align")) bs->valign = sw(val, "middle") ? 'm' : sw(val, "bottom") ? 'b' : 't';
    }
    if (overflow_hidden && zero_height) bs->hidden = 1;
}

/* ---- tags ---- */

enum { K_INLINE, K_BLOCK, K_DROP, K_RAW, K_IGNORE };

static int tag_is(const char *t, const char *const *list) { for (; *list; list++) if (!strcmp(t, *list)) return 1; return 0; }
static const char *const T_VOID[] = { "br", "hr", "img", "meta", "link", "input", "wbr", "col", "area", "base", "source", "track", "param", "embed", "keygen", "basefont", "image", NULL };
static const char *const T_RAW[] = { "script", "style", "textarea", "title", "xmp", "noembed", "noframes", "plaintext", "iframe", NULL };
static const char *const T_DROP[] = { "head", "object", "applet", "frame", "frameset", "noscript", "template", "svg", "math", "audio", "video", "canvas", "map",
                                      "select", "button", "option", "optgroup", "datalist", "dialog", "portal", "picture", NULL };
static const char *const T_BLOCK[] = { "div", "p", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "center", "ul", "ol", "li", "dl", "dt", "dd",
                                       "address", "article", "section", "header", "footer", "nav", "main", "aside", "figure", "figcaption", "table",
                                       "tr", "td", "th", "caption", "body", "html", "form", "fieldset", "legend", "details", "summary", "hgroup", "menu", "listing", NULL };
static const char *const T_IGNORE[] = { "thead", "tbody", "tfoot", "colgroup", NULL };

static int kind_of(const char *t) {
    if (tag_is(t, T_RAW)) return K_RAW;
    if (tag_is(t, T_DROP)) return K_DROP;
    if (tag_is(t, T_IGNORE)) return K_IGNORE;
    if (tag_is(t, T_BLOCK)) return K_BLOCK;
    return K_INLINE;
}

/* ---- attributes ---- */

#define MAX_ATTRS 32
typedef struct { char name[24]; const char *v; size_t vn; char *dec; } Attr;

static const char *attr(Attr *a, int n, const char *name) {
    for (int i = 0; i < n; i++) if (!strcmp(a[i].name, name)) return a[i].dec ? a[i].dec : "";
    return NULL;
}

/* attribute value with entities decoded (malloc) */
static char *decode_attr(const char *v, size_t n) {
    Buf b = { 0 };
    for (size_t i = 0; i < n;) {
        if (v[i] == '&') {
            size_t used;
            uint32_t cp = entity(v, n, i, &used);
            if (cp && used) { char t[4]; bput(&b, t, (size_t)put_cp(t, cp)); i += used; continue; }
        }
        bput(&b, v + i, 1);
        i++;
    }
    if (!b.p) { b.p = calloc(1, 1); }
    return b.p;
}

/* ---- the builder ---- */

typedef struct { char tag[16]; IStyle is; int frame; int list_n, ordered; } Open;
typedef struct { int kids; char kind; } Frame;
typedef struct { char *href; Buf text; int warn; } Link;

typedef struct {
    JBuf out;
    Open open[HTML_MAX_DEPTH * 2];
    int nopen;
    Frame fr[HTML_MAX_DEPTH + 2];
    int nfr;
    IStyle cur;
    Buf run;
    IStyle run_st;
    int space, line_start;
    Link links[HTML_MAX_LINKS];
    int nlinks, cur_link;
    char *remote[HTML_MAX_REMOTE];
    int nremote;
    HtmlStats st;
    uint32_t root_bg; int has_root_bg;
    HtmlData *data; int ndata;
    int colors;
} H;

static void frame_sep(H *h) {
    Frame *f = &h->fr[h->nfr - 1];
    if (f->kids++) jb_raw(&h->out, ",");
}

static int istyle_eq(const IStyle *a, const IStyle *b) {
    return a->b == b->b && a->i == b->i && a->u == b->u && a->st == b->st && a->mono == b->mono && a->has_color == b->has_color &&
           (!a->has_color || a->color == b->color) && a->fs == b->fs && a->link == b->link && a->has_bg == b->has_bg && (!a->has_bg || a->bg == b->bg);
}

static void color_json(JBuf *o, const char *k, uint32_t c) {
    if ((c & 0xff) == 0xff) jb_printf(o, ",\"%s\":\"#%06x\"", k, c >> 8);
    else jb_printf(o, ",\"%s\":\"#%08x\"", k, c);
}

static void flush_run(H *h) {
    if (!h->run.n) return;
    if (h->st.nodes++ >= HTML_MAX_NODES) { h->st.truncated = 1; h->run.n = 0; return; }
    frame_sep(h);
    jb_raw(&h->out, "{\"t\":\"x\",\"x\":");
    jb_strn(&h->out, h->run.p, h->run.n);
    IStyle *s = &h->run_st;
    if (s->b || s->i || s->u || s->st || s->mono || s->has_color || s->fs > 0 || s->has_bg) {
        jb_raw(&h->out, ",\"s\":{\"_\":0");
        if (s->b) jb_raw(&h->out, ",\"b\":1");
        if (s->i) jb_raw(&h->out, ",\"i\":1");
        if (s->u) jb_raw(&h->out, ",\"u\":1");
        if (s->st) jb_raw(&h->out, ",\"st\":1");
        if (s->mono) jb_raw(&h->out, ",\"mono\":1");
        if (s->has_color) { color_json(&h->out, "c", s->color); h->colors = 1; }
        if (s->has_bg) color_json(&h->out, "bg", s->bg);
        if (s->fs > 0) jb_printf(&h->out, ",\"fs\":%d", (int)(s->fs + 0.5));
        jb_raw(&h->out, "}");
    }
    if (s->link >= 0) jb_printf(&h->out, ",\"l\":%d", s->link);
    jb_raw(&h->out, "}");
    h->run.n = 0;
}

static void block_break(H *h) {
    flush_run(h);
    h->space = 0;
    h->line_start = 1;
}

static void add_text(H *h, const char *s, size_t n) {
    /* s: UTF-8, already decoded */
    for (size_t i = 0; i < n;) {
        uint32_t cp = next_cp(s, n, &i);
        if (invisible(cp)) continue;
        if (!h->cur.pre && (cp == ' ' || cp == '\t' || cp == '\n' || cp == '\r' || cp == '\f')) { if (!h->line_start) h->space = 1; continue; }
        if (h->cur.pre && cp == '\r') continue;
        if (h->cur.pre && cp == '\n') {
            flush_run(h);
            if (h->st.nodes++ < HTML_MAX_NODES) { frame_sep(h); jb_raw(&h->out, "{\"t\":\"br\"}"); }
            h->line_start = 1;
            continue;
        }
        if (cp < 0x20 && cp != '\t') continue;
        if (h->run.n && !istyle_eq(&h->run_st, &h->cur)) flush_run(h);
        if (!h->run.n) h->run_st = h->cur;
        if (h->space) {
            bput(&h->run, " ", 1);
            if (h->cur_link >= 0) bput(&h->links[h->cur_link].text, " ", 1);
        }
        h->space = 0;
        h->line_start = 0;
        char t[4];
        int k = put_cp(t, cp);
        bput(&h->run, t, (size_t)k);
        if (h->cur_link >= 0 && h->links[h->cur_link].text.n < 600) bput(&h->links[h->cur_link].text, t, (size_t)k);
    }
}

static void bstyle_json(H *h, const BStyle *b, const IStyle *is) {
    JBuf *o = &h->out;
    jb_raw(o, ",\"s\":{\"_\":0");
    if (b->align) jb_printf(o, ",\"a\":\"%c\"", b->align);
    if (b->self_center) jb_raw(o, ",\"ac\":1");
    if (b->has_bg) { color_json(o, "bg", b->bg); h->colors = 1; }
    if (b->has_pad) jb_printf(o, ",\"p\":[%d,%d,%d,%d]", (int)b->pad[0], (int)b->pad[1], (int)b->pad[2], (int)b->pad[3]);
    if (b->mt > 0) jb_printf(o, ",\"mt\":%d", (int)(b->mt > 60 ? 60 : b->mt));
    if (b->mb > 0) jb_printf(o, ",\"mb\":%d", (int)(b->mb > 60 ? 60 : b->mb));
    if (b->has_w) jb_printf(o, b->wpct ? ",\"wp\":%d" : ",\"w\":%d", (int)(b->w > 4000 ? 4000 : b->w));
    if (b->has_mw) jb_printf(o, ",\"mw\":%d", (int)(b->mw > 4000 ? 4000 : b->mw));
    if (b->has_border) { jb_printf(o, ",\"bw\":%d", (int)(b->bw + 0.5) ? (int)(b->bw + 0.5) : 1); color_json(o, "bc", b->bc); }
    if (b->has_bl) { jb_printf(o, ",\"bl\":%d", (int)(b->bl + 0.5) ? (int)(b->bl + 0.5) : 1); color_json(o, "blc", b->blc); }
    if (b->radius > 0) jb_printf(o, ",\"r\":%d", (int)b->radius);
    if (b->valign) jb_printf(o, ",\"va\":\"%c\"", b->valign);
    if (is && is->pre) jb_raw(o, ",\"pre\":1");
    jb_raw(o, "}");
}

static int open_frame(H *h, const char *t, const BStyle *b, const char *extra) {
    if (h->nfr >= HTML_MAX_DEPTH) { block_break(h); return 0; }
    block_break(h);
    if (h->st.nodes++ >= HTML_MAX_NODES) { h->st.truncated = 1; return 0; }
    frame_sep(h);
    jb_printf(&h->out, "{\"t\":\"%s\"", t);
    bstyle_json(h, b, &h->cur);
    if (extra) jb_raw(&h->out, extra);
    jb_raw(&h->out, ",\"c\":[");
    h->fr[h->nfr].kids = 0;
    h->fr[h->nfr].kind = !strcmp(t, "tbl") ? 'T' : !strcmp(t, "tr") ? 'R' : !strcmp(t, "td") ? 'D' : 'B';
    h->nfr++;
    return 1;
}

static void close_frame(H *h) {
    block_break(h);
    if (h->nfr <= 1) return;
    jb_raw(&h->out, "]}");
    h->nfr--;
}

static void end_link(H *h) {
    if (h->cur_link < 0) return;
    Link *l = &h->links[h->cur_link];
    const char *why = html_link_warning(l->text.p ? l->text.p : "", l->href);
    if (why) { l->warn = 1; h->st.warn++; }
    h->cur_link = -1;
    h->cur.link = -1;
}

/* Closes open elements from the top down to index idx (inclusive). */
static void pop_to(H *h, int idx) {
    while (h->nopen > idx) {
        Open *o = &h->open[--h->nopen];
        if (!strcmp(o->tag, "a")) end_link(h);
        if (o->frame) close_frame(h);
        else if (kind_of(o->tag) == K_BLOCK) block_break(h);
        flush_run(h);
        h->cur = o->is;          /* the style from before it was opened */
        if (h->cur_link >= 0 && h->cur.link != h->cur_link) h->cur_link = h->cur.link;
    }
}

static int find_open(H *h, const char *tag, const char *const *stop) {
    for (int i = h->nopen - 1; i >= 0; i--) {
        if (!strcmp(h->open[i].tag, tag)) return i;
        if (stop && tag_is(h->open[i].tag, stop)) return -1;
    }
    return -1;
}

static const char *const STOP_CELL[] = { "td", "th", "table", NULL };
static const char *const STOP_TABLE[] = { "table", NULL };
static const char *const STOP_LIST[] = { "ul", "ol", "table", "td", "th", NULL };

static int is_ip_host(const char *h) {
    if (h[0] == '[') return 1;
    int dots = 0, digits = 1;
    for (const char *c = h; *c; c++) { if (*c == '.') dots++; else if (!isdigit((unsigned char)*c)) digits = 0; }
    return digits && dots == 3;
}

void html_url_host(const char *url, char *out, size_t cap) {
    if (!cap) return;
    out[0] = 0;
    if (!url) return;
    const char *p = url;
    while (*p == ' ') p++;
    const char *sch = strstr(p, "://");
    if (sch && sch - p < 10) p = sch + 3;
    else if (!strncasecmp(p, "www.", 4)) { /* no scheme */ }
    else return;
    const char *end = p;
    while (*end && *end != '/' && *end != '?' && *end != '#') end++;
    /* user@host */
    const char *at = NULL;
    for (const char *c = p; c < end; c++) if (*c == '@') at = c;
    if (at) p = at + 1;
    const char *colon = NULL;
    for (const char *c = p; c < end; c++) if (*c == ':' && p[0] != '[') colon = c;
    if (colon) end = colon;
    size_t n = (size_t)(end - p);
    if (n >= cap) n = cap - 1;
    for (size_t i = 0; i < n; i++) out[i] = (char)tolower((unsigned char)p[i]);
    out[n] = 0;
    while (n && out[n - 1] == '.') out[--n] = 0;
    if (n >= 4 && !strncmp(out, "www.", 4)) memmove(out, out + 4, n - 3);
}

static int host_match(const char *a, const char *b) {
    if (!strcmp(a, b)) return 1;
    size_t la = strlen(a), lb = strlen(b);
    if (la > lb && a[la - lb - 1] == '.' && !strcmp(a + la - lb, b)) return 1;
    if (lb > la && b[lb - la - 1] == '.' && !strcmp(b + lb - la, a)) return 1;
    return 0;
}

/* Does the text look like a web address? Stores the host in out. */
static int text_as_url(const char *t, char *out, size_t cap) {
    char buf[600];
    size_t n = 0;
    while (*t == ' ') t++;
    for (; *t && n + 1 < sizeof buf; t++) buf[n++] = *t;
    while (n && (buf[n - 1] == ' ' || buf[n - 1] == '.' || buf[n - 1] == '/')) n--;
    buf[n] = 0;
    if (!n || strchr(buf, ' ')) return 0;
    if (!strncasecmp(buf, "http://", 7) || !strncasecmp(buf, "https://", 8) || !strncasecmp(buf, "www.", 4)) {
        html_url_host(buf, out, cap);
        return out[0] != 0;
    }
    /* domain.tld[/...] */
    const char *slash = strchr(buf, '/');
    size_t hl = slash ? (size_t)(slash - buf) : n;
    if (hl >= cap || hl < 4) return 0;
    int dot = 0;
    for (size_t i = 0; i < hl; i++) {
        char c = buf[i];
        if (c == '.') dot = 1;
        else if (!isalnum((unsigned char)c) && c != '-' && (unsigned char)c < 0x80) return 0;
        if (c == '@') return 0;
    }
    if (!dot) return 0;
    /* the last label must be letters (a TLD), not "3.5" */
    const char *ld = strrchr(buf, '.');
    if (!ld || (size_t)(ld - buf) >= hl || !isalpha((unsigned char)ld[1])) return 0;
    for (size_t i = 0; i < hl; i++) out[i] = (char)tolower((unsigned char)buf[i]);
    out[hl] = 0;
    if (!strncmp(out, "www.", 4)) memmove(out, out + 4, strlen(out + 4) + 1);
    return 1;
}

const char *html_link_warning(const char *text, const char *href) {
    if (!href) return NULL;
    if (!strncasecmp(href, "mailto:", 7)) {
        /* text shaped like an email address different from the mailto one */
        char tt[300];
        snprintf(tt, sizeof tt, "%s", text ? text : "");
        char *a = tt; while (*a == ' ') a++;
        size_t l = strlen(a); while (l && a[l - 1] == ' ') a[--l] = 0;
        if (strchr(a, '@') && !strchr(a, ' ')) {
            const char *m = href + 7;
            size_t ml = strcspn(m, "?");
            if (strlen(a) != ml || strncasecmp(a, m, ml)) return "deceptive";
        }
        return NULL;
    }
    char hh[256];
    html_url_host(href, hh, sizeof hh);
    if (!hh[0]) return NULL;
    /* user@ in the authority: "https://bank.com@other.net" */
    const char *p = strstr(href, "://");
    if (p) {
        p += 3;
        const char *e = p + strcspn(p, "/?#");
        if (memchr(p, '@', (size_t)(e - p))) return "userinfo";
    }
    char th[256];
    int looks = text_as_url(text ? text : "", th, sizeof th);
    if (is_ip_host(hh) && looks) return "ip";
    if (looks && !host_match(th, hh)) return "deceptive";
    if (looks && strstr(hh, "xn--") && !strstr(th, "xn--")) return "punycode";
    return NULL;
}

static int href_ok(const char *v) {
    /* no whitespace or control characters inside the scheme ("java\tscript:") */
    char sch[16];
    size_t n = 0;
    for (const char *c = v; *c && n + 1 < sizeof sch; c++) {
        if ((unsigned char)*c <= 0x20) continue;
        if (*c == ':') break;
        sch[n++] = (char)tolower((unsigned char)*c);
    }
    sch[n] = 0;
    if (!strchr(v, ':')) return 0;
    return !strcmp(sch, "http") || !strcmp(sch, "https") || !strcmp(sch, "mailto");
}

static int add_remote(H *h, const char *url) {
    for (int i = 0; i < h->nremote; i++) if (!strcmp(h->remote[i], url)) return i;
    if (h->nremote >= HTML_MAX_REMOTE) return -1;
    h->remote[h->nremote] = strdup(url);
    if (!h->remote[h->nremote]) return -1;
    return h->nremote++;
}

static void do_img(H *h, Attr *a, int na, const BStyle *bs) {
    const char *src = attr(a, na, "src");
    if (!src || !*src) return;
    while (*src == ' ' || *src == '\n' || *src == '\t') src++;
    double w = 0, hh = 0, px; int pct = 0, wpct = 0;
    const char *ws = attr(a, na, "width"), *hs = attr(a, na, "height");
    if (ws && parse_len(ws, 14, &px, &pct)) { w = px; wpct = pct; }
    if (hs && parse_len(hs, 14, &px, &pct) && !pct) hh = px;
    if (bs->has_w) { w = bs->w; wpct = bs->wpct; }
    if (!wpct && ((ws && w <= 2 && hs && hh <= 2) || (ws && w == 0) || (hs && hh == 0))) { h->st.trackers++; return; }
    char ref[32];
    if (!strncasecmp(src, "cid:", 4)) { h->st.cid++; ref[0] = 0; }
    else if (!strncasecmp(src, "http://", 7) || !strncasecmp(src, "https://", 8)) {
        int k = add_remote(h, src);
        if (k < 0) return;
        snprintf(ref, sizeof ref, "r:%d", k);
        h->st.remote++;
    } else if (!strncasecmp(src, "data:image/", 11)) {
        const char *semi = strstr(src, ";base64,");
        if (!semi || !h->data || h->ndata >= HTML_MAX_DATA) return;
        HtmlData *d = &h->data[h->ndata];
        size_t tl = (size_t)(semi - (src + 11));
        if (tl >= sizeof d->type) return;
        memcpy(d->type, src + 11, tl);
        d->type[tl] = 0;
        if (strcmp(d->type, "png") && strcmp(d->type, "jpeg") && strcmp(d->type, "jpg") && strcmp(d->type, "gif") && strcmp(d->type, "bmp")) return;
        d->p = semi + 8;     /* points to the decoded copy (attributes live until the end) */
        d->n = strlen(d->p);
        snprintf(ref, sizeof ref, "d:%d", h->ndata);
        h->ndata++;
        h->st.data++;
    } else return;            /* relative paths, file:, javascript: … */
    flush_run(h);
    if (h->st.nodes++ >= HTML_MAX_NODES) { h->st.truncated = 1; return; }
    frame_sep(h);
    jb_raw(&h->out, "{\"t\":\"img\",\"src\":");
    if (ref[0]) jb_str(&h->out, ref);
    else {
        char cid[200];
        snprintf(cid, sizeof cid, "cid:%s", src + 4);
        jb_str(&h->out, cid);
    }
    if (w > 0) jb_printf(&h->out, wpct ? ",\"wp\":%d" : ",\"w\":%d", (int)(w > 4000 ? 4000 : w));
    if (hh > 0) jb_printf(&h->out, ",\"h\":%d", (int)(hh > 4000 ? 4000 : hh));
    const char *alt = attr(a, na, "alt");
    if (alt && *alt) { char t[200]; snprintf(t, sizeof t, "%s", alt); jb_raw(&h->out, ",\"alt\":"); jb_str(&h->out, t); }
    if (h->cur.link >= 0) jb_printf(&h->out, ",\"l\":%d", h->cur.link);
    if (bs->align) jb_printf(&h->out, ",\"a\":\"%c\"", bs->align);
    jb_raw(&h->out, "}");
    h->line_start = 0;
}

/* Legacy presentational attributes (bgcolor, align, width…) */
static void legacy_attrs(Attr *a, int na, IStyle *is, BStyle *bs, const char *tag) {
    const char *v;
    uint32_t c;
    double px; int pct;
    if ((v = attr(a, na, "bgcolor")) && parse_color(v, &c)) { bs->bg = c; bs->has_bg = 1; }
    if ((v = attr(a, na, "align"))) {
        if (!strcmp(tag, "table") || !strcmp(tag, "img")) { if (!strcasecmp(v, "center")) bs->self_center = 1; }
        else bs->align = !strcasecmp(v, "center") ? 'c' : !strcasecmp(v, "right") ? 'e' : !strcasecmp(v, "justify") ? 'j' : 's';
    }
    if ((v = attr(a, na, "valign"))) bs->valign = !strcasecmp(v, "middle") ? 'm' : !strcasecmp(v, "bottom") ? 'b' : 't';
    if (strcmp(tag, "img") && (v = attr(a, na, "width")) && parse_len(v, 14, &px, &pct) && px > 0) { bs->w = px; bs->wpct = pct; bs->has_w = 1; }
    if (!strcmp(tag, "table") && (v = attr(a, na, "border")) && hnum(v, 0, 4, 0) > 0) { bs->bw = hnum(v, 0, 4, 0); bs->bc = 0x999999ff; bs->has_border = 1; }
    if (!strcmp(tag, "font")) {
        if ((v = attr(a, na, "color")) && parse_color(v, &c)) { is->color = c; is->has_color = 1; }
        if ((v = attr(a, na, "size"))) {
            static const int fsz[] = { 10, 10, 13, 15, 18, 24, 32, 44 };
            int k = hnum(v, -10, 10, 3);
            if (v[0] == '+') k = 3 + hnum(v + 1, 0, 10, 0); else if (v[0] == '-') k = 3 - hnum(v + 1, 0, 10, 0);
            if (k < 1) k = 1;
            if (k > 7) k = 7;
            is->fs = fsz[k];
        }
        if ((v = attr(a, na, "face"))) is->mono = strcasestr(v, "mono") || strcasestr(v, "courier");
    }
    if (!strcmp(tag, "body") && (v = attr(a, na, "text")) && parse_color(v, &c)) { is->color = c; is->has_color = 1; }
    if ((v = attr(a, na, "hidden"))) bs->hidden = 1;
}

static void open_tag(H *h, const char *tag, Attr *a, int na, int selfclose) {
    int kind = kind_of(tag);
    if (kind == K_IGNORE) return;
    int is_void = tag_is(tag, T_VOID);
    /* implicit closes */
    if (kind == K_BLOCK) {
        int p = find_open(h, "p", STOP_CELL);
        if (p >= 0 && strcmp(tag, "td") && strcmp(tag, "th") && strcmp(tag, "tr")) pop_to(h, p);
    }
    if (!strcmp(tag, "li")) { int k = find_open(h, "li", STOP_LIST); if (k >= 0) pop_to(h, k); }
    if (!strcmp(tag, "dt") || !strcmp(tag, "dd")) { int k = find_open(h, "dt", STOP_LIST); if (k < 0) k = find_open(h, "dd", STOP_LIST); if (k >= 0) pop_to(h, k); }
    if (!strcmp(tag, "td") || !strcmp(tag, "th")) {
        int k = find_open(h, "td", STOP_TABLE); if (k < 0) k = find_open(h, "th", STOP_TABLE);
        if (k >= 0) pop_to(h, k);
    }
    if (!strcmp(tag, "tr")) {
        int k = find_open(h, "tr", STOP_TABLE);
        if (k >= 0) pop_to(h, k);
    }
    if (!strcmp(tag, "a")) { int k = find_open(h, "a", STOP_CELL); if (k >= 0) pop_to(h, k); }
    /* stray td inside a table without tr: open a tr */
    if ((!strcmp(tag, "td") || !strcmp(tag, "th")) && h->nfr > 0 && h->fr[h->nfr - 1].kind == 'T' && h->nopen < (int)(sizeof h->open / sizeof h->open[0])) {
        Attr none[1];
        open_tag(h, "tr", none, 0, 0);
    }

    IStyle saved = h->cur;
    BStyle bs;
    memset(&bs, 0, sizeof bs);
    legacy_attrs(a, na, &h->cur, &bs, tag);
    const char *style = attr(a, na, "style");
    if (style) apply_css(style, strlen(style), &h->cur, &bs);

    if (bs.hidden || kind == K_DROP) {
        h->cur = saved;
        /* skipped up to its close: the main loop does it with skip */
        if (!is_void && !selfclose) {
            if (h->nopen < (int)(sizeof h->open / sizeof h->open[0])) {
                Open *o = &h->open[h->nopen++];
                snprintf(o->tag, sizeof o->tag, "%s", tag);
                o->is = saved;
                o->frame = -1;          /* marker: skip */
            }
        }
        return;
    }

    /* per-tag styles */
    if (!strcmp(tag, "b") || !strcmp(tag, "strong") || !strcmp(tag, "th")) h->cur.b = 1;
    else if (!strcmp(tag, "i") || !strcmp(tag, "em") || !strcmp(tag, "cite") || !strcmp(tag, "var") || !strcmp(tag, "dfn")) h->cur.i = 1;
    else if (!strcmp(tag, "u") || !strcmp(tag, "ins")) h->cur.u = 1;
    else if (!strcmp(tag, "s") || !strcmp(tag, "strike") || !strcmp(tag, "del")) h->cur.st = 1;
    else if (!strcmp(tag, "code") || !strcmp(tag, "tt") || !strcmp(tag, "kbd") || !strcmp(tag, "samp")) h->cur.mono = 1;
    else if (!strcmp(tag, "pre") || !strcmp(tag, "listing")) { h->cur.mono = 1; h->cur.pre = 1; }
    else if (!strcmp(tag, "small") || !strcmp(tag, "sub") || !strcmp(tag, "sup")) h->cur.fs = (saved.fs > 0 ? saved.fs : 14) * 0.85;
    else if (!strcmp(tag, "big")) h->cur.fs = (saved.fs > 0 ? saved.fs : 14) * 1.2;
    else if (tag[0] == 'h' && tag[1] >= '1' && tag[1] <= '6' && !tag[2]) {
        static const int hs[] = { 24, 20, 17, 15, 14, 13 };
        h->cur.b = 1;
        if (!style || !strcasestr(style, "font-size")) h->cur.fs = hs[tag[1] - '1'];
        if (bs.mt <= 0) bs.mt = 10;
        if (bs.mb <= 0) bs.mb = 6;
    } else if (!strcmp(tag, "p")) { if (bs.mt <= 0 && !(style && strcasestr(style, "margin"))) bs.mt = 6; if (bs.mb <= 0 && !(style && strcasestr(style, "margin"))) bs.mb = 6; }
    else if (!strcmp(tag, "blockquote")) { if (!bs.has_bl) { bs.bl = 3; bs.blc = 0x88888888; bs.has_bl = 1; } if (!bs.has_pad) { bs.pad[3] = 10; bs.has_pad = 1; } bs.mt = bs.mt > 0 ? bs.mt : 4; bs.mb = bs.mb > 0 ? bs.mb : 4; }
    else if (!strcmp(tag, "center")) bs.align = 'c';
    else if (!strcmp(tag, "ul") || !strcmp(tag, "ol") || !strcmp(tag, "menu") || !strcmp(tag, "dd")) { if (!bs.has_pad) { bs.pad[3] = 22; bs.has_pad = 1; } }

    if (!strcmp(tag, "body") || !strcmp(tag, "html")) {
        if (bs.has_bg && !h->has_root_bg) { h->root_bg = bs.bg; h->has_root_bg = 1; }
        bs.has_bg = 0;
    }

    if (!strcmp(tag, "br")) {
        flush_run(h);
        if (h->st.nodes++ < HTML_MAX_NODES) { frame_sep(h); jb_raw(&h->out, "{\"t\":\"br\"}"); }
        h->space = 0;
        h->line_start = 1;
        h->cur = saved;
        return;
    }
    if (!strcmp(tag, "hr")) {
        block_break(h);
        if (h->st.nodes++ < HTML_MAX_NODES) { frame_sep(h); jb_raw(&h->out, "{\"t\":\"hr\"}"); }
        h->cur = saved;
        return;
    }
    if (!strcmp(tag, "img") || !strcmp(tag, "image")) { do_img(h, a, na, &bs); h->cur = saved; return; }
    if (is_void) { h->cur = saved; return; }

    if (!strcmp(tag, "a")) {
        const char *href = attr(a, na, "href");
        h->cur.link = -1;
        if (href && href_ok(href) && h->nlinks < HTML_MAX_LINKS) {
            Link *l = &h->links[h->nlinks];
            memset(l, 0, sizeof *l);
            l->href = strdup(href);
            if (l->href) {
                /* canonical form: no blanks or control characters anywhere (as browsers do) */
                size_t o2 = 0;
                for (char *c = l->href; *c; c++) if ((unsigned char)*c > 0x20 && *c != 0x7f) l->href[o2++] = *c;
                l->href[o2] = 0;
                h->cur.link = h->nlinks;
                h->cur_link = h->nlinks;
                h->nlinks++;
            }
        }
    }

    if (h->nopen >= (int)(sizeof h->open / sizeof h->open[0])) { h->cur = saved; return; }
    Open *o = &h->open[h->nopen++];
    snprintf(o->tag, sizeof o->tag, "%s", tag);
    o->is = saved;
    o->frame = 0;
    o->list_n = 0;
    o->ordered = !strcmp(tag, "ol");
    if (kind == K_BLOCK) {
        const char *t = "blk";
        char extra[96] = "";
        if (!strcmp(tag, "table")) {
            t = "tbl";
            const char *cp = attr(a, na, "cellpadding");
            if (cp && hnum(cp, 0, 30, 0) > 0) snprintf(extra, sizeof extra, ",\"cp\":%d", hnum(cp, 0, 30, 0));
        } else if (!strcmp(tag, "tr")) t = "tr";
        else if (!strcmp(tag, "td") || !strcmp(tag, "th")) {
            t = "td";
            const char *cs = attr(a, na, "colspan");
            if (cs && hnum(cs, 1, 50, 1) > 1) snprintf(extra, sizeof extra, ",\"cs\":%d", hnum(cs, 1, 50, 1));
        } else if (!strcmp(tag, "li")) {
            int lst = find_open(h, "ol", STOP_LIST);
            int ul = find_open(h, "ul", STOP_LIST);
            if (lst > ul) { h->open[lst].list_n++; snprintf(extra, sizeof extra, ",\"mk\":\"%d.\"", h->open[lst].list_n); }
            else snprintf(extra, sizeof extra, ",\"mk\":\"•\"");
        }
        o->frame = open_frame(h, t, &bs, extra[0] ? extra : NULL);
    }
}

static void close_tag(H *h, const char *tag) {
    if (kind_of(tag) == K_IGNORE) return;
    const char *const *stop = (!strcmp(tag, "td") || !strcmp(tag, "th") || !strcmp(tag, "tr")) ? STOP_TABLE :
                              !strcmp(tag, "table") ? NULL : STOP_CELL;
    if (!strcmp(tag, "td") || !strcmp(tag, "th") || !strcmp(tag, "tr")) {
        int k = find_open(h, tag, stop);
        if (k >= 0) pop_to(h, k);
        return;
    }
    if (!strcmp(tag, "br")) { Attr none[1]; open_tag(h, "br", none, 0, 1); return; }    /* </br> = <br> */
    int k = find_open(h, tag, stop);
    if (k >= 0) pop_to(h, k);
}

/* ---- tokenizer ---- */

static int name_char(unsigned char c) { return isalnum(c) || c == '-' || c == '_' || c == ':'; }

char *html_sanitize(const char *s, size_t len, size_t *olen, HtmlStats *stats, HtmlData *data, int *ndata) {
    H *h = calloc(1, sizeof *h);
    if (!h) return NULL;
    if (len > HTML_MAX_INPUT) { len = HTML_MAX_INPUT; h->st.truncated = 1; }
    jb_init(&h->out);
    h->cur.link = -1;
    h->cur_link = -1;
    h->line_start = 1;
    h->data = data;
    jb_raw(&h->out, "{\"v\":1,\"blocks\":[");
    h->nfr = 1;
    h->fr[0].kids = 0;
    h->fr[0].kind = 'r';
    /* decoded attribute values live until the end (data: references them) */
    char **keep = NULL;
    size_t nkeep = 0, capkeep = 0;
    Attr at[MAX_ATTRS];
    size_t i = 0;
    while (i < len) {
        /* inside something being skipped? */
        int skipping = h->nopen && h->open[h->nopen - 1].frame == -1;
        if (s[i] != '<') {
            size_t a = i;
            while (i < len && s[i] != '<') i++;
            if (skipping) continue;
            /* text: entities → UTF-8 */
            Buf t = { 0 };
            for (size_t k = a; k < i;) {
                if (s[k] == '&') {
                    size_t used;
                    uint32_t cp = entity(s, i, k, &used);
                    if (cp && used) { char u[4]; bput(&t, u, (size_t)put_cp(u, cp)); k += used; continue; }
                    bput(&t, "&", 1);
                    k++;
                    continue;
                }
                size_t k0 = k;
                while (k < i && s[k] != '&') k++;
                bput(&t, s + k0, k - k0);
            }
            if (t.n) add_text(h, t.p, t.n);
            free(t.p);
            continue;
        }
        /* comments, doctype, CDATA, instructions */
        if (i + 3 < len && !memcmp(s + i, "<!--", 4)) {
            const char *e = NULL;
            for (size_t k = i + 4; k + 2 < len + 0 && k + 3 <= len; k++) if (s[k] == '-' && s[k + 1] == '-' && s[k + 2] == '>') { e = s + k; break; }
            i = e ? (size_t)(e - s) + 3 : len;
            continue;
        }
        if (i + 1 < len && (s[i + 1] == '!' || s[i + 1] == '?')) {
            const char *e = memchr(s + i, '>', len - i);
            i = e ? (size_t)(e - s) + 1 : len;
            continue;
        }
        int closing = i + 1 < len && s[i + 1] == '/';
        size_t k = i + 1 + (size_t)closing;
        if (k >= len || !isalpha((unsigned char)s[k])) {
            /* stray "<": it is text */
            if (!skipping) add_text(h, "<", 1);
            i++;
            continue;
        }
        char tag[16];
        size_t tn = 0;
        while (k < len && name_char((unsigned char)s[k])) { if (tn + 1 < sizeof tag) tag[tn++] = (char)tolower((unsigned char)s[k]); k++; }
        tag[tn] = 0;
        /* attributes */
        int na = 0, selfclose = 0;
        while (k < len && s[k] != '>') {
            while (k < len && (isspace((unsigned char)s[k]) || s[k] == '/')) { if (s[k] == '/' && k + 1 < len && s[k + 1] == '>') selfclose = 1; k++; }
            if (k >= len || s[k] == '>') break;
            size_t an = k;
            while (k < len && !isspace((unsigned char)s[k]) && s[k] != '=' && s[k] != '>' && s[k] != '/') k++;
            size_t anl = k - an;
            while (k < len && isspace((unsigned char)s[k])) k++;
            const char *v = NULL;
            size_t vn = 0;
            if (k < len && s[k] == '=') {
                k++;
                while (k < len && isspace((unsigned char)s[k])) k++;
                if (k < len && (s[k] == '"' || s[k] == '\'')) {
                    char q = s[k++];
                    v = s + k;
                    while (k < len && s[k] != q) k++;
                    vn = (size_t)(s + k - v);
                    if (k < len) k++;
                } else {
                    v = s + k;
                    while (k < len && !isspace((unsigned char)s[k]) && s[k] != '>') k++;
                    vn = (size_t)(s + k - v);
                }
            }
            if (!anl) { if (k < len && s[k] != '>') k++; continue; }
            if (na < MAX_ATTRS && anl < sizeof at[0].name && !closing && !skipping) {
                Attr *x = &at[na];
                for (size_t c = 0; c < anl; c++) x->name[c] = (char)tolower((unsigned char)s[an + c]);
                x->name[anl] = 0;
                /* no events (on*) or anything unused */
                if (!strncmp(x->name, "on", 2)) continue;
                x->v = v; x->vn = vn;
                x->dec = v ? decode_attr(v, vn) : calloc(1, 1);
                if (!x->dec) continue;
                if (nkeep == capkeep) { capkeep = capkeep ? capkeep * 2 : 64; char **nk = realloc(keep, capkeep * sizeof *nk); if (!nk) { free(x->dec); continue; } keep = nk; }
                keep[nkeep++] = x->dec;
                na++;
            }
        }
        i = k < len ? k + 1 : len;
        if (skipping) {
            /* only the close of what is being skipped matters (with same-name nesting) */
            Open *top = &h->open[h->nopen - 1];
            if (!strcmp(tag, top->tag)) {
                if (closing) { if (top->list_n > 0) top->list_n--; else { h->nopen--; h->cur = top->is; } }
                else if (!selfclose && !tag_is(tag, T_VOID)) top->list_n++;
            } else if (!closing && !strcmp(top->tag, "head") && !strcmp(tag, "body")) {
                h->nopen--;                         /* unclosed <head> */
                open_tag(h, tag, at, na, selfclose);
            }
            continue;
        }
        if (closing) { close_tag(h, tag); continue; }
        if (tag_is(tag, T_RAW)) {
            /* skip up to </tag> without interpreting anything */
            size_t e = i;
            for (;;) {
                const char *lt = memchr(s + e, '<', len - e);
                if (!lt) { e = len; break; }
                e = (size_t)(lt - s);
                if (e + 2 + tn <= len && s[e + 1] == '/' && !strncasecmp(s + e + 2, tag, tn) && (e + 2 + tn == len || !name_char((unsigned char)s[e + 2 + tn]))) {
                    const char *gt = memchr(s + e, '>', len - e);
                    e = gt ? (size_t)(gt - s) + 1 : len;
                    break;
                }
                e++;
            }
            i = e;
            continue;
        }
        open_tag(h, tag, at, na, selfclose);
    }
    pop_to(h, 0);
    flush_run(h);
    while (h->nfr > 1) close_frame(h);
    jb_raw(&h->out, "]");
    if (h->has_root_bg) color_json(&h->out, "bg", h->root_bg);
    if (h->colors || h->has_root_bg) jb_raw(&h->out, ",\"colors\":1");
    jb_raw(&h->out, ",\"remote\":[");
    for (int r = 0; r < h->nremote; r++) { if (r) jb_raw(&h->out, ","); jb_str(&h->out, h->remote[r]); free(h->remote[r]); }
    jb_raw(&h->out, "],\"links\":[");
    for (int l = 0; l < h->nlinks; l++) {
        if (l) jb_raw(&h->out, ",");
        jb_raw(&h->out, "{\"h\":");
        jb_str(&h->out, h->links[l].href);
        if (h->links[l].warn) {
            jb_raw(&h->out, ",\"w\":");
            jb_str(&h->out, html_link_warning(h->links[l].text.p ? h->links[l].text.p : "", h->links[l].href));
            jb_raw(&h->out, ",\"txt\":");
            jb_str(&h->out, h->links[l].text.p ? h->links[l].text.p : "");
        }
        jb_raw(&h->out, "}");
        free(h->links[l].href);
        free(h->links[l].text.p);
    }
    jb_printf(&h->out, "],\"stats\":{\"remote\":%d,\"trackers\":%d,\"warn\":%d,\"cid\":%d,\"data\":%d,\"nodes\":%d,\"truncated\":%d}}",
              h->st.remote, h->st.trackers, h->st.warn, h->st.cid, h->st.data, h->st.nodes, h->st.truncated);
    free(h->run.p);
    if (stats) *stats = h->st;
    /* data: points into the decoded copies: copy them before freeing */
    if (data && ndata) {
        for (int d = 0; d < h->ndata; d++) {
            char *cp = malloc(data[d].n + 1);
            if (cp) { memcpy(cp, data[d].p, data[d].n); cp[data[d].n] = 0; }
            data[d].p = cp;
            if (!cp) data[d].n = 0;
        }
        *ndata = h->ndata;
    }
    for (size_t k2 = 0; k2 < nkeep; k2++) free(keep[k2]);
    free(keep);
    char *r = h->out.oom ? NULL : h->out.p;
    if (olen) *olen = r ? h->out.n : 0;
    if (!r) jb_free(&h->out);
    free(h);
    return r;
}

char *html_to_text(const char *s, size_t len, size_t *olen) {
    Buf b = { 0 };
    int space = 0;
    int skip = 0;
    char skiptag[16] = "";
    for (size_t i = 0; i < len;) {
        if (s[i] == '<') {
            size_t k = i + 1;
            int closing = k < len && s[k] == '/';
            if (closing) k++;
            char tag[16];
            size_t tn = 0;
            while (k < len && name_char((unsigned char)s[k])) { if (tn + 1 < sizeof tag) tag[tn++] = (char)tolower((unsigned char)s[k]); k++; }
            tag[tn] = 0;
            int comment = i + 3 < len && !memcmp(s + i, "<!--", 4);
            if (comment) {
                size_t e = i + 4;
                while (e + 2 < len && !(s[e] == '-' && s[e + 1] == '-' && s[e + 2] == '>')) e++;
                i = e + 3 < len ? e + 3 : len;
                continue;
            }
            const char *gt = memchr(s + k, '>', len - k);
            i = gt ? (size_t)(gt - s) + 1 : len;
            if (skip) { if (closing && !strcmp(tag, skiptag)) skip = 0; continue; }
            if (!closing && (tag_is(tag, T_RAW) || !strcmp(tag, "head"))) { skip = 1; snprintf(skiptag, sizeof skiptag, "%s", tag); continue; }
            if (!strcmp(tag, "br") || !strcmp(tag, "p") || !strcmp(tag, "div") || !strcmp(tag, "tr") || !strcmp(tag, "li") || (tag[0] == 'h' && isdigit((unsigned char)tag[1])) || !strcmp(tag, "table")) {
                if (b.n && b.p[b.n - 1] != '\n') bput(&b, "\n", 1);
                space = 0;
            } else if (!strcmp(tag, "td") || !strcmp(tag, "th")) space = 1;
            continue;
        }
        if (skip) { i++; continue; }
        uint32_t cp;
        if (s[i] == '&') {
            size_t used;
            cp = entity(s, len, i, &used);
            if (cp && used) i += used; else { cp = '&'; i++; }
        } else cp = next_cp(s, len, &i);
        if (invisible(cp)) continue;
        if (cp == ' ' || cp == '\t' || cp == '\n' || cp == '\r' || cp == 0xa0) { space = 1; continue; }
        if (cp < 0x20) continue;
        if (space && b.n && b.p[b.n - 1] != '\n') bput(&b, " ", 1);
        space = 0;
        char t[4];
        bput(&b, t, (size_t)put_cp(t, cp));
    }
    while (b.n && (b.p[b.n - 1] == '\n' || b.p[b.n - 1] == ' ')) b.p[--b.n] = 0;
    if (!b.p) b.p = calloc(1, 1);
    if (olen) *olen = b.p ? b.n : 0;
    return b.p;
}
