/* xmlmini.h — minimal non-validating XML reader for the KeePass (KDBX) XML and key
 * files: builds a tree in the secrets arena (smem.h). Elements, attributes, text with the
 * predefined and numeric entities (&#N; &#xN;), CDATA, comments and processing
 * instructions. Never expands declared entities: a <!DOCTYPE is an error (no "billion
 * laughs" or external entities). Maximum depth 256. */
#ifndef LORD_VAULT_XMLMINI_H
#define LORD_VAULT_XMLMINI_H

#include <stddef.h>

typedef struct { char *name, *value; } XAttr;
typedef struct XNode {
    char *name;
    char *text; size_t tlen;          /* direct text (without the children's), no entities */
    XAttr *attrs; int nattrs;
    struct XNode **kids; int nkids, cap;
    struct XNode *parent;
} XNode;

/* Document root (the topmost element) or NULL; err (if not NULL) receives a code. */
XNode *xml_parse(const char *s, size_t len, const char **err);
void   xml_free(XNode *n);
XNode *xml_child(const XNode *n, const char *name);              /* first child with that name */
const char *xml_attr(const XNode *n, const char *name);           /* NULL if absent */
const char *xml_text(const XNode *n);                             /* "" if none */
/* Replaces the text (the node takes ownership of the new one; in smem). */
void   xml_set_text(XNode *n, char *text, size_t len);

#endif
