/* arcpath.h — rules for extracting an archive without leaving the destination folder.
 *
 * These are pure functions (they don't touch the disk) so they can be tested on their own (tests/arcpath_test.c). The
 * helper (h_arc.c) also opens each folder with O_NOFOLLOW from the destination fd, creates the
 * files with O_EXCL and runs under Landlock with write access only below that folder: three layers. */
#ifndef LORD_DOC_ARCPATH_H
#define LORD_DOC_ARCPATH_H

#include <stddef.h>

#define ARC_PATH_MAX  4000
#define ARC_DEPTH_MAX 64

/* Entry name → clean relative path ("a/b/c"), without "." or double slashes.
 * Rejects (-1): empty, absolute, with a ".." component, with control characters, components over
 * 255 bytes, more than ARC_DEPTH_MAX levels or more than ARC_PATH_MAX bytes. 0 if it's fine. */
int arc_clean_path(const char *name, char *out, size_t n);

/* Does a symbolic link at `entry` (already clean) pointing to `target` stay inside the root?
 * Only relative targets that, resolved without looking at the disk from the entry's folder,
 * don't go above the root. 1 yes, 0 no. */
int arc_link_ok(const char *entry, const char *target);

/* Is `path` (clean) at `prefix` or below it? (to extract only a part). */
int arc_under(const char *path, const char *prefix);

/* Safe permissions for what's extracted: no setuid/setgid/sticky nor write for others, and
 * always readable and writable by the owner. */
unsigned arc_file_mode(unsigned mode);
unsigned arc_dir_mode(unsigned mode);

#endif
