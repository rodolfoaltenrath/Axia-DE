/* js_media.h — lord.media (see js_media.c and docs/MEDIA.md). */
#ifndef LORD_JS_MEDIA_H
#define LORD_JS_MEDIA_H

#include "quickjs.h"

struct Node;
void js_media_register(JSContext *c, JSValue lord);
void js_media_node(struct Node *n, int id);       /* `media` prop of a media node */

#endif
