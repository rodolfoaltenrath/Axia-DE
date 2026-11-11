/* js_help.h — lord.help (Help articles) and lord.time.zoneOffset (Clock). See js_help.c. */
#ifndef LORD_JS_HELP_H
#define LORD_JS_HELP_H

#include "quickjs.h"

/* Rust content service. Returned JSON is released with axia_view_free. */
char *axia_help_request(const char *assets, const char *input);

/* Call after js_sys_register (it adds zoneOffset to the lord.time object that one creates). */
void js_help_register(JSContext *c, JSValue lord);

#endif
