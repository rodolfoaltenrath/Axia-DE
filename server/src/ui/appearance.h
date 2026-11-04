/* Rust owns toolkit theme discovery and the GLib keyfile merge. Call on the UI
 * thread when appearance changes, never per frame. Returned names are static. */
#ifndef AXIA_APPEARANCE_H
#define AXIA_APPEARANCE_H
#include <stdbool.h>
const char *axia_appearance_theme(bool dark);
/* Strings are valid NUL-terminated UTF-8 for the duration of this call. */
int axia_appearance_write(bool dark, const char *theme, const char *accent, const char *buttons);
#endif
