/* xwayland.h — X11 apps through Xwayland, started only when one connects (Rust x11/xwayland.rs).
 *
 * The fourth exception to "axia-server doesn't run programs" (AGENTS.md): it only ever starts
 * /usr/bin/Xwayland (root-owned, not writable by others), with an argument list fixed in Rust, when
 * the first X11 client connects to this session's X socket; never from JS and with no argument
 * from anyone. See docs/WAYLAND.md, "X11 apps (Xwayland)". */
#ifndef LORD_XWAYLAND_H
#define LORD_XWAYLAND_H

/* Compatibility ABI during migration. Use only on the Wayland/UI thread; stop
 * before destroying its display. String views remain owned by Rust and valid
 * until initialization/stop changes their contents. Host hooks must not recurse
 * into initialization or shutdown. */

/* After wl_start(), in a user session (not the login screen, not app mode): if Xwayland is
 * installed, reserves a display (/tmp/.X11-unix/X<n>, 0600, and its lock file), sets DISPLAY for
 * the apps LordOS starts, and waits for a connection. 0 if there is an X socket. */
int  xwl_init(void);
void xwl_stop(void);                    /* stops Xwayland and removes the socket and the lock */
const char *xwl_display(void);          /* ":1", or "" */
const char *xwl_socket_path(void);      /* "/tmp/.X11-unix/X1", or "" (apps.c: restricted apps' view) */
int  xwl_running(void);                 /* is Xwayland running now? */
void xwl_wm_lost(void);                 /* window-manager connection failed */
void xwl_idle(void);                    /* no X11 application for the idle interval */

#endif
