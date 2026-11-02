/* js_monitor.h — lord.monitor (metrics for the Monitor app; see js_monitor.c). */
#ifndef LORD_JS_MONITOR_H
#define LORD_JS_MONITOR_H

#include "quickjs.h"

void js_monitor_register(JSContext *c, JSValue lord, const char *assets_dir);

#endif
