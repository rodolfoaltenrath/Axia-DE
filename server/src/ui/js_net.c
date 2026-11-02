/* js_net.c — lord.net: network state for the Network and VPN app (see docs/NETWORK.md).
 *
 * Everything is read-only and without root (rtnetlink and public files in /etc/lordos/net). Anything
 * that changes something goes through the privileged helper: the SDK's admin.run('net-…'). Kernel
 * changes (links, addresses, routes) arrive at __lordNet(what), with what = link | addr | route. */
#include "js_net.h"
#include "../net/net.h"
#include "js_err.h"

#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static JSValue json_or_null(JSContext *c, char *j, const char *what) {
    if (!j) return JS_NULL;
    JSValue v = JS_ParseJSON(c, j, strlen(j), what);
    free(j);
    return v;
}

static JSValue f_interfaces(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return json_or_null(c, net_interfaces_json(), "<net.interfaces>");
}

static JSValue f_routes(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return json_or_null(c, net_routes_json(), "<net.routes>");
}

static JSValue f_dns(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return json_or_null(c, net_dns_json(), "<net.dns>");
}

static JSValue f_vpn_list(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return json_or_null(c, net_vpn_list_json(), "<net.vpnList>");
}

static JSValue f_wifi_known(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return json_or_null(c, net_wifi_known_json(), "<net.wifiKnown>");
}

/* lord.net.config(interface): the configuration saved by LordOS, or null. */
static JSValue f_config(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc < 1) return JS_NULL;
    const char *n = JS_ToCString(c, argv[0]);
    if (!n) return JS_EXCEPTION;
    char *j = net_if_config_json(n);
    JS_FreeCString(c, n);
    return json_or_null(c, j, "<net.config>");
}

/* lord.net.prepareOvpn(path): the .ovpn (inside $HOME) with its files embedded, ready
 * for admin.run('net-vpn-import', 'openvpn\n<name>\n' + text). Throws an error if something is missing. */
static JSValue f_prepare_ovpn(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t;
    if (argc < 1) return js_throw_code(c, "admin.invalidArgument", NULL);
    const char *p = JS_ToCString(c, argv[0]);
    if (!p) return JS_EXCEPTION;
    char real[PATH_MAX], home[PATH_MAX];
    const char *h = getenv("HOME");
    int ok = h && realpath(h, home) && realpath(p, real);
    size_t hl = ok ? strlen(home) : 0;
    ok = ok && !strncmp(real, home, hl) && real[hl] == '/';
    JS_FreeCString(c, p);
    if (!ok) return js_throw_code(c, "fs.outsideHome", NULL);
    char err[256] = "";
    char *txt = net_ovpn_inline(real, err, sizeof err);
    if (!txt) return js_throw_line(c, err);
    JSValue v = JS_NewString(c, txt);
    free(txt);
    return v;
}

/* lord.net.vpnTypes(): [{type, available, optional, package, insecure}] — the optional ones are
 * enabled with admin.run('net-vpn-enable', 'openconnect'|'fortinet'). */
static JSValue f_vpn_types(JSContext *c, JSValueConst t, int argc, JSValueConst *argv) {
    (void)t; (void)argc; (void)argv;
    return json_or_null(c, net_vpn_types_json(), "<net.vpnTypes>");
}

static const JSCFunctionListEntry net_funcs[] = {
    JS_CFUNC_DEF("vpnTypes", 0, f_vpn_types),
    JS_CFUNC_DEF("interfaces", 0, f_interfaces),
    JS_CFUNC_DEF("routes", 0, f_routes),
    JS_CFUNC_DEF("dns", 0, f_dns),
    JS_CFUNC_DEF("vpnList", 0, f_vpn_list),
    JS_CFUNC_DEF("wifiKnown", 0, f_wifi_known),
    JS_CFUNC_DEF("config", 1, f_config),
    JS_CFUNC_DEF("prepareOvpn", 1, f_prepare_ovpn),
};

void js_net_register(JSContext *c, JSValue lord) {
    JSValue o = JS_NewObject(c);
    JS_SetPropertyFunctionList(c, o, net_funcs, sizeof net_funcs / sizeof net_funcs[0]);
    JS_SetPropertyStr(c, lord, "net", o);
}
