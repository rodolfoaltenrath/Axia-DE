/* js_print.c — lord.print: printers (CUPS over IPP on its local socket, our own client) and
 * eSCL scanners on the network (see server/src/print and docs/PRINTING.md). All asynchronous: each
 * function returns an id and the result arrives with poll() → [{ id, kind, ok, data }].
 *
 *   printers()                       CUPS printers and the default one
 *   jobs()                           unfinished jobs (from all users)
 *   cancel(job)
 *   submit(printer, source, options)     source: a lord.doc blob (built PDF) or the path of a
 *                                    PDF; options { name, copies, pages, sides, color, media, orientation }
 *   add(name, uri, info, location)   driverless printing (IPP Everywhere) printer
 *   remove(name), setDefault(name)
 *   discover()                       printers and scanners on the network via mDNS
 *   scanCaps(ip, port, rs), scan(ip, port, rs, settings)
 */
#include "js_print.h"
#include "js_err.h"
#include "../print/print.h"
#include "../print/cups.h"
#include "../print/escl.h"
#include "../print/phttp.h"
#include "../doc/doc.h"
#include "../doc/wire.h"
#include "../sec/json.h"
#include "../fsops.h"

#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static JSValue task(JSContext *c, const char *kind, JBuf *b, int docfd, size_t doclen) {
    if (b && b->oom) { jb_free(b); if (docfd >= 0) close(docfd); return JS_ThrowOutOfMemory(c); }
    int id = print_task(kind, b ? b->p : NULL, b ? b->n : 0, docfd, doclen);
    if (b) jb_free(b);
    return JS_NewInt32(c, id);
}

static const char *str_arg(JSContext *c, int argc, JSValueConst *argv, int i) {
    return i < argc && JS_IsString(argv[i]) ? JS_ToCString(c, argv[i]) : NULL;
}

static JSValue f_printers(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) { (void)t; (void)argc; (void)argv; return task(c, "printers", NULL, -1, 0); }
static JSValue f_jobs(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) { (void)t; (void)argc; (void)argv; return task(c, "jobs", NULL, -1, 0); }
static JSValue f_discover(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) { (void)t; (void)argc; (void)argv; return task(c, "discover", NULL, -1, 0); }

static JSValue f_cancel(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    int32_t job = 0;
    if (argc < 1 || JS_ToInt32(c, &job, argv[0]) || job <= 0) return js_throw_code(c, "sys.badArgument", NULL);
    JBuf b;
    jb_init(&b);
    jb_printf(&b, "{\"job\":%d}", job);
    return task(c, "cancel", &b, -1, 0);
}

/* printer name + JSON text of the options (from JSON.stringify) */
static JSValue f_submit(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    const char *printer = str_arg(c, argc, argv, 0);
    if (!printer || !cups_valid_name(printer)) { if (printer) JS_FreeCString(c, printer); return js_throw_code(c, "print.badPrinter", NULL); }
    int fd = -1;
    size_t len = 0;
    if (argc > 1 && JS_IsNumber(argv[1])) {
        int32_t h = 0;
        JS_ToInt32(c, &h, argv[1]);
        fd = doc_blob_dup(h, &len);
    } else if (argc > 1 && JS_IsString(argv[1])) {
        const char *p = JS_ToCString(c, argv[1]);
        char full[PATH_MAX * 2], real[PATH_MAX];
        const char *home = getenv("HOME");
        if (p && p[0] == '/') snprintf(full, sizeof full, "%s", p);
        else if (p && home) snprintf(full, sizeof full, "%s/%s", home, p);
        else full[0] = 0;
        if (p) JS_FreeCString(c, p);
        if (full[0] && realpath(full, real) && fsops_allowed(real)) {
            fd = open(real, O_RDONLY | O_CLOEXEC | O_NOCTTY | O_NONBLOCK);
            struct stat st;
            char magic[5] = "";
            if (fd >= 0 && (fstat(fd, &st) || !S_ISREG(st.st_mode) || pread(fd, magic, 5, 0) != 5 || memcmp(magic, "%PDF-", 5))) { close(fd); fd = -1; }
            else if (fd >= 0) { len = (size_t)st.st_size; fcntl(fd, F_SETFL, 0); }
        }
    }
    if (fd < 0) { JS_FreeCString(c, printer); return js_throw_code(c, "print.badDocument", NULL); }
    const char *opts = str_arg(c, argc, argv, 2);
    JBuf b;
    jb_init(&b);
    jb_raw(&b, "{\"printer\":");
    jb_str(&b, printer);
    JS_FreeCString(c, printer);
    /* the options are fully validated in cups_submit; here only that it is a JSON object */
    JDoc d = { 0 };
    if (opts && !json_parse(opts, strlen(opts), &d, NULL, 0) && d.root->type == J_OBJ) { jb_raw(&b, ",\"options\":"); jb_raw(&b, opts); }
    json_free(&d);
    if (opts) JS_FreeCString(c, opts);
    jb_raw(&b, "}");
    return task(c, "submit", &b, fd, len);
}

