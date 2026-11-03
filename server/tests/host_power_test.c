/* No real bus or power action: capture and answer the logind transport. */
#include "../src/platform/power.h"
#include "../src/hw/sysbus.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static int local, connected, calls, emitted, queue_ok = 1;
static SbReply reply;
static void *reply_ctx;
static char member[32], event_json[128];
int seat_managed(void) { return local; }
int sysbus_up(void) { return connected; }
void hw_emit(const char *kind, const char *json) {
    assert(!strcmp(kind, "session-power"));
    snprintf(event_json, sizeof event_json, "%s", json);
    emitted++;
}
uint32_t sysbus_call(const char *dest, const char *path, const char *iface,
                    const char *method, const char *sig, const DBuf *body, SbReply cb, void *ctx) {
    assert(!strcmp(dest, "org.freedesktop.login1"));
    assert(!strcmp(path, "/org/freedesktop/login1"));
    assert(!strcmp(iface, "org.freedesktop.login1.Manager"));
    assert(!strcmp(sig, "b"));
    assert(dbus_body_valid(sig, body->p, body->n, 0));
    DMsg msg = { .body = body->p, .body_len = body->n };
    DRd r;
    dr_init(&r, &msg);
    assert(dr_bool(&r) == 1 && !r.err); // normal host authentication allowed
    snprintf(member, sizeof member, "%s", method);
    reply = cb; reply_ctx = ctx; calls++;
    return queue_ok ? 42 : 0;
}
static void answer(const char *error) {
    DMsg msg = { .type = error ? DBUS_ERROR : DBUS_RETURN, .error = error };
    reply(&msg, reply_ctx);
}

int main(void) {
    assert(!host_power_available());
    assert(!host_power_request("reboot") && calls == 0);
    connected = 1;
    assert(!host_power_available()); // headless/remote-only sessions cannot power the host
    local = 1;
    assert(host_power_available());
    assert(!host_power_request("reboot --force") && calls == 0);
    assert(!host_power_request(NULL) && calls == 0);
    assert(host_power_request("reboot") && calls == 1 && !strcmp(member, "Reboot"));
    assert(!host_power_request("poweroff") && calls == 1); // single outstanding request
    answer("org.freedesktop.DBus.Error.AccessDenied");
    assert(emitted == 1 && strstr(event_json, "denied"));
    assert(host_power_request("poweroff") && !strcmp(member, "PowerOff"));
    answer("org.freedesktop.login1.OperationInhibited");
    assert(strstr(event_json, "inhibited"));
    assert(host_power_request("reboot"));
    reply(NULL, reply_ctx); // bus disconnect releases the outstanding request
    assert(strstr(event_json, "unavailable"));
    queue_ok = 0;
    assert(!host_power_request("reboot"));
    queue_ok = 1;
    assert(host_power_request("poweroff")); // failed queue also releases state
    answer(NULL);
    assert(!strcmp(event_json, "{\"ok\":true}"));
    puts("host-power: local gating, fixed D-Bus actions, authentication, duplicate requests and replies passed");
}
