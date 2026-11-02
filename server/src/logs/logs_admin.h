/* logs_admin.h — the helper's logs-system action (system logs for administrators; Logs app,
 * see docs/SYSTEM.md). */
#ifndef LORD_LOGS_ADMIN_H
#define LORD_LOGS_ADMIN_H

#include "../admin.h"

void logs_act_system(const AdminReq *r, const char *arg, size_t len);

/* Administrators only (no A_ANYONE); in a separate process because it can take a while. */
#define LOGS_ADMIN_ACTIONS \
    { "logs-system", logs_act_system, A_FORK },

#endif
