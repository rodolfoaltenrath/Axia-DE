/* csvp.h — CSV reader (RFC 4180) for the importers: quoted fields with "" inside,
 * line breaks inside a field, CRLF or LF, UTF-8 BOM at the start. Fields are copied
 * into the secrets arena (smem.h): a password CSV never goes through the libc heap. */
#ifndef LORD_VAULT_CSVP_H
#define LORD_VAULT_CSVP_H

#include <stddef.h>

typedef struct { char **f; int n, cap; } CsvRow;
typedef struct { const char *s; size_t len, p; int bad; } Csv;

void csv_init(Csv *c, const char *s, size_t len);
/* Next row into row (reused: frees the previous one). 1 = row read, 0 = end, -1 = unclosed quote
 * or out of memory. Empty rows are skipped. */
int  csv_next(Csv *c, CsvRow *row);
void csv_row_free(CsvRow *row);
/* Header column with that name (case-insensitive, ignoring surrounding spaces), or -1. */
int  csv_col(const CsvRow *hdr, const char *name);
/* Field i of the row, or "" if it does not exist. */
const char *csv_get(const CsvRow *row, int i);

#endif
