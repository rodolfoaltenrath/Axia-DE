/* ssh_admin.h — root actions of remote login (SSH) (see ssh_admin.c and docs/REMOTE.md). */
#ifndef LORD_SSH_ADMIN_H
#define LORD_SSH_ADMIN_H

#include "../admin.h"

void ssh_act_set(const AdminReq *r, const char *arg, size_t len);
void ssh_act_allow(const AdminReq *r, const char *arg, size_t len);

/* Fixed text of the "who can log in" drop-in for an allow value ("all" → NULL: no file;
 * "admins" → the AllowGroups line); -1 for anything else. Exposed for tests. */
int ssh_allow_text(const char *value, const char **text);

#define SSH_ADMIN_ACTIONS \
    { "ssh-set", ssh_act_set, A_FORK }, \
    { "ssh-allow", ssh_act_allow, A_FORK },

#endif
