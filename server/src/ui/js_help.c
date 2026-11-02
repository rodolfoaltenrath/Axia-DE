/* js_help.c — lord.help (Help app articles) and lord.time.zoneOffset (Clock app).
 * See docs/SYSTEM.md.
 *
 *   lord.help.list(lang)      → [{ id, title, app, section, order, keywords }] of the .md files in assets/help/<lang>
 *   lord.help.read(lang, id)  → the article text, or null
 *   lord.time.zoneOffset(zone, unixSeconds) → { offset, abbr, dst } or null (our own TZif reader, tzif.c)
 *
 * Read-only and only from LordOS's assets/help folder: the language and the id are validated
 * ([a-z0-9-], no dots or slashes), so nothing outside it can be reached. English (assets/help/en)
 * is the default; the SDK (sdk/help.js) falls back to it when an article is not translated. */
#include "js_help.h"
#include "js_sys.h"
#include "tzif.h"

#include <ctype.h>
#include <dirent.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define HELP_MAX (256 * 1024)           /* one article */

static int valid_part(const char *s, size_t max) {
    size_t n = s ? strlen(s) : 0;
    if (!n || n > max || s[0] == '-') return 0;
    for (; *s; s++) if (!(islower((unsigned char)*s) || isdigit((unsigned char)*s) || *s == '-')) return 0;
    return 1;
}

static char *read_article(const char *lang, const char *id, size_t *len) {
    const char *assets = js_sys_assets();
    if (!assets[0] || !valid_part(lang, 12) || !valid_part(id, 48)) return NULL;
    char path[PATH_MAX + 128];
    snprintf(path, sizeof path, "%s/help/%s/%s.md", assets, lang, id);
    int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return NULL;
    struct stat st;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size > HELP_MAX) { close(fd); return NULL; }
    char *buf = malloc((size_t)st.st_size + 1);
    size_t got = 0;
    while (buf && got < (size_t)st.st_size) {
        ssize_t r = read(fd, buf + got, (size_t)st.st_size - got);
        if (r <= 0) break;
        got += (size_t)r;
    }
    close(fd);
    if (!buf) return NULL;
    buf[got] = 0;
    if (len) *len = got;
    return buf;
}

/* value of "key: value" in the --- … --- header */
static void front(const char *txt, const char *key, char *out, size_t n) {
    out[0] = 0;
    if (strncmp(txt, "---\n", 4)) return;
    const char *end = strstr(txt + 4, "\n---");
    size_t kl = strlen(key);
    for (const char *l = txt + 4; l && end && l < end; l = strchr(l, '\n'), l = l ? l + 1 : NULL) {
        if (!strncmp(l, key, kl) && l[kl] == ':') {
            const char *v = l + kl + 1;
            while (*v == ' ') v++;
            size_t k = 0;
            while (v[k] && v[k] != '\n' && k + 1 < n) { out[k] = v[k]; k++; }
            out[k] = 0;
            return;
        }
    }
}

static JSValue f_help_list(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val; (void)argc;
    JSValue arr = JS_NewArray(c);
    const char *lang = JS_ToCString(c, argv[0]);
    const char *assets = js_sys_assets();
    if (lang && valid_part(lang, 12) && assets[0]) {
        char dir[PATH_MAX + 64];
        snprintf(dir, sizeof dir, "%s/help/%s", assets, lang);
        DIR *d = opendir(dir);
        struct dirent *e;
        uint32_t i = 0;
        while (d && (e = readdir(d)) && i < 500) {
            size_t nl = strlen(e->d_name);
            if (nl < 4 || nl > 51 || strcmp(e->d_name + nl - 3, ".md")) continue;
            char id[64];
            snprintf(id, sizeof id, "%.*s", (int)(nl - 3), e->d_name);
            char *txt = read_article(lang, id, NULL);
            if (!txt) continue;
            char v[256];
            JSValue o = JS_NewObject(c);
            JS_SetPropertyStr(c, o, "id", JS_NewString(c, id));
            front(txt, "title", v, sizeof v); JS_SetPropertyStr(c, o, "title", JS_NewString(c, v[0] ? v : id));
            front(txt, "app", v, sizeof v); JS_SetPropertyStr(c, o, "app", JS_NewString(c, v));
            front(txt, "section", v, sizeof v); JS_SetPropertyStr(c, o, "section", JS_NewString(c, v));
            front(txt, "order", v, sizeof v); JS_SetPropertyStr(c, o, "order", JS_NewInt32(c, atoi(v)));
            front(txt, "keywords", v, sizeof v); JS_SetPropertyStr(c, o, "keywords", JS_NewString(c, v));
            JS_SetPropertyUint32(c, arr, i++, o);
            free(txt);
        }
        if (d) closedir(d);
    }
    if (lang) JS_FreeCString(c, lang);
    return arr;
}

static JSValue f_help_read(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val; (void)argc;
    const char *lang = JS_ToCString(c, argv[0]), *id = JS_ToCString(c, argv[1]);
    JSValue r = JS_NULL;
    size_t len = 0;
    char *txt = lang && id ? read_article(lang, id, &len) : NULL;
    if (txt) { r = JS_NewStringLen(c, txt, len); free(txt); }
    if (lang) JS_FreeCString(c, lang);
    if (id) JS_FreeCString(c, id);
    return r;
}

static JSValue f_zone_offset(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    const char *zone = JS_ToCString(c, argv[0]);
    double t = 0;
    if (argc > 1) JS_ToFloat64(c, &t, argv[1]);
    JSValue r = JS_NULL;
    TzInfo z;
    if (zone && t > -1e12 && t < 1e12 && !tz_zone_offset(zone, (int64_t)t, &z)) {
        r = JS_NewObject(c);
        JS_SetPropertyStr(c, r, "offset", JS_NewInt32(c, z.utoff));
        JS_SetPropertyStr(c, r, "abbr", JS_NewString(c, z.abbr));
        JS_SetPropertyStr(c, r, "dst", JS_NewBool(c, z.isdst));
    }
    if (zone) JS_FreeCString(c, zone);
    return r;
}

static const JSCFunctionListEntry help_funcs[] = {
    JS_CFUNC_DEF("list", 1, f_help_list),
    JS_CFUNC_DEF("read", 2, f_help_read),
};

void js_help_register(JSContext *c, JSValue lord) {
    JSValue o = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, o, help_funcs, sizeof help_funcs / sizeof help_funcs[0]);
    JS_SetPropertyStr(c, lord, "help", o);
    /* lord.time was already created by js_sys_register: zoneOffset is added to it */
    JSValue time = JS_GetPropertyStr(c, lord, "time");
    if (JS_IsObject(time)) JS_SetPropertyStr(c, time, "zoneOffset", JS_NewCFunction(c, f_zone_offset, "zoneOffset", 2));
    JS_FreeValue(c, time);
}
