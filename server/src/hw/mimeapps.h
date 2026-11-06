/* Validators used by the C Settings bridge. MIME text parsing, editing and
 * file ownership live in rust/axia-shell/src/applications/. */
#ifndef LORD_HW_MIMEAPPS_H
#define LORD_HW_MIMEAPPS_H

#define MIME_MAX 128
#define DESKTOP_ID_MAX 160

/* "type/subtype" (also x-scheme-handler/https): [a-z0-9] start, then [A-Za-z0-9.+_-]. */
int mime_valid(const char *mime);
/* A desktop file id: "org.gnome.Evince.desktop", "lordos-photos.desktop". No '/', no "..". */
int desktop_id_valid(const char *id);

#endif
