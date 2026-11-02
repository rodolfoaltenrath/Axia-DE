/* cbor.h — small, strict CBOR (RFC 8949) for CTAP2: integers, bytes, text, arrays, maps,
 * true/false/null. No floats, tags or indefinite lengths.
 *
 * Writer: over a fixed buffer; the caller writes the map keys already in CTAP2 canonical order
 * (shorter first, then bytewise: 1, 2, 3 … -1, -2; "id" < "name").
 * Reader: builds an array of items in order (no copying), each with where its subtree ends;
 * rejects non-minimal encodings ("overlong"), reserved simple values, invalid UTF-8 and
 * nesting deeper than 16 levels. */
#ifndef LORD_VAULT_CBOR_H
#define LORD_VAULT_CBOR_H

#include <stddef.h>
#include <stdint.h>

typedef struct { uint8_t *p; size_t n, cap; int err; } CbW;
void cbw_init(CbW *w, uint8_t *buf, size_t cap);
void cbw_uint(CbW *w, uint64_t v);
void cbw_int(CbW *w, int64_t v);
void cbw_bytes(CbW *w, const void *d, size_t n);
void cbw_text(CbW *w, const char *s);
void cbw_textn(CbW *w, const char *s, size_t n);
void cbw_array(CbW *w, size_t n);
void cbw_map(CbW *w, size_t n);
void cbw_bool(CbW *w, int v);
void cbw_null(CbW *w);
void cbw_raw(CbW *w, const void *d, size_t n);      /* an already-encoded item */

enum { CB_UINT = 1, CB_NINT, CB_BYTES, CB_TEXT, CB_ARRAY, CB_MAP, CB_FALSE, CB_TRUE, CB_NULL, CB_UNDEF };

typedef struct {
    uint8_t type;
    uint64_t v;            /* UINT: the value; NINT: the value is -1 - v; ARRAY/MAP: count */
    const uint8_t *ptr;    /* BYTES/TEXT: the data */
    size_t len;
    int next;              /* index of the item following this subtree */
} CbItem;

typedef struct { const CbItem *it; int n; size_t used; } Cb;

/* Reads one item from d. items: caller's array (max items). With allow_trailing, whatever
 * follows is left over (c->used says how many bytes it took); otherwise it must end exactly. 0 = ok. */
int  cb_parse(Cb *c, const uint8_t *d, size_t len, CbItem *items, int max, int allow_trailing);
int  cb_type(const Cb *c, int i);                    /* 0 if i < 0 */
int  cb_map_int(const Cb *c, int map, int64_t key);  /* index of the value, or -1 */
int  cb_map_text(const Cb *c, int map, const char *key);
int  cb_at(const Cb *c, int arr, int k);             /* k-th element, or -1 */
int  cb_int(const Cb *c, int i, int64_t *out);       /* UINT/NINT that fits in int64: 0 = ok */
int  cb_text_eq(const Cb *c, int i, const char *s);

#endif
