/* js_logs.h — lord.logs (Logs app; see js_logs.c and docs/SYSTEM.md). */
#ifndef LORD_JS_LOGS_H
#define LORD_JS_LOGS_H

#include "quickjs.h"

void js_logs_register(JSContext *c, JSValue lord);
/* epoll fd for the main loop (thread results and inotify), or -1 */
int  logs_fd(void);
/* kind: "result" (id), "journal", "file" (name) */
void logs_dispatch(void (*fn)(const char *kind, int id, const char *name));

#endif
