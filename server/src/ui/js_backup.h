/* js_backup.h — lord.backup (Backups; see js_backup.c). */
#ifndef LORD_JS_BACKUP_H
#define LORD_JS_BACKUP_H

#include "quickjs.h"

void js_backup_register(JSContext *c, JSValue lord);

#endif
