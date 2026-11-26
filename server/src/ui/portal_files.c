/* FileChooser wire transport only. The request's options are read in Rust (portal_chooser.rs), and
 * the user interface and path validation live there too. A request for a file is never refused
 * for what it asks: an application that gets an error often shows nothing at all. */
#include "portal_files.h"
#include "js_wm.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define FILES "org.freedesktop.portal.FileChooser"
#define REQUEST "org.freedesktop.portal.Request"
#define ROOT "/org/freedesktop/portal/desktop"
#define SLOTS 8
#define PER_CALLER 2
/* A request bigger than this is not a file dialog. */
#define LARGEST (4u * 1024 * 1024)

/* rust/axia-shell/src/portal_chooser.rs */
typedef struct {
    char token[129], mode[12], title[1025], folder[4096], name[16384], patterns[16384], choices[4096];
    int multiple;
    unsigned filters;
    int ignored;
} AxiaChooser;
void axia_portal_chooser(const char *member, const unsigned char *body, size_t len, int big_endian, AxiaChooser *out);

static struct Pick { unsigned id; char owner[256], path[512], choices[4096]; } picks[SLOTS];
static unsigned sequence;
static DConn *bus;

static int copy(char *out, size_t size, const char *in) {
    if (!in || strlen(in) >= size) return 0;
    memcpy(out, in, strlen(in) + 1);
    return 1;
}
/* The dialog of the oldest request of `owner` (any caller when NULL), NULL if there is none. */
static struct Pick *oldest(const char *owner, unsigned *count) {
    struct Pick *found = NULL;
    unsigned n = 0;
    for (unsigned i = 0; i < SLOTS; i++) {
        if (!picks[i].id || (owner && strcmp(picks[i].owner, owner))) continue;
        n++;
        if (!found || picks[i].id < found->id) found = &picks[i];
    }
    if (count) *count = n;
    return found;
}
/* Closes a dialog that makes room for a newer one, and tells its caller it was cancelled. */
static void replace(struct Pick *p, const char *why) {
    fprintf(stderr, "portal: file dialog %u cancelled: %s\n", p->id, why);
    js_portal_close(p->id);
    portal_files_result(p->id, NULL, 0);
}
static void open_file(DConn *c, const DMsg *m) {
    if (!m->sig || strcmp(m->sig, "ssa{sv}") || !m->sender || m->body_len > LARGEST) {
        fprintf(stderr, "portal: %s from %s is not a file chooser request\n", m->member, m->sender ? m->sender : "?");
        dbus_error(c, m, "org.freedesktop.DBus.Error.InvalidArgs", "Invalid file chooser request");
        return;
    }
    static AxiaChooser ask;
    axia_portal_chooser(m->member, m->body, m->body_len, m->big_endian, &ask);
    char sender[256];
    if (!copy(sender, sizeof sender, m->sender + 1)) return;
    for (char *q = sender; *q; q++) if (*q == '.') *q = '_';
    unsigned id = ++sequence;
    if (!id) id = ++sequence;
    if (!ask.token[0]) snprintf(ask.token, sizeof ask.token, "axia%u", id);
    char path[512];
    snprintf(path, sizeof path, ROOT "/request/%s/%s", sender, ask.token);
    bus = c;
    /* the same request again: the old dialog goes, without an answer (it would reach the new one) */
    for (unsigned i = 0; i < SLOTS; i++)
        if (picks[i].id && !strcmp(picks[i].path, path)) {
            js_portal_close(picks[i].id);
            picks[i].id = 0;
        }
    unsigned theirs = 0;
    struct Pick *old = oldest(m->sender, &theirs);
    if (theirs >= PER_CALLER) replace(old, "the application asked for another one");
    struct Pick *p = NULL;
    for (unsigned i = 0; i < SLOTS && !p; i++) if (!picks[i].id) p = &picks[i];
    if (!p) {
        p = oldest(NULL, NULL);
        replace(p, "too many dialogs were open");
    }
    p->id = id;
    copy(p->owner, sizeof p->owner, m->sender);
    copy(p->path, sizeof p->path, path);
    copy(p->choices, sizeof p->choices, ask.choices);
    fprintf(stderr, "portal: file dialog %u for %s: %s, %u filters%s\n", id, m->sender, ask.mode, ask.filters,
            ask.ignored ? " (some options were left out)" : "");
    /* A queued UI cannot answer until this event returns, so return the handle first. */
    DBuf reply;
    dw_init(&reply);
    dw_str(&reply, p->path);
    dbus_reply(c, m, "o", &reply);
    dw_free(&reply);
    if (!js_portal_pick(id, ask.mode, ask.title, ask.folder, ask.name, ask.multiple, ask.patterns)) {
        fprintf(stderr, "portal: file dialog %u could not be shown\n", id);
        portal_files_result(id, NULL, 0);
    }
}
/* The application's extra choices go back with the values they started with. */
static void put_choices(DBuf *b, char *lines) {
    dw_struct(b);
    dw_str(b, "choices");
    dw_variant(b, "a(ss)");
    size_t list = dw_arr_begin(b, 8);
    for (char *line = lines; *line; ) {
        char *end = strchr(line, '\n'), *tab = strchr(line, '\t');
        if (!end || !tab || tab > end) break;
        *tab = 0;
        *end = 0;
        dw_struct(b);
        dw_str(b, line);
        dw_str(b, tab + 1);
        line = end + 1;
    }
    dw_arr_end(b, list);
}
void portal_files_result(unsigned id, const char *const *uris, unsigned count) {
    if (!bus || !id) return;
    for (unsigned i = 0; i < SLOTS; i++) {
        if (picks[i].id != id) continue;
        DBuf b;
        dw_init(&b);
        dw_u32(&b, count ? 0 : 1);
        size_t dict = dw_arr_begin(&b, 8);
        if (count) {
            dw_struct(&b);
            dw_str(&b, "uris");
            dw_variant(&b, "as");
            size_t list = dw_arr_begin(&b, 4);
            for (unsigned j = 0; j < count; j++) dw_str(&b, uris[j]);
            dw_arr_end(&b, list);
            if (picks[i].choices[0]) put_choices(&b, picks[i].choices);
        }
        dw_arr_end(&b, dict);
        DHdr h = {.type = DBUS_SIGNAL, .path = picks[i].path, .iface = REQUEST, .member = "Response", .dest = picks[i].owner, .sig = "ua{sv}"};
        dbus_send(bus, &h, &b);
        dw_free(&b);
        picks[i].id = 0;
        return;
    }
}
int portal_files_handle(DConn *c, const DMsg *m) {
    if (m->type == DBUS_SIGNAL && m->sender && !strcmp(m->sender, "org.freedesktop.DBus") && m->iface && !strcmp(m->iface, "org.freedesktop.DBus") && m->member && !strcmp(m->member, "NameOwnerChanged") && m->sig && !strcmp(m->sig, "sss")) {
        DRd r;
        dr_init(&r, m);
        const char *name = dr_str(&r, NULL);
        dr_str(&r, NULL);
        const char *owner = dr_str(&r, NULL);
        if (!r.err && !owner[0])
            for (unsigned i = 0; i < SLOTS; i++)
                if (picks[i].id && !strcmp(picks[i].owner, name)) {
                    js_portal_close(picks[i].id);
                    picks[i].id = 0;
                }
        return 1;
    }
    if (m->type != DBUS_CALL || !m->path || !m->member) return 0;
    if (m->iface && !strcmp(m->iface, REQUEST) && !strcmp(m->member, "Close")) {
        for (unsigned i = 0; i < SLOTS; i++)
            if (picks[i].id && !strcmp(picks[i].path, m->path) && m->sender && !strcmp(picks[i].owner, m->sender)) {
                js_portal_close(picks[i].id);
                picks[i].id = 0;
                dbus_reply(c, m, NULL, NULL);
                return 1;
            }
        dbus_error(c, m, "org.freedesktop.DBus.Error.UnknownObject", "Request no longer exists");
        return 1;
    }
    if (!strcmp(m->path, ROOT) && m->iface && !strcmp(m->iface, FILES)) {
        if (!strcmp(m->member, "OpenFile") || !strcmp(m->member, "SaveFile") || !strcmp(m->member, "SaveFiles")) open_file(c, m);
        else dbus_error(c, m, "org.freedesktop.DBus.Error.UnknownMethod", "Unknown file chooser method");
        return 1;
    }
    return 0;
}
