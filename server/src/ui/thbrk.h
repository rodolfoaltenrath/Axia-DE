/* thbrk.h — Thai word breaking with the libthai-data dictionary (see thbrk.c). */
#ifndef LORD_THBRK_H
#define LORD_THBRK_H

#include <stdint.h>

int thbrk_available(void);              /* is /usr/share/libthai/thbrk.tri there? (loads it the first time) */
int thbrk_is_thai(uint32_t c);          /* Thai letter or mark (what goes to the dictionary) */
/* Between two consecutive characters of the Thai block that are not both letters (ๆ, digits, ฿):
 * 1 can break, 0 cannot, -1 one of them is not in the block. */
int thbrk_pair(uint32_t a, uint32_t b);
/* Breaks inside a Thai run ws[0..len): positions 1..len-1 into out (up to max).
 * Without a dictionary, the syllable starts. */
int thbrk_chunk(const uint32_t *ws, int len, int *out, int max);

/* A whole paragraph: for each place Thai decides, bk[i] = 1 (may break before
 * s[i]) or 2 (may not); the rest stays as it was (0). Runs by dictionary, abbreviations with a period
 * and ๆ, digits and ฿ like libthai. */
void thbrk_text(const uint32_t *s, int n, uint8_t *bk);

#endif
