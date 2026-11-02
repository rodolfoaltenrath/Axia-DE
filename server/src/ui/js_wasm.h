/* js_wasm.h — the WebAssembly global (see js_wasm.c and docs/WASM.md). */
#ifndef LORD_JS_WASM_H
#define LORD_JS_WASM_H

#include <quickjs.h>

void js_wasm_register(JSContext *ctx, JSValueConst global);
void js_wasm_shutdown(JSContext *ctx);     /* before JS_FreeContext */

#endif
