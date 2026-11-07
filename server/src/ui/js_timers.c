/* QuickJS references stay in the prelude; this bridge only converts scheduler arguments/results.
 * Every context owns its Rust queue through a finalizable native object. */
#include "js_timers.h"
#include "../scheduler_ffi.h"
#include <math.h>
#include <stdlib.h>

static JSClassID scheduler_class;
static AxiaScheduler *state(JSContext *c, JSValueConst self) {
    return JS_GetOpaque2(c, self, scheduler_class);
}
static void finalize(JSRuntime *rt, JSValue value) {
    (void)rt;
    axia_scheduler_free(JS_GetOpaque(value, scheduler_class));
}
static uint64_t timer_id(JSContext *c, JSValueConst value) {
    double id;
    if (!JS_IsNumber(value) || JS_ToFloat64(c, &id, value) || !isfinite(id) || id < 1 || id > 9007199254740991. || floor(id) != id) return 0;
    return (uint64_t)id;
}
static JSValue add(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaScheduler *s = state(c, self); double now, delay;
    if (!s) return JS_EXCEPTION;
    if (argc < 3 || JS_ToFloat64(c, &now, argv[0]) || JS_ToFloat64(c, &delay, argv[1])) return JS_EXCEPTION;
    uint64_t id = axia_scheduler_add(s, now, delay, JS_ToBool(c, argv[2]) > 0);
    return id ? JS_NewFloat64(c, (double)id) : JS_ThrowRangeError(c, "timer queue full or invalid clock");
}
static JSValue cancel(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaScheduler *s = state(c, self); if (!s) return JS_EXCEPTION;
    if (argc) axia_scheduler_cancel(s, timer_id(c, argv[0]));
    return JS_UNDEFINED;
}
static JSValue due(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaScheduler *s = state(c, self); double now;
    if (!s) return JS_EXCEPTION;
    if (!argc || JS_ToFloat64(c, &now, argv[0])) return JS_EXCEPTION;
    size_t n = axia_scheduler_due(s, now, NULL, 0);
    if (!n) return JS_NULL; /* Idle ticks allocate no JS arrays. */
    uint64_t *ids = n ? malloc(n * sizeof *ids) : NULL;
    if (n && !ids) return JS_ThrowOutOfMemory(c);
    if (n) axia_scheduler_due(s, now, ids, n);
    JSValue list = JS_NewArray(c);
    if (!JS_IsException(list)) for (size_t i = 0; i < n; i++) JS_SetPropertyUint32(c, list, (uint32_t)i, JS_NewFloat64(c, (double)ids[i]));
    free(ids); return list;
}
static JSValue fire(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaScheduler *s = state(c, self); double now;
    if (!s) return JS_EXCEPTION;
    if (argc < 2 || JS_ToFloat64(c, &now, argv[1])) return JS_EXCEPTION;
    return JS_NewInt32(c, axia_scheduler_fire(s, timer_id(c, argv[0]), now));
}
static JSValue next(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaScheduler *s = state(c, self); double now;
    if (!s) return JS_EXCEPTION;
    if (!argc || JS_ToFloat64(c, &now, argv[0])) return JS_EXCEPTION;
    double wait = axia_scheduler_wait(s, now);
    axia_scheduler_changed(s); /* The caller now knows every timer deadline queued so far. */
    return JS_NewFloat64(c, wait);
}
static JSValue request_frame(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)argc; (void)argv;
    AxiaScheduler *s = state(c, self); if (!s) return JS_EXCEPTION;
    uint64_t id = axia_scheduler_frame(s);
    return id ? JS_NewFloat64(c, (double)id) : JS_ThrowRangeError(c, "animation frame queue full");
}
static JSValue cancel_frame(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    AxiaScheduler *s = state(c, self); if (!s) return JS_EXCEPTION;
    if (argc) axia_scheduler_cancel_frame(s, timer_id(c, argv[0]));
    return JS_UNDEFINED;
}
static JSValue frames(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)argc; (void)argv;
    AxiaScheduler *s = state(c, self); if (!s) return JS_EXCEPTION;
    size_t n = axia_scheduler_frame_count(s);
    uint64_t *ids = n ? malloc(n * sizeof *ids) : NULL;
    if (n && !ids) return JS_ThrowOutOfMemory(c);
    JSValue list = JS_NewArray(c);
    if (JS_IsException(list)) { free(ids); return list; }
    if (n) axia_scheduler_take_frames(s, ids, n);
    for (size_t i = 0; i < n; i++) JS_SetPropertyUint32(c, list, (uint32_t)i, JS_NewFloat64(c, (double)ids[i]));
    free(ids); return list;
}
static JSValue has_frames(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)argc; (void)argv;
    AxiaScheduler *s = state(c, self); if (!s) return JS_EXCEPTION;
    return JS_NewBool(c, axia_scheduler_frame_count(s) > 0);
}
static JSValue frame_gate(JSContext *c, JSValueConst self, int argc, JSValueConst *argv, int ready) {
    AxiaScheduler *s = state(c, self); double now; uint32_t epoch = 0;
    if (!s) return JS_EXCEPTION;
    if (argc < 3 || JS_ToFloat64(c, &now, argv[0]) || JS_ToUint32(c, &epoch, argv[2])) return JS_EXCEPTION;
    bool waiting = JS_ToBool(c, argv[1]) > 0;
    return ready ? JS_NewBool(c, axia_scheduler_frame_ready(s, now, waiting, epoch))
                 : JS_NewFloat64(c, axia_scheduler_frame_wait(s, now, waiting, epoch));
}
static JSValue reset(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)argc; (void)argv;
    AxiaScheduler *s = state(c, self); if (!s) return JS_EXCEPTION;
    axia_scheduler_reset(s); return JS_UNDEFINED;
}
static JSValue changed(JSContext *c, JSValueConst self, int argc, JSValueConst *argv) {
    (void)argc; (void)argv;
    AxiaScheduler *s = state(c, self); if (!s) return JS_EXCEPTION;
    return JS_NewBool(c, axia_scheduler_changed(s));
}
static const JSCFunctionListEntry functions[] = {
    JS_CFUNC_DEF("add", 3, add), JS_CFUNC_DEF("cancel", 1, cancel),
    JS_CFUNC_DEF("due", 1, due), JS_CFUNC_DEF("fire", 2, fire), JS_CFUNC_DEF("next", 1, next),
    JS_CFUNC_DEF("requestFrame", 0, request_frame), JS_CFUNC_DEF("cancelFrame", 1, cancel_frame),
    JS_CFUNC_DEF("frames", 0, frames), JS_CFUNC_DEF("hasFrames", 0, has_frames),
    JS_CFUNC_MAGIC_DEF("frameReady", 3, frame_gate, 1), JS_CFUNC_MAGIC_DEF("frameWait", 3, frame_gate, 0),
    JS_CFUNC_DEF("reset", 0, reset), JS_CFUNC_DEF("changed", 0, changed),
};
void js_timers_register(JSContext *c, JSValue lord) {
    if (!scheduler_class) JS_NewClassID(&scheduler_class);
    JSRuntime *rt = JS_GetRuntime(c);
    if (!JS_IsRegisteredClass(rt, scheduler_class)) {
        JSClassDef def = { .class_name = "AxiaScheduler", .finalizer = finalize };
        if (JS_NewClass(rt, scheduler_class, &def) < 0) return;
    }
    JSValue object = JS_NewObjectClass(c, scheduler_class);
    if (JS_IsException(object)) return;
    JS_SetOpaque(object, axia_scheduler_new());
    JS_SetPropertyFunctionList(c, object, functions, sizeof functions / sizeof functions[0]);
    JS_SetPropertyStr(c, lord, "timers", object);
}
