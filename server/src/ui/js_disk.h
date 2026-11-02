/* js_disk.h — lord.disks (Disks app; see js_disk.c). */
#ifndef LORD_JS_DISK_H
#define LORD_JS_DISK_H

#include "quickjs.h"

void js_disk_register(JSContext *c, JSValue lord);

#endif
