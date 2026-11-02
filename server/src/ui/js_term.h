/* js_term.h — lord.term.* (terminals) and lord.clipboard. */
#ifndef LORD_JS_TERM_H
#define LORD_JS_TERM_H

#include "quickjs.h"

void js_term_register(JSContext *c, JSValue lord);

#endif
