#ifndef AXIA_WINMODEL_FFI_H
#define AXIA_WINMODEL_FFI_H
/* The window manager's pure rules in Rust (rust/axia-shell/src/winmodel): remembered places, the
 * list of desktops, the Overview's and Snap Assist's layouts, Settings → Displays' arrangement.
 * One call: `op` with `json` (a JSON array of `len` bytes, at most 16 MiB). 0 and the result's JSON in
 * *out, or -1 and why in *out (NULL only without memory). Free *out with axia_winmodel_free. */
#include <stddef.h>

int  axia_winmodel_call(const char *op, const char *json, size_t len, char **out);
void axia_winmodel_free(char *s);

/* The session's windows (rust winmodel/session.rs): opaque, Rust-owned, UI thread only. Like
 * axia_winmodel_call, with `json` = [facts, arguments…]; the answer is { result, state }. */
typedef struct AxiaWindows AxiaWindows;
AxiaWindows *axia_windows_new(void);
void         axia_windows_free(AxiaWindows *);
int          axia_windows_call(AxiaWindows *, const char *op, const char *json, size_t len, char **out);
#endif
