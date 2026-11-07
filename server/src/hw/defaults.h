/* defaults.h — default applications on disk (Settings → Default Apps): the user's
 * ~/.config/mimeapps.list and the system ones, shared-mime-info's globs2, and the preferred
 * terminal in ~/.config/xdg-terminals.list (the xdg-terminal-exec convention). LordOS's own apps
 * are written as "lordos-<app>.desktop" (no such file exists; other desktops skip them). */
#ifndef LORD_HW_DEFAULTS_H
#define LORD_HW_DEFAULTS_H

#include <stddef.h>

/* First [Default Applications] entry for a MIME type over those files, without checking that it is
 * installed (lord.mime.get). 0 if found ("x.desktop"). The full lookup, with installed apps and the
 * automatic choice, is apps_default_for() in the Rust applications core. */
int  defaults_lookup(const char *mime, char *id, size_t n);
/* Sets it in $XDG_CONFIG_HOME/mimeapps.list (~/.config fallback; atomic write). */
int  defaults_set(const char *mime, const char *id);
/* Removes the user's choice for mime from ~/.config/mimeapps.list ([Default Applications] and
 * [Added Associations]; back to automatic). 0 if done or nothing was there. */
int  defaults_unset(const char *mime);
/* MIME type of a file name (globs2). 0 if known. */
int  defaults_guess(const char *path, char *mime, size_t n);
/* Tests only: a folder read in place of "/" for the system files (fake XDG trees). */
void defaults_set_root(const char *root);
/* Preferred terminal ("foot.desktop", "lordos-terminal.desktop"). */
int  defaults_terminal(char *id, size_t n);
int  defaults_set_terminal(const char *id);

#endif
