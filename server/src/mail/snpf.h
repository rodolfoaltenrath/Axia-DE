/* snpf.h — formatting into fixed-size fields where cutting the text is intended (folder
 * names, headers shown in the UI). A real function, so -Wformat-truncation stays meaningful for
 * every other snprintf. */
#ifndef LORD_MAIL_SNPF_H
#define LORD_MAIL_SNPF_H
#include <stddef.h>
void snpf(char *dst, size_t cap, const char *fmt, ...) __attribute__((format(printf, 3, 4)));
#define SNPF snpf
#endif
