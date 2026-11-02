/* js_pkg.h — lord.pkg (Debian package queries, see js_pkg.c). */
#ifndef LORD_JS_PKG_H
#define LORD_JS_PKG_H

#include "quickjs.h"

void js_pkg_register(JSContext *c, JSValue lord);

#endif
