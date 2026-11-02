/* h_font.c — fonts in the helper (FreeType + cairo): data of a .ttf/.otf/.ttc, a sample with
 * a text and sizes, and the character table. The Fonts app and Quick Preview use it: a
 * broken or malicious font (for example, one downloaded from the internet before installing it) can only
 * bring down this process. */
#include "helper.h"

#include <cairo.h>
#include <cairo-ft.h>
#include <ft2build.h>
#include FT_FREETYPE_H

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define FONT_FILE_MAX (64 << 20)

static FT_Library ft;

typedef struct { FT_Face face; void *data; } Holder;
static const cairo_user_data_key_t holder_key;
static void holder_free(void *p) {
    Holder *h = p;
    FT_Done_Face(h->face);
    free(h->data);
    free(h);
}

/* the last font opened (Fonts asks for several samples in a row from the same one) */
static struct { dev_t dev; ino_t ino; time_t mtime; off_t size; int index; FT_Face face; cairo_font_face_t *cf; } cur;

static FT_Face load(int fd, int index, cairo_font_face_t **cf, const char **why) {
    *why = "invalid";
    struct stat st;
    if (fd < 0 || fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size <= 0 || st.st_size > FONT_FILE_MAX) return NULL;
    if (cur.face && cur.dev == st.st_dev && cur.ino == st.st_ino && cur.mtime == st.st_mtime && cur.size == st.st_size && cur.index == index) {
        *cf = cur.cf;
        return cur.face;
    }
    if (!ft && FT_Init_FreeType(&ft)) return NULL;
    void *data = malloc((size_t)st.st_size);
    if (!data) return NULL;
    off_t off = 0;
    while (off < st.st_size) {
        ssize_t r = pread(fd, (char *)data + off, (size_t)(st.st_size - off), off);
        if (r <= 0) { free(data); return NULL; }
        off += r;
    }
    FT_Face face;
    if (FT_New_Memory_Face(ft, data, (FT_Long)st.st_size, index, &face)) { free(data); return NULL; }
    Holder *h = calloc(1, sizeof *h);
    if (!h) { FT_Done_Face(face); free(data); return NULL; }
    h->face = face;
    h->data = data;
    cairo_font_face_t *c = cairo_ft_font_face_create_for_ft_face(face, 0);
    if (cairo_font_face_set_user_data(c, &holder_key, h, holder_free)) { cairo_font_face_destroy(c); holder_free(h); return NULL; }
    if (cur.cf) cairo_font_face_destroy(cur.cf);
    cur.dev = st.st_dev; cur.ino = st.st_ino; cur.mtime = st.st_mtime; cur.size = st.st_size; cur.index = index;
    cur.face = face;
    cur.cf = c;
    *cf = c;
    return face;
}

static void color_of(const JVal *a, const char *key, double rgba[4], const char *def) {
    const char *s = json_str(a, key);
    if (!s || s[0] != '#') s = def;
    unsigned r = 0, g = 0, b = 0, al = 255;
    size_t n = strlen(s);
    if (n == 7) sscanf(s + 1, "%2x%2x%2x", &r, &g, &b);
    else if (n == 9) sscanf(s + 1, "%2x%2x%2x%2x", &r, &g, &b, &al);
    rgba[0] = r / 255.0; rgba[1] = g / 255.0; rgba[2] = b / 255.0; rgba[3] = al / 255.0;
}

static int arg_fd(const DocMsg *m) { return m->nfds > 0 ? m->fds[0] : -1; }

static void op_info(const DocMsg *m, const JVal *a) {
    cairo_font_face_t *cf;
    const char *why;
    int index = (int)json_num(a, "index", 0);
    if (index < 0 || index > 255) index = 0;
    FT_Face f = load(arg_fd(m), index, &cf, &why);
    if (!f) { reply_err(m->h.id, why, ""); return; }
    JBuf b;
    jb_init(&b);
    jb_raw(&b, "{\"family\":");
    jb_str(&b, f->family_name ? f->family_name : "");
    jb_raw(&b, ",\"style\":");
    jb_str(&b, f->style_name ? f->style_name : "");
    const char *ps = FT_Get_Postscript_Name(f);
    jb_raw(&b, ",\"ps\":");
    jb_str(&b, ps ? ps : "");
    jb_printf(&b, ",\"faces\":%ld,\"glyphs\":%ld,\"mono\":%s,\"color\":%s,\"scalable\":%s,\"bold\":%s,\"italic\":%s,\"ranges\":[",
              (long)f->num_faces, (long)f->num_glyphs, FT_IS_FIXED_WIDTH(f) ? "true" : "false", FT_HAS_COLOR(f) ? "true" : "false",
              FT_IS_SCALABLE(f) ? "true" : "false", (f->style_flags & FT_STYLE_FLAG_BOLD) ? "true" : "false",
              (f->style_flags & FT_STYLE_FLAG_ITALIC) ? "true" : "false");
    /* characters it has, as ranges [from, to] */
    FT_UInt gi;
    FT_ULong c = FT_Get_First_Char(f, &gi), start = c, prev = c;
    long chars = 0;
    int nr = 0;
    if (FT_Select_Charmap(f, FT_ENCODING_UNICODE) == 0) c = FT_Get_First_Char(f, &gi), start = prev = c;
    while (gi && nr < 20000) {
        chars++;
        FT_ULong next = FT_Get_Next_Char(f, c, &gi);
        if (!gi || next != prev + 1) {
            if (nr++) jb_raw(&b, ",");
            jb_printf(&b, "[%lu,%lu]", (unsigned long)start, (unsigned long)prev);
            start = next;
        }
        prev = c = next;
    }
    jb_printf(&b, "],\"chars\":%ld}", chars);
    reply_json(DR_DONE, m->h.id, &b);
}

