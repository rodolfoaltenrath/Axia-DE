/* js_print.h — lord.print: printers (CUPS over IPP) and eSCL scanners (see js_print.c). */
#ifndef LORD_JS_PRINT_H
#define LORD_JS_PRINT_H

#include "quickjs.h"

void js_print_register(JSContext *c, JSValue lord);

#endif
