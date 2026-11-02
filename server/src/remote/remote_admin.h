/* remote_admin.h — root actions of the remote desktop (see remote_admin.c and docs/REMOTE.md). */
#ifndef LORD_REMOTE_ADMIN_H
#define LORD_REMOTE_ADMIN_H

#include "../admin.h"

void remote_act_enable(const AdminReq *r, const char *arg, size_t len);
void remote_act_disable(const AdminReq *r, const char *arg, size_t len);
void remote_act_config(const AdminReq *r, const char *arg, size_t len);
void remote_act_member(const AdminReq *r, const char *arg, size_t len);
void remote_act_kick(const AdminReq *r, const char *arg, size_t len);
void remote_act_block(const AdminReq *r, const char *arg, size_t len);
void remote_act_unblock(const AdminReq *r, const char *arg, size_t len);
void remote_act_allow(const AdminReq *r, const char *arg, size_t len);
void remote_act_failures(const AdminReq *r, const char *arg, size_t len);
void remote_act_ice(const AdminReq *r, const char *arg, size_t len);

#define REMOTE_ADMIN_ACTIONS \
    { "remote-enable", remote_act_enable, A_FORK }, \
    { "remote-disable", remote_act_disable, A_FORK }, \
    { "remote-config", remote_act_config, A_FORK }, \
    { "remote-member", remote_act_member, 0 }, \
    { "remote-kick", remote_act_kick, 0 }, \
    { "remote-block", remote_act_block, 0 }, \
    { "remote-unblock", remote_act_unblock, 0 }, \
    { "remote-allow", remote_act_allow, 0 }, \
    { "remote-failures", remote_act_failures, 0 }, \
    { "remote-ice", remote_act_ice, 0 },

#endif
