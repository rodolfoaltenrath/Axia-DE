/* Included by wl.c: protocol transport for user-initiated focus transfers.
 * Tokens outlive their protocol objects, are bounded, expire, and are consumed once. */
#include "xdg-activation-v1-protocol.h"
#include <sys/random.h>
#define ACTIVATION_TTL 10000u
static struct { char token[65]; unsigned created; } activation_tokens[32];
static unsigned activation_slot;
typedef struct { int committed, surface; uint32_t serial; int has_serial; } Activation;
static void activation_gone(struct wl_resource *r) { free(wl_resource_get_user_data(r)); }
static int activation_mutable(struct wl_resource *r) {
    Activation *a = wl_resource_get_user_data(r);
    if (a->committed) {
        wl_resource_post_error(r, XDG_ACTIVATION_TOKEN_V1_ERROR_ALREADY_USED, "token already committed");
        return 0;
    }
    return 1;
}
static void activation_serial(struct wl_client *c, struct wl_resource *r, uint32_t serial, struct wl_resource *seat) {
    (void)c; (void)seat;
    if (!activation_mutable(r)) return;
    Activation *a = wl_resource_get_user_data(r);
    a->has_serial = 1; a->serial = serial;
}
static void activation_app(struct wl_client *c, struct wl_resource *r, const char *app) {
    (void)c; (void)app; activation_mutable(r);
}
static void activation_surface(struct wl_client *c, struct wl_resource *r, struct wl_resource *surface) {
    (void)c;
    if (!activation_mutable(r)) return;
    Activation *a = wl_resource_get_user_data(r);
    Surf *s = wl_resource_get_user_data(surface);
    a->surface = s ? s->id : 0;  /* no dangling pointers when the source closes */
}
static void activation_destroy(struct wl_client *c, struct wl_resource *r) { (void)c; wl_resource_destroy(r); }
static int activation_serial_valid(struct wl_client *c, uint32_t serial) {
    for (unsigned i = 0; i < 32; i++)
        if (activation_inputs[i].client == c && activation_inputs[i].serial == serial &&
            now_ms() - activation_inputs[i].time <= ACTIVATION_TTL) return 1;
    return 0;
}
static void activation_commit(struct wl_client *c, struct wl_resource *r) {
    if (!activation_mutable(r)) return;
    Activation *a = wl_resource_get_user_data(r);
    a->committed = 1;
    char token[65] = "";
    /* set_serial/set_surface are optional. A focused initiator can transfer focus
     * even when its toolkit supplies neither; an unfocused client cannot mint it. */
    if (key_focus && key_focus->client == c && (!a->surface || toplevel_of(find(a->surface)) == toplevel_of(key_focus)) &&
        (!a->has_serial || activation_serial_valid(c, a->serial))) {
        unsigned char random[32];
        if (getrandom(random, sizeof random, 0) == sizeof random) {
            for (unsigned i = 0; i < sizeof random; i++) snprintf(token + i*2, 3, "%02x", random[i]);
            unsigned slot = activation_slot++ % 32;
            memcpy(activation_tokens[slot].token, token, sizeof token);
            activation_tokens[slot].created = now_ms();
        }
    }
    xdg_activation_token_v1_send_done(r, token);
}
static const struct xdg_activation_token_v1_interface activation_token_impl = {
    .set_serial = activation_serial, .set_app_id = activation_app, .set_surface = activation_surface,
    .commit = activation_commit, .destroy = activation_destroy
};
static void activation_get(struct wl_client *c, struct wl_resource *r, uint32_t id) {
    (void)r;
    Activation *a = calloc(1, sizeof *a);
    struct wl_resource *token = a ? wl_resource_create(c, &xdg_activation_token_v1_interface, 1, id) : NULL;
    if (!token) { free(a); wl_client_post_no_memory(c); return; }
    wl_resource_set_implementation(token, &activation_token_impl, a, activation_gone);
}
static int activation_consume(const char *token) {
    if (!*token) return 0;
    for (unsigned i=0; i<32; i++) if (!strcmp(token, activation_tokens[i].token)) {
        activation_tokens[i].token[0] = 0;
        return now_ms() - activation_tokens[i].created <= ACTIVATION_TTL;
    }
    return 0;
}
static void activation_activate(struct wl_client *c, struct wl_resource *r, const char *token, struct wl_resource *surface) {
    (void)c; (void)r;
    if (!activation_consume(token)) return;
    Surf *s = wl_resource_get_user_data(surface);
    if (!s || s->role != R_TOPLEVEL) return;
    if (s->mapped) emit(s, "activate", "{}");
    else s->activation_pending = 1;
}
static const struct xdg_activation_v1_interface activation_impl = {
    .destroy = activation_destroy, .get_activation_token = activation_get, .activate = activation_activate
};
static void bind_activation(struct wl_client *c, void *data, uint32_t ver, uint32_t id) {
    (void)data; (void)ver;
    struct wl_resource *r = wl_resource_create(c, &xdg_activation_v1_interface, 1, id);
    if (!r) { wl_client_post_no_memory(c); return; }
    wl_resource_set_implementation(r, &activation_impl, NULL, NULL);
}
