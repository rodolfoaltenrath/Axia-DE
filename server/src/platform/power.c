/* Transport only: reuse the compositor's asynchronous system bus connection. */
#include "power.h"
#include "seat.h"
#include "../hw/hw.h"
#include "../hw/sysbus.h"

int host_power_available(void) { return seat_managed() && sysbus_up(); }

static void power_reply(const DMsg *m, void *ctx) {
    (void)ctx;
    const char *error = !m ? "org.freedesktop.DBus.Error.Disconnected"
                      : m->type == DBUS_RETURN ? NULL
                      : m->error ? m->error : "org.freedesktop.DBus.Error.Failed";
    int status = axia_host_power_finish(error);
    const char *json = status == 1 ? "{\"ok\":true}"
                     : status == 2 ? "{\"ok\":false,\"why\":\"denied\"}"
                     : status == 3 ? "{\"ok\":false,\"why\":\"inhibited\"}"
                                   : "{\"ok\":false,\"why\":\"unavailable\"}";
    hw_emit("session-power", json);
}

int host_power_request(const char *action) {
    if (!host_power_available()) return 0;
    const char *method = axia_host_power_begin(action);
    if (!method) return 0;
    DBuf b;
    dw_init(&b);
    dw_bool(&b, 1); // Allow the host's polkit agent to authenticate when required.
    uint32_t serial = sysbus_call("org.freedesktop.login1", "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager", method, "b", &b, power_reply, NULL);
    dw_free(&b);
    if (!serial) axia_host_power_finish("org.freedesktop.DBus.Error.Disconnected");
    return serial != 0;
}