static JSValue name_task(JSContext *c, int argc, JSValueConst *argv, const char *kind) {
    const char *n = str_arg(c, argc, argv, 0);
    if (!n || !cups_valid_name(n)) { if (n) JS_FreeCString(c, n); return js_throw_code(c, "print.badPrinter", NULL); }
    JBuf b;
    jb_init(&b);
    jb_raw(&b, "{\"name\":");
    jb_str(&b, n);
    jb_raw(&b, "}");
    JS_FreeCString(c, n);
    return task(c, kind, &b, -1, 0);
}

static JSValue f_remove(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) { (void)t; return name_task(c, argc, argv, "remove"); }
static JSValue f_default(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) { (void)t; return name_task(c, argc, argv, "default"); }

static JSValue f_add(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    const char *n = str_arg(c, argc, argv, 0), *u = str_arg(c, argc, argv, 1), *i = str_arg(c, argc, argv, 2), *l = str_arg(c, argc, argv, 3);
    int ok = n && u && cups_valid_name(n) && cups_valid_uri(u) && cups_valid_text(i, 127) && cups_valid_text(l, 127);
    JBuf b;
    jb_init(&b);
    if (ok) {
        jb_raw(&b, "{\"name\":"); jb_str(&b, n);
        jb_raw(&b, ",\"uri\":"); jb_str(&b, u);
        jb_raw(&b, ",\"info\":"); jb_str(&b, i ? i : "");
        jb_raw(&b, ",\"location\":"); jb_str(&b, l ? l : "");
        jb_raw(&b, "}");
    }
    if (n) JS_FreeCString(c, n);
    if (u) JS_FreeCString(c, u);
    if (i) JS_FreeCString(c, i);
    if (l) JS_FreeCString(c, l);
    if (!ok) { jb_free(&b); return js_throw_code(c, "print.badAddress", NULL); }
    return task(c, "add", &b, -1, 0);
}

/* ip (LAN IPv4), port, rs */
static int scanner_args(JSContext *c, int argc, JSValueConst *argv, JBuf *b) {
    const char *ip = str_arg(c, argc, argv, 0), *rs = str_arg(c, argc, argv, 2);
    int32_t port = 0;
    if (argc > 1) JS_ToInt32(c, &port, argv[1]);
    int ok = ip && phttp_lan_ipv4(ip) && port > 0 && port < 65536 && (!rs || escl_valid_rs(rs));
    if (ok) {
        jb_raw(b, "{\"addr\":");
        jb_str(b, ip);
        jb_printf(b, ",\"port\":%d,\"rs\":", port);
        jb_str(b, rs ? rs : "eSCL");
    }
    if (ip) JS_FreeCString(c, ip);
    if (rs) JS_FreeCString(c, rs);
    return ok ? 0 : -1;
}

static JSValue f_scan_caps(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    JBuf b;
    jb_init(&b);
    if (scanner_args(c, argc, argv, &b)) { jb_free(&b); return js_throw_code(c, "print.badScanner", NULL); }
    jb_raw(&b, "}");
    return task(c, "scancaps", &b, -1, 0);
}

static JSValue f_scan(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    JBuf b;
    jb_init(&b);
    if (scanner_args(c, argc, argv, &b)) { jb_free(&b); return js_throw_code(c, "print.badScanner", NULL); }
    const char *s = str_arg(c, argc, argv, 3);
    JDoc d = { 0 };
    if (s && !json_parse(s, strlen(s), &d, NULL, 0) && d.root->type == J_OBJ) { jb_raw(&b, ",\"settings\":"); jb_raw(&b, s); }
    json_free(&d);
    if (s) JS_FreeCString(c, s);
    jb_raw(&b, "}");
    return task(c, "scan", &b, -1, 0);
}

static JSValue f_poll(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    char *j = print_poll();
    if (!j) return JS_NULL;
    JSValue v = JS_ParseJSON(c, j, strlen(j), "<print>");
    free(j);
    return v;
}

static JSValue f_pending(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return JS_NewInt32(c, print_pending());
}

static JSValue f_cups(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return JS_NewBool(c, access(cups_socket, F_OK) == 0);
}

static const JSCFunctionListEntry print_funcs[] = {
    JS_CFUNC_DEF("printers", 0, f_printers),
    JS_CFUNC_DEF("jobs", 0, f_jobs),
    JS_CFUNC_DEF("cancel", 1, f_cancel),
    JS_CFUNC_DEF("submit", 3, f_submit),
    JS_CFUNC_DEF("add", 4, f_add),
    JS_CFUNC_DEF("remove", 1, f_remove),
    JS_CFUNC_DEF("setDefault", 1, f_default),
    JS_CFUNC_DEF("discover", 0, f_discover),
    JS_CFUNC_DEF("scanCaps", 3, f_scan_caps),
    JS_CFUNC_DEF("scan", 4, f_scan),
    JS_CFUNC_DEF("poll", 0, f_poll),
    JS_CFUNC_DEF("pending", 0, f_pending),
    JS_CFUNC_DEF("hasCups", 0, f_cups),
};

void js_print_register(JSContext *c, JSValue lord) {
    JSValue o = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, o, print_funcs, sizeof print_funcs / sizeof print_funcs[0]);
    JS_SetPropertyStr(c, lord, "print", o);
}
