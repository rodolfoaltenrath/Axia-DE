/* portal.h — our own org.freedesktop.portal.Settings on the session bus (see portal.c). */
#ifndef LORD_PORTAL_H
#define LORD_PORTAL_H

int  portal_start(void);                /* after dropping privileges; 0 if it owns the name */
int  portal_fd(void);                   /* for poll; -1 if absent */
int  portal_wants_write(void);          /* there are unsent messages (POLLOUT) */
void portal_dispatch(void);
/* From the resident Rust preference service: mode, RGB, GNOME accent and GTK buttons. */
void portal_pick_result(unsigned id, const char *const *uris, unsigned count);
void portal_set(int dark, const double rgb[3], const char *accent_name, const char *button_layout, int legacy_style);

#endif
