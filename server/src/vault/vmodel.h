/* vmodel.h — the contents of an open vault, in the secrets arena: entries with their fields,
 * tags, attachments and history, plus the categories, tags and settings (which the
 * UI handles as JSON and which are stored here as-is, validated).
 *
 * Contents (JSON, version 1):
 *   {"v":1,"meta":{…},"cats":[…],"tags":[…],"entries":[{
 *      "id":"<32 hex>","cat":"internet","title":"…","notes":"…","fav":true,"arch":false,
 *      "trash":0,"created":…,"modified":…,"pwch":…,"rotate":90,"tags":["…"],
 *      "f":[{"k":"username","t":"text","l":"…","v":"…"},…],
 *      "att":[{"id","name","size","mime","nonce","mac"}],
 *      "hist":[{"t":…,"title":…,"f":[…]}],"x":{…}}]}
 * Field types: text, hidden, url, date, number, file, totp, email, multiline, pin. */
#ifndef LORD_VAULT_VMODEL_H
#define LORD_VAULT_VMODEL_H

#include <stddef.h>
#include <stdint.h>
#include "vjson.h"

typedef struct { char *k, *t, *l, *v; } VField;
typedef struct { uint8_t id[16]; char *name, *mime; uint64_t size; uint8_t nonce[24], mac[16]; } VAtt;

typedef struct {
    char id[33];
    char *cat, *title, *notes;
    VField *f; int nf;
    char **tags; int ntags;
    VAtt *att; int natt;
    int fav, arch, rotate;
    int64_t trash, created, modified, pwch;
    char *hist;               /* JSON (array) as-is */
    char *x;                  /* extra JSON (object): keyring data, etc. */
    char *fold;               /* search text (lowercase, no accents) */
    uint8_t pwhash[16];       /* BLAKE2b of the password (to find duplicates); all zero if none */
} VEntry;

typedef struct {
    VEntry **e; int n, cap;
    char *meta, *cats, *tags; /* JSON as-is */
    int dirty;
} VModel;

int   vm_load(VModel *m, const char *json, size_t len);  /* 0 on success */
void  vm_init(VModel *m);
void  vm_free(VModel *m);
char *vm_dump(const VModel *m, size_t *len);              /* JSON contents in smem */

VEntry *vm_find(const VModel *m, const char *id);
int   vm_index(const VModel *m, const char *id);
/* Creates or updates from the UI JSON (object in j/tok). Hidden fields that arrive
 * with "keep":true keep their previous value. Saves history if anything changed. *out = the entry. */
int   vm_upsert(VModel *m, const Vj *j, int tok, VEntry **out, char *err, size_t errn);
int   vm_remove(VModel *m, const char *id);               /* delete permanently (empty the trash) */
void  vm_entry_free(VEntry *e);
const char *vm_field(const VEntry *e, const char *key);   /* value of the first field with that key */
const char *vm_field_type(const VEntry *e, const char *type);   /* value of the first field of that type */
void  vm_refresh(VEntry *e);                              /* recomputes fold and pwhash */
void  vm_new_id(char out[33]);

/* JSON of an entry (for the UI). reveal: 0 = hidden fields without value ("has":true),
 * 1 = with value. With hist it includes the history (hidden history values only with reveal). */
void  vm_entry_json(VjB *b, const VEntry *e, int reveal, int hist);
/* Lightweight list of all entries (no secrets): id, cat, title, sub, url, fav, arch, trash,
 * modified, tags, totp (if any), natt, password kind (age), expires. */
void  vm_list_json(VjB *b, const VModel *m);
/* Search: ids that contain all the words of q (ignoring accents and case), in list
 * order; scope: "all" (no trash), "trash", "archived"… filtered by the UI. */
void  vm_search_json(VjB *b, const VModel *m, const char *q, int max);
/* Lowercases and strips accents from a UTF-8 text (in smem). */
char *vm_fold(const char *s);

#endif