/* samples: each line {text, size} (px), fitted to the width; returns the image */
static void op_render(const DocMsg *m, const JVal *a) {
    cairo_font_face_t *cf;
    const char *why;
    int index = (int)json_num(a, "index", 0);
    FT_Face f = load(arg_fd(m), index < 0 || index > 255 ? 0 : index, &cf, &why);
    if (!f) { reply_err(m->h.id, why, ""); return; }
    int W = (int)json_num(a, "width", 600);
    if (W < 16) W = 16;
    if (W > 4096) W = 4096;
    double fg[4], bg[4];
    color_of(a, "fg", fg, "#000000");
    color_of(a, "bg", bg, "#ffffff00");
    const JVal *lines = json_get(a, "lines");
    if (!lines || lines->type != J_ARR || lines->len > 64) { reply_err(m->h.id, "bad-request", "lines"); return; }
    /* first pass: measure with a small surface */
    cairo_surface_t *probe = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 1, 1);
    cairo_t *pc = cairo_create(probe);
    cairo_set_font_face(pc, cf);
    double H = 8;
    for (const JVal *l = lines->kid; l; l = l->next) {
        double size = json_num(l, "size", 24);
        if (!(size >= 4 && size <= 400)) size = 24;
        const char *text = json_str(l, "text");
        if (!text) continue;
        cairo_set_font_size(pc, size);
        cairo_font_extents_t fe;
        cairo_font_extents(pc, &fe);
        cairo_text_extents_t te;
        cairo_text_extents(pc, text, &te);
        int rows = te.x_advance > W - 16 ? (int)ceil(te.x_advance / (W - 16)) : 1;
        if (rows > 8) rows = 8;
        H += fe.height * rows + size * 0.25;
    }
    cairo_destroy(pc);
    cairo_surface_destroy(probe);
    int Hi = (int)ceil(H);
    if (Hi > 4096) Hi = 4096;
    cairo_surface_t *s = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, W, Hi);
    cairo_t *cr = cairo_create(s);
    cairo_set_source_rgba(cr, bg[0], bg[1], bg[2], bg[3]);
    cairo_set_operator(cr, CAIRO_OPERATOR_SOURCE);
    cairo_paint(cr);
    cairo_set_operator(cr, CAIRO_OPERATOR_OVER);
    cairo_set_font_face(cr, cf);
    cairo_set_source_rgba(cr, fg[0], fg[1], fg[2], fg[3]);
    double y = 4;
    for (const JVal *l = lines->kid; l; l = l->next) {
        double size = json_num(l, "size", 24);
        if (!(size >= 4 && size <= 400)) size = 24;
        const char *text = json_str(l, "text");
        if (!text) continue;
        cairo_set_font_size(cr, size);
        cairo_font_extents_t fe;
        cairo_font_extents(cr, &fe);
        /* break by words at the width */
        const char *p = text;
        int rows = 0;
        while (*p && rows < 8) {
            const char *end = p + strlen(p), *cut = end;
            char tmp[4096];
            size_t len = (size_t)(end - p);
            if (len >= sizeof tmp) len = sizeof tmp - 1;
            memcpy(tmp, p, len);
            tmp[len] = 0;
            cairo_text_extents_t te;
            cairo_text_extents(cr, tmp, &te);
            while (te.x_advance > W - 16 && len > 1) {
                char *sp = strrchr(tmp, ' ');
                if (sp && sp != tmp) *sp = 0;
                else {
                    size_t k = strlen(tmp);
                    do k--; while (k > 1 && ((unsigned char)tmp[k] & 0xc0) == 0x80);
                    tmp[k] = 0;
                }
                len = strlen(tmp);
                cairo_text_extents(cr, tmp, &te);
            }
            cut = p + len;
            cairo_move_to(cr, 8, y + fe.ascent);
            cairo_show_text(cr, tmp);
            y += fe.height;
            rows++;
            p = cut;
            while (*p == ' ') p++;
        }
        y += size * 0.25;
    }
    cairo_destroy(cr);
    cairo_surface_flush(s);
    reply_image(m->h.id, (const uint32_t *)cairo_image_surface_get_data(s), W, Hi, NULL);
    cairo_surface_destroy(s);
}

