/* ppd.c — power profiles (see ppd.h). */
#include "ppd.h"
#include "jbuf.h"
#include "sysbus.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static const char *const names[2] = { "org.freedesktop.UPower.PowerProfiles", "net.hadess.PowerProfiles" };
static const char *const paths[2] = { "/org/freedesktop/UPower/PowerProfiles", "/net/hadess/PowerProfiles" };
static PpdEvent emit;
static int which = -1;                  /* index in names of the daemon on the bus */
static char active[24], degraded[48];
static char profiles[3][24];
static int nprofiles;

int ppd_profile_valid(const char *p) { return p && (!strcmp(p, "performance") || !strcmp(p, "balanced") || !strcmp(p, "power-saver")); }

static void changed(void) {
    char *j = ppd_state_json();
    if (j && emit) emit("ppd", j);
    free(j);
}

static void read_props(DRd *r) {
    size_t end = dr_arr(r, 8);
    while (!r->err && dr_more(r, end)) {
        dr_struct(r);
        const char *k = dr_str(r, NULL), *sig = dr_variant(r);
        if (r->err || !k || !sig) break;
        if (!strcmp(k, "ActiveProfile") && !strcmp(sig, "s")) { const char *v = dr_str(r, NULL); if (v && ppd_profile_valid(v)) snprintf(active, sizeof active, "%s", v); }
        else if (!strcmp(k, "PerformanceDegraded") && !strcmp(sig, "s")) { const char *v = dr_str(r, NULL); snprintf(degraded, sizeof degraded, "%s", v ? v : ""); }
        else if (!strcmp(k, "Profiles") && !strcmp(sig, "aa{sv}")) {
            nprofiles = 0;
            size_t e1 = dr_arr(r, 4);               /* elements are arrays: 4-aligned */
            while (!r->err && dr_more(r, e1)) {
                size_t e2 = dr_arr(r, 8);
                while (!r->err && dr_more(r, e2)) {
                    dr_struct(r);
                    const char *pk = dr_str(r, NULL), *ps = dr_variant(r);
                    if (r->err || !pk || !ps) break;
                    if (!strcmp(pk, "Profile") && !strcmp(ps, "s")) {
                        const char *v = dr_str(r, NULL);
                        if (v && ppd_profile_valid(v) && nprofiles < 3) snprintf(profiles[nprofiles++], sizeof profiles[0], "%s", v);
                    } else dr_skip(r, ps);
                }
            }
        } else dr_skip(r, sig);
    }
}

static void got_all(const DMsg *m, void *ctx) {
    (void)ctx;
    if (m && m->type == DBUS_RETURN && m->sig && !strcmp(m->sig, "a{sv}")) {
        DRd r;
        dr_init(&r, m);
        read_props(&r);
    }
    changed();
}

static void get_all(void) {
    if (which < 0) return;
    DBuf b;
    dw_init(&b);
    dw_str(&b, names[which]);
    sysbus_call(names[which], paths[which], "org.freedesktop.DBus.Properties", "GetAll", "s", &b, got_all, NULL);
    dw_free(&b);
}

static void on_owner(const char *name, const char *owner) {
    int i = !strcmp(name, names[0]) ? 0 : 1;
    if (owner && owner[0]) { if (which < 0 || i < which) which = i; get_all(); return; }
    if (which == i) { which = -1; nprofiles = 0; active[0] = degraded[0] = 0; changed(); }
}

static void on_signal(const DMsg *m) {
    if (which < 0 || !m->member || strcmp(m->member, "PropertiesChanged") || !m->path || strcmp(m->path, paths[which])) return;
    get_all();
}

void ppd_start(PpdEvent fn) {
    emit = fn;
    if (!sysbus_up()) return;
    sysbus_on_signal(on_signal);
    for (int i = 0; i < 2; i++) {
        char rule[200];
        snprintf(rule, sizeof rule, "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='%s'", paths[i]);
        sysbus_add_match(rule);
        sysbus_watch_name(names[i], on_owner);
    }
}

static void governor(char *out, size_t n) {
    out[0] = 0;
    int fd = open("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor", O_RDONLY | O_CLOEXEC);
    if (fd < 0) return;
    ssize_t r = read(fd, out, n - 1);
    close(fd);
    if (r > 0) { out[r] = 0; out[strcspn(out, "\n")] = 0; } else out[0] = 0;
}

char *ppd_state_json(void) {
    char gov[32];
    governor(gov, sizeof gov);
    HBuf b = { 0 };
    hb_printf(&b, "{\"daemon\":%s,\"active\":", which >= 0 ? "true" : "false");
    const char *act = active;
    if (which < 0) act = !strcmp(gov, "performance") ? "performance" : !strcmp(gov, "powersave") ? "power-saver" : gov[0] ? "balanced" : "";
    hb_str(&b, act);
    hb_printf(&b, ",\"degraded\":");
    hb_str(&b, degraded);
    hb_printf(&b, ",\"governor\":");
    hb_str(&b, gov);
    hb_printf(&b, ",\"profiles\":[");
    if (which >= 0) {
        for (int i = 0; i < nprofiles; i++) { if (i) hb_printf(&b, ","); hb_str(&b, profiles[i]); }
    } else if (gov[0]) hb_printf(&b, "\"power-saver\",\"balanced\",\"performance\"");
    hb_printf(&b, "]}");
    return hb_take(&b);
}

static void set_done(const DMsg *m, void *ctx) {
    (void)ctx;
    HBuf b = { 0 };
    int ok = m && m->type == DBUS_RETURN;
    hb_printf(&b, "{\"ok\":%s,\"error\":", ok ? "true" : "false");
    hb_str(&b, ok ? "" : m ? (m->error ? m->error : "error") : "disconnected");
    hb_printf(&b, "}");
    char *j = hb_take(&b);
    if (j && emit) emit("ppd-result", j);
    free(j);
    if (ok) get_all();
}

int ppd_set(const char *p) {
    if (!ppd_profile_valid(p) || which < 0) return -1;
    DBuf b;
    dw_init(&b);
    dw_str(&b, names[which]);
    dw_str(&b, "ActiveProfile");
    dw_variant(&b, "s");
    dw_str(&b, p);
    uint32_t s = sysbus_call(names[which], paths[which], "org.freedesktop.DBus.Properties", "Set", "ssv", &b, set_done, NULL);
    dw_free(&b);
    return s ? 0 : -1;
}
