/* bidi.h — Unicode bidirectional algorithm (UAX #9) for text that mixes
 * left-to-right scripts (Latin, CJK) with right-to-left ones (Arabic, Hebrew). See bidi.c. */
#ifndef LORD_BIDI_H
#define LORD_BIDI_H

#include <stdint.h>

int bidi_class(uint32_t cp);                        /* BIDI_* from unidata.h */
uint32_t bidi_mirror(uint32_t cp);                  /* "(" ↔ ")" in RTL runs; 0 if none */

/* Direction of the first strong character (rules P2/P3): 0 left to right, 1 right to
 * left, -1 if there is none. */
int bidi_direction(const uint32_t *cp, int n);

/* Levels of a paragraph (rules X1–I2). para: 0 LTR, 1 RTL, -1 automatic (P2/P3, LTR if there are
 * no strong ones). Returns the paragraph level. Characters removed by X9 (embedding
 * marks, BN) get the level of the previous one; removed[i] = 1 if skipped. */
int bidi_levels(const uint32_t *cp, int n, int para, uint8_t *levels, uint8_t *removed);

/* Visual order of the line [start, end) of the paragraph: applies L1 (trailing spaces and
 * separators at the paragraph level) on levels and L2. order[k] = logical index of the
 * character that goes at visual position k (left to right). */
void bidi_line(const uint32_t *cp, uint8_t *levels, int start, int end, int para, int *order);

#endif
