/* proc_admin.h — the root helper's process actions for the Monitor (administrators only; see
 * proc_admin.c and docs/MONITOR.md, "Process actions"). */
#ifndef LORD_PROC_ADMIN_H
#define LORD_PROC_ADMIN_H

#include "../admin.h"

void proc_act_signal(const AdminReq *r, const char *arg, size_t len);
void proc_act_info(const AdminReq *r, const char *arg, size_t len);

/* Administrators only (no A_ANYONE); both are quick, so they run inside the helper. */
#define PROC_ADMIN_ACTIONS \
    { "proc-signal", proc_act_signal, 0 }, \
    { "proc-info", proc_act_info, 0 },

#endif
