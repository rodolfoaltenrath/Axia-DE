/* uevent.h — kernel uevents (NETLINK_KOBJECT_UEVENT) for Settings: DRM connector changes
 * (hotplugged displays), power_supply (battery, charger) and backlight. One socket in the main
 * loop's poll: nothing is polled. Receiving kernel uevents needs no privileges. */
#ifndef LORD_HW_UEVENT_H
#define LORD_HW_UEVENT_H

enum { UEV_DRM = 1, UEV_POWER = 2, UEV_BACKLIGHT = 4 };

int  uevent_open(void);                 /* 0 or -1 */
int  uevent_fd(void);                   /* -1 if closed */
/* Reads every pending uevent; returns a mask of UEV_* subsystems that changed. */
int  uevent_dispatch(void);
/* Parses one uevent datagram ("ACTION@devpath\0KEY=VAL\0..."): its UEV_* subsystem, or 0. */
int  uevent_parse(const char *buf, int len);

#endif
