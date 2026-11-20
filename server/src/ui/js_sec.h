/* js_sec.h — privacy and signing APIs for the theme and the system apps:
 * lord.perms (per-app permissions with system-drawn prompts) and lord.sign (signature, trust, "Modified by
 * you", restore and share). See server/src/sec and docs/PRIVACY.md, docs/SIGNING.md. */
#ifndef LORD_JS_SEC_H
#define LORD_JS_SEC_H

#include "quickjs.h"

void js_sec_register(JSContext *c, JSValue lord);
/* Rust owns notification permission validation, persistence and revocation. */
int axia_notification_permission_set(const char *app, int state);

#endif
