/* imager_admin.h — the LordOS Imager's admin actions (imager_admin.c, docs/IMAGER.md). */
#ifndef LORD_IMAGER_ADMIN_H
#define LORD_IMAGER_ADMIN_H

#include "../admin.h"

void imager_act_fetch(const AdminReq *r, const char *arg, size_t len);
void imager_act_fetch_cancel(const AdminReq *r, const char *arg, size_t len);
void imager_act_cache_clear(const AdminReq *r, const char *arg, size_t len);
void imager_act_write(const AdminReq *r, const char *arg, size_t len);

/* Any session user, like the disk-* actions for removable disks (udisks with an active session):
 * the downloads run as the user, and imager-write only ever touches a removable disk.
 * Cancelling a write is disk-cancel <disk>. */
#define IMAGER_ADMIN_ACTIONS \
    { "imager-fetch", imager_act_fetch, A_FORK | A_ANYONE }, \
    { "imager-fetch-cancel", imager_act_fetch_cancel, A_ANYONE }, \
    { "imager-cache-clear", imager_act_cache_clear, A_FORK | A_ANYONE }, \
    { "imager-write", imager_act_write, A_FORK | A_ANYONE },

#endif
