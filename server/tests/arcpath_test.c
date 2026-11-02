/* arcpath_test.c — safe extraction rules (src/doc/arcpath.c): entry names and
 * symbolic link targets that must not escape the destination folder. */
#include "doc/arcpath.h"

#include <stdio.h>
#include <string.h>

static int fails, total;

static void path(const char *in, const char *want) {
    char out[ARC_PATH_MAX + 1];
    int r = arc_clean_path(in, out, sizeof out);
    total++;
    int ok = want ? (r == 0 && !strcmp(out, want)) : r != 0;
    if (!ok) { fails++; printf("FAIL path(\"%s\"): r=%d out=\"%s\" (expected %s)\n", in, r, r ? "" : out, want ? want : "rejection"); }
}

static void link_(const char *entry, const char *target, int want) {
    total++;
    int r = arc_link_ok(entry, target);
    if (r != want) { fails++; printf("FAIL link(\"%s\" -> \"%s\") = %d (expected %d)\n", entry, target, r, want); }
}

static void under(const char *p, const char *pre, int want) {
    total++;
    if (arc_under(p, pre) != want) { fails++; printf("FAIL under(\"%s\", \"%s\")\n", p, pre); }
}

int main(void) {
    /* nombres normales */
    path("a.txt", "a.txt");
    path("dir/sub/a.txt", "dir/sub/a.txt");
    path("./dir//sub/./a.txt", "dir/sub/a.txt");
    path("dir/", "dir");
    path("ñandú/日本語/файл.txt", "ñandú/日本語/файл.txt");
    path("..foo/bar..", "..foo/bar..");                 /* ".." only as a whole component */
    path("a\\..\\b", "a\\..\\b");                       /* on Linux the backslash is an ordinary character */
    /* what gets rejected */
    path("", NULL);
    path(".", NULL);
    path("./", NULL);
    path("/etc/passwd", NULL);                          /* absoluta */
    path("//etc/passwd", NULL);
    path("../x", NULL);                                 /* sube */
    path("a/../../x", NULL);
    path("a/..", NULL);
    path("a/b/../c", NULL);                             /* even if it stays inside: ".." is not accepted */
    path("a\nb", NULL);                                 /* control */
    path("a\x7f", NULL);
    char deep[400] = "";
    for (int i = 0; i < 70; i++) strcat(deep, "d/");
    path(deep, NULL);                                   /* more than 64 levels */
    char longc[300];
    memset(longc, 'x', 256);
    longc[256] = 0;
    path(longc, NULL);                                  /* component longer than 255 */
    /* symbolic links */
    link_("a/link", "b", 1);
    link_("a/link", "../b", 1);                         /* a/../b = b, adentro */
    link_("a/b/link", "../../c", 1);
    link_("a/b/link", "../../../c", 0);                 /* escapes */
    link_("link", "..", 0);
    link_("link", "../x", 0);
    link_("link", "/etc/passwd", 0);                    /* absoluto */
    link_("link", "", 0);
    link_("a/link", "./b/./c", 1);
    link_("a/link", "b/../../x", 0);                    /* ".." after descending: no */
    link_("a/b/link", "../c/../../d", 0);
    link_("a/link", "x\n", 0);
    /* part of an archive */
    under("a/b/c", "a/b", 1);
    under("a/b", "a/b", 1);
    under("a/bc", "a/b", 0);
    under("x", "", 1);
    /* permisos */
    total++;
    if (arc_file_mode(04777) != 0755 || arc_file_mode(0) != 0600 || arc_dir_mode(02000) != 0700 || arc_file_mode(0666) != 0644) { fails++; printf("FAIL permissions\n"); }
    printf("arcpath: %d/%d\n", total - fails, total);
    return fails ? 1 : 0;
}
