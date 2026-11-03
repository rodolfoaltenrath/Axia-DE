/* Host power through logind; the Rust service owns validation/request state. */
#ifndef AXIA_HOST_POWER_H
#define AXIA_HOST_POWER_H

#include <stdbool.h>
int host_power_available(void);
int host_power_request(const char *action);

bool axia_battery_due(double now, bool force, bool battery);
double axia_battery_timeout(double now);
bool axia_battery_changed(const char *json);
const char *axia_host_power_begin(const char *action);
int axia_host_power_finish(const char *error);

#endif
