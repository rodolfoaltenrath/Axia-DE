/* ABI-only bridge for app mode. Process/window policy lives in the app Rust modules. */
#ifndef AXIA_JS_APP_HOST_H
#define AXIA_JS_APP_HOST_H
#include "quickjs.h"
#include <stddef.h>
#include <stdint.h>
int axia_app_window_open(const char *title, int width, int height);
int axia_app_window_attach(int key, int node);
void axia_app_window_close(int key);
void axia_app_window_title(int key, const char *title);
uint8_t *axia_app_qjs_compile(const char *src, size_t len, const char *name, size_t *outlen);
int axia_app_qjs_eval(JSContext *ctx, const uint8_t *bytecode, size_t len, int *ok);
#endif