static int utf8_put(char *o, uint32_t c) {
    if (c < 0x80) { o[0] = (char)c; return 1; }
    if (c < 0x800) { o[0] = (char)(0xc0 | c >> 6); o[1] = (char)(0x80 | (c & 63)); return 2; }
    if (c < 0x10000) { o[0] = (char)(0xe0 | c >> 12); o[1] = (char)(0x80 | (c >> 6 & 63)); o[2] = (char)(0x80 | (c & 63)); return 3; }
    o[0] = (char)(0xf0 | c >> 18); o[1] = (char)(0x80 | (c >> 12 & 63)); o[2] = (char)(0x80 | (c >> 6 & 63)); o[3] = (char)(0x80 | (c & 63));
    return 4;
}

/* table: cps[] in a grid of cols columns and cells of cell px; says which ones the font has */
static void op_grid(const DocMsg *m, const JVal *a) {
    cairo_font_face_t *cf;
    const char *why;
    int index = (int)json_num(a, "index", 0);
    FT_Face f = load(arg_fd(m), index < 0 || index > 255 ? 0 : index, &cf, &why);
    if (!f) { reply_err(m->h.id, why, ""); return; }
    const JVal *cps = json_get(a, "cps");
    if (!cps || cps->type != J_ARR || !cps->len || cps->len > 1024) { reply_err(m->h.id, "bad-request", "cps"); return; }
    int cell = (int)json_num(a, "cell", 40), cols = (int)json_num(a, "cols", 16);
    if (cell < 12 || cell > 160) cell = 40;
    if (cols < 1 || cols > 64) cols = 16;
    int rows = ((int)cps->len + cols - 1) / cols;
    int W = cell * cols, H = cell * rows;
    if (H > DOC_IMG_MAX) { reply_err(m->h.id, "bad-request", "size"); return; }
    double fg[4];
    color_of(a, "fg", fg, "#000000");
    cairo_surface_t *s = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, W, H);
    cairo_t *cr = cairo_create(s);
    cairo_set_font_face(cr, cf);
    cairo_set_font_size(cr, cell * 0.6);
    cairo_set_source_rgba(cr, fg[0], fg[1], fg[2], fg[3]);
    FT_Select_Charmap(f, FT_ENCODING_UNICODE);
    JBuf pres;
    jb_init(&pres);
    jb_raw(&pres, ",\"present\":[");
    int i = 0;
    for (const JVal *e = cps->kid; e; e = e->next, i++) {
        uint32_t c = e->type == J_NUM && e->num >= 0 && e->num <= 0x10ffff ? (uint32_t)e->num : 0;
        int has = c && FT_Get_Char_Index(f, c) != 0;
        jb_raw(&pres, i ? (has ? ",1" : ",0") : (has ? "1" : "0"));
        if (!has) continue;
        char u[8];
        int k = utf8_put(u, c);
        u[k] = 0;
        cairo_text_extents_t te;
        cairo_text_extents(cr, u, &te);
        double x = (i % cols) * cell + (cell - te.width) / 2 - te.x_bearing;
        double y = (i / cols) * cell + (cell - te.height) / 2 - te.y_bearing;
        cairo_move_to(cr, x, y);
        cairo_show_text(cr, u);
    }
    jb_raw(&pres, "]");
    cairo_destroy(cr);
    cairo_surface_flush(s);
    reply_image(m->h.id, (const uint32_t *)cairo_image_surface_get_data(s), W, H, pres.p);
    jb_free(&pres);
    cairo_surface_destroy(s);
}

int font_handle(const DocMsg *m, const JVal *a) {
    switch (m->h.op) {
    case DQ_FONT_INFO: op_info(m, a); return 1;
    case DQ_FONT_RENDER: op_render(m, a); return 1;
    case DQ_FONT_GRID: op_grid(m, a); return 1;
    case DQ_PDF_THUMB: pdf_thumb(m, a); return 1;
    case DQ_WARM: pdf_warm(m->h.id); return 1;
    default: return make_handle(m, a);        /* the same session also decodes images */
    }
}
