/* fonts.h — catalog of installed fonts (TTF/OTF/TTC) to pick the terminal's font.
 *
 * Searches LordOS's assets/fonts, /usr/share/fonts, /usr/local/share/fonts,
 * ~/.local/share/fonts and ~/.fonts. It only reads the name/OS-2/post/hmtx tables with our own
 * parser that validates bounds; stb_truetype loads only the font that is chosen. */
#ifndef LORD_FONTS_H
#define LORD_FONTS_H

typedef struct {
    char family[64], style[48], ps[64];
    char path[512];
    int index;                  /* inside a .ttc */
    int weight, italic, mono, color;
} FontEntry;

void fonts_set_builtin_dir(const char *dir);
/* Scans (once; force = rescan) and returns the catalog. */
const FontEntry *fonts_list(int force, int *count);
/* Best variant of the family for bold/italic; NULL if none exists. */
const FontEntry *fonts_pick(const char *family, int bold, int italic);
/* By PostScript name (as terminal profiles exported by other terminal emulators save it: "JetBrainsMono-Regular"). */
const FontEntry *fonts_by_ps(const char *ps);
/* First installed color font (Noto Color Emoji...). */
const FontEntry *fonts_color(void);

#endif
