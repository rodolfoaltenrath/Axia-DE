/* vgen.h — password and passphrase generator, and the strength estimate used by the
 * security report. All offline: the "breach" check runs against the list of the 10,000
 * most common passwords shipped with LordOS (and, if the user chooses, against a local
 * Have I Been Pwned file: SHA-1 sorted by hash, binary search in the file). */
#ifndef LORD_VAULT_VGEN_H
#define LORD_VAULT_VGEN_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    int length;                 /* password: 4–128 */
    int upper, lower, digits, symbols;
    int no_ambiguous;           /* without Il1O0o */
    int words;                  /* passphrase: 3–12 words (0 = password) */
    char sep[8];                /* passphrase separator ("-") */
    int capitalize, add_digit;  /* passphrase: Capitalize Words, a number in one word */
} VGenOpts;

/* Embedded lists (EFF words and common passwords), one per line. */
void   vg_set_lists(const char *words, size_t wlen, const char *common, size_t clen);
int    vg_generate(const VGenOpts *o, char *out, size_t cap, double *bits);   /* 0 on success */
/* Strength: 0 (very weak) – 4 (very strong) and estimated bits. */
int    vg_strength(const char *pw, double *bits);
/* Position in the common list (1 = the most common) or 0 if absent (also tries without
 * capitals, without trailing digits/symbols and without "leet": p4ssw0rd → password). */
int    vg_common_rank(const char *pw);
/* Local HIBP file (fd of "pwned-passwords-sha1-ordered-by-hash", lines "HASH:count").
 * Returns the number of times it appears, 0 if not, -1 if the file is unusable. */
long   vg_hibp_lookup(int fd, const char *pw);
size_t vg_word_count(void);

#endif
