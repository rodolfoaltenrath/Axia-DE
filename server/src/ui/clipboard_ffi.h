/* Session-only Rust history service. Returned JSON is owned by Rust. Captured
 * descriptors are transferred to the worker; strings are copied during calls. */
#ifndef AXIA_CLIPBOARD_FFI_H
#define AXIA_CLIPBOARD_FFI_H
#include <stdbool.h>
#include <stddef.h>
const char *axia_clipboard_kind(const char *text, bool image);
int axia_clipboard_fd(void);
char *axia_clipboard_take(void);
char *axia_clipboard_request(const char *json);
void axia_clipboard_free(char *json);
void axia_clipboard_capture(const char *text, size_t len, const char *origin, bool sensitive, int image_fd, const char *source);
#endif
