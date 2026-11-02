/* ipp.h — our own IPP/1.1 encoder and decoder (RFC 8010/8011), without libcups.
 *
 * Enough to talk to CUPS over its local socket (cups.c) and to read what it returns:
 * groups, multi-valued attributes, integers, enums, booleans, ranges, resolutions, texts
 * and (nested) collections. The decoder validates every length: what arrives over the socket
 * is treated as untrusted (tests/ipp_test.c tests it with broken messages). */
#ifndef LORD_PRINT_IPP_H
#define LORD_PRINT_IPP_H

#include <stddef.h>
#include <stdint.h>

/* group tags */
enum { IPP_G_OPERATION = 0x01, IPP_G_JOB = 0x02, IPP_G_END = 0x03, IPP_G_PRINTER = 0x04, IPP_G_UNSUPPORTED = 0x05 };
/* value tags */
enum {
    IPP_T_UNSUPPORTED = 0x10, IPP_T_UNKNOWN = 0x12, IPP_T_NOVALUE = 0x13,
    IPP_T_INTEGER = 0x21, IPP_T_BOOLEAN = 0x22, IPP_T_ENUM = 0x23,
    IPP_T_STRING = 0x30, IPP_T_DATE = 0x31, IPP_T_RESOLUTION = 0x32, IPP_T_RANGE = 0x33,
    IPP_T_BEGCOL = 0x34, IPP_T_TEXTLANG = 0x35, IPP_T_NAMELANG = 0x36, IPP_T_ENDCOL = 0x37,
    IPP_T_TEXT = 0x41, IPP_T_NAME = 0x42, IPP_T_KEYWORD = 0x44, IPP_T_URI = 0x45, IPP_T_URISCHEME = 0x46,
    IPP_T_CHARSET = 0x47, IPP_T_LANGUAGE = 0x48, IPP_T_MIMETYPE = 0x49, IPP_T_MEMBERNAME = 0x4a,
};
/* operations */
enum {
    IPP_OP_PRINT_JOB = 0x0002, IPP_OP_VALIDATE_JOB = 0x0004, IPP_OP_CANCEL_JOB = 0x0008, IPP_OP_GET_JOB_ATTRIBUTES = 0x0009,
    IPP_OP_GET_JOBS = 0x000a, IPP_OP_GET_PRINTER_ATTRIBUTES = 0x000b,
    IPP_OP_CUPS_GET_DEFAULT = 0x4001, IPP_OP_CUPS_GET_PRINTERS = 0x4002, IPP_OP_CUPS_ADD_MODIFY_PRINTER = 0x4003,
    IPP_OP_CUPS_DELETE_PRINTER = 0x4004, IPP_OP_CUPS_SET_DEFAULT = 0x400a,
};

/* ---- encoding ---- */

typedef struct { uint8_t *p; size_t n, cap; int err; } IppBuf;

void ipp_init(IppBuf *b, uint16_t op_or_status, uint32_t request_id);   /* version 1.1 */
void ipp_free(IppBuf *b);
void ipp_group(IppBuf *b, uint8_t tag);
/* the first value carries the name; the following ones of the same attribute, name = NULL */
void ipp_add_str(IppBuf *b, uint8_t tag, const char *name, const char *value);
void ipp_add_int(IppBuf *b, uint8_t tag, const char *name, int32_t value);   /* INTEGER o ENUM */
void ipp_add_bool(IppBuf *b, const char *name, int value);
void ipp_add_range(IppBuf *b, const char *name, int32_t lo, int32_t hi);
void ipp_end(IppBuf *b);                                                   /* end-of-attributes */
/* common header: attributes-charset, attributes-natural-language, printer-uri (if not NULL)
 * and requesting-user-name */
void ipp_std(IppBuf *b, const char *printer_uri, const char *user);

/* ---- decoding ---- */

typedef struct IppVal IppVal;
typedef struct IppAttr IppAttr;
struct IppVal {
    uint8_t tag;
    const uint8_t *data;        /* points inside the message (or to a NUL-terminated copy) */
    uint16_t len;
    char *str;                  /* texts: NUL-terminated copy */
    int32_t i, i2;              /* INTEGER/ENUM/BOOLEAN; RANGE (i..i2); RESOLUTION (i x i2) */
    IppAttr *col;               /* BEGCOL: members */
    IppVal *next;
};
struct IppAttr {
    uint8_t group;
    int gseq;                   /* group number in the message (each printer in a list has its own) */
    char *name;
    IppVal *vals;
    int nvals;
    IppAttr *next;
};
typedef struct {
    uint8_t major, minor;
    uint16_t code;              /* status in replies */
    uint32_t request_id;
    IppAttr *attrs;
    size_t used;                /* bytes the message took (what follows is document data) */
} IppMsg;

/* 0 on success; -1 malformed message or larger than the caps. */
int  ipp_parse(const uint8_t *data, size_t len, IppMsg *m);
void ipp_msg_free(IppMsg *m);

/* the first attribute with that name in the group (0 = any group), starting after from */
const IppAttr *ipp_find(const IppMsg *m, const IppAttr *from, uint8_t group, const char *name);
/* within the same group as start (same gseq) */
const IppAttr *ipp_find_in(const IppAttr *start, const char *name);
const char *ipp_str(const IppAttr *a);         /* first value as text or NULL */
int32_t     ipp_int(const IppAttr *a, int32_t def);
const char *ipp_tag_name(uint8_t tag);
const char *ipp_status_name(uint16_t status);

#endif
