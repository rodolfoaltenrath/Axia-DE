/* ime.h — input methods for Korean, Japanese and Chinese (see ime.c). */
#ifndef LORD_IME_H
#define LORD_IME_H

#include <stddef.h>

#include "keyboard.h"

#define IME_MAX_CAND 64
#define IME_PAGE 9

typedef struct {
    char preedit[512];          /* text being composed (UTF-8), "" if none */
    int ncand, sel;             /* candidates and the chosen one */
    char cand[IME_MAX_CAND][96];
    int converting;             /* Japanese: showing the conversion */
    int focus_begin, focus_end; /* bytes of the focused segment within preedit (Japanese) */
} ImeState;

/* "" (none), "hangul", "kana", "pinyin". 0 ok; 1 ok but without dictionary (kana without
 * skkdic: only hiragana/katakana; pinyin without dictionary is useless); -1 unknown. */
int  ime_set(const char *name);
const char *ime_current(void);
/* A text field or a terminal has focus: otherwise keys pass through unchanged. */
void ime_context(int on);
/* Processes a key. Returns 1 if it consumed it. commit: finished text to deliver. */
int  ime_key(const KeyInfo *k, char *commit, size_t size);
void ime_reset(void);                       /* discards the composition */
const ImeState *ime_state(void);
int  ime_changed(void);                     /* the state changed since the last query */
const char *ime_dict_path(void);            /* loaded dictionary ("" if none) */

#endif
