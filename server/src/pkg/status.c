/* status.c — reading /var/lib/dpkg/status (see status.h). */
#include "status.h"
#include "deb.h"
#include "idx.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/utsname.h>

static const char *machine_arch(void) {
    static char arch[16];
    if (arch[0]) return arch;
    struct utsname u;
    const char *a = "amd64";
    if (!uname(&u)) {
        if (!strcmp(u.machine, "aarch64")) a = "arm64";
        else if (!strncmp(u.machine, "armv7", 5)) a = "armhf";
        else if (!strcmp(u.machine, "i686") || !strcmp(u.machine, "i386")) a = "i386";
        else if (!strcmp(u.machine, "riscv64")) a = "riscv64";
        else if (!strcmp(u.machine, "ppc64le")) a = "ppc64el";
        else if (!strcmp(u.machine, "s390x")) a = "s390x";
    }
    snprintf(arch, sizeof arch, "%s", a);
    return arch;
}

/* dpkg's architecture is that of the dpkg package itself (apt does it this way too). */
const char *pkg_native_arch(void) {
    static char arch[16];
    if (arch[0]) return arch;
    FILE *f = fopen(DPKG_STATUS, "r");
    char line[256];
    int in_dpkg = 0;
    while (f && fgets(line, sizeof line, f)) {
        if (!strcmp(line, "Package: dpkg\n")) in_dpkg = 1;
        else if (line[0] == '\n') in_dpkg = 0;
        else if (in_dpkg && !strncmp(line, "Architecture: ", 14)) {
            line[strcspn(line, "\n")] = 0;
            if (deb_valid_name(line + 14, strlen(line + 14))) snprintf(arch, sizeof arch, "%s", line + 14);
            break;
        }
    }
    if (f) fclose(f);
    if (!arch[0]) snprintf(arch, sizeof arch, "%s", machine_arch());
    return arch;
}

static int st_cmp(const void *pa, const void *pb) {
    const StPkg *a = pa, *b = pb;
    int c = strcmp(a->name, b->name);
    if (c) return c;
    return (int)b->native - (int)a->native;
}

void status_free(Status *s) {
    free(s->p);
    free(s->buf);
    memset(s, 0, sizeof *s);
}

int status_load(Status *s, const char *path, const char *arch) {
    struct stat st;
    if (stat(path, &st)) return -1;
    if (s->buf && s->mtime == (long)st.st_mtime && s->size == (long)st.st_size) return 0;
    FILE *f = fopen(path, "rb");
    if (!f) return -1;
    size_t cap = (size_t)st.st_size + 2, len = 0;
    char *buf = malloc(cap);
    if (!buf) { fclose(f); return -1; }
    for (size_t r; len < cap - 1 && (r = fread(buf + len, 1, cap - 1 - len, f)) > 0;) len += r;
    fclose(f);
    buf[len] = 0;
    StPkg *v = NULL;
    int n = 0, vcap = 0;
    const char *pos = buf, *end = buf + len, *sa, *se;
    size_t archlen = strlen(arch);
    while (d822_next_stanza(&pos, end, &sa, &se)) {
        D822 it = { sa, se };
        D822Field fl;
        StPkg p;
        memset(&p, 0, sizeof p);
        struct { const char **dst; const char *v; size_t n; } set[16];
        int ns = 0, present = 0;
        const char *desc = NULL;
        size_t desclen = 0;
#define TAKE(field, member) else if (d822_is(&fl, field) && ns < 16) { set[ns].dst = &p.member; set[ns].v = fl.value; set[ns].n = fl.vlen; ns++; }
        while (d822_next_field(&it, &fl)) {
            if (d822_is(&fl, "Status")) {
                /* "install ok installed": the third word is the status */
                const char *sp = memchr(fl.value, ' ', fl.vlen);
                sp = sp ? memchr(sp + 1, ' ', fl.vlen - (size_t)(sp + 1 - fl.value)) : NULL;
                if (sp) {
                    size_t sl = fl.vlen - (size_t)(sp + 1 - fl.value);
                    const char *w = sp + 1;
                    present = (sl == 9 && !strncmp(w, "installed", 9)) || (sl == 8 && !strncmp(w, "unpacked", 8)) ||
                              (sl == 15 && !strncmp(w, "half-configured", 15)) || !strncmp(w, "triggers-", 9);
                }
            }
            TAKE("Package", name)
            TAKE("Version", version)
            TAKE("Architecture", arch)
            TAKE("Section", section)
            TAKE("Source", source)
            TAKE("Depends", depends)
            TAKE("Pre-Depends", predepends)
            TAKE("Provides", provides)
            TAKE("Conflicts", conflicts)
            TAKE("Breaks", breaks)
            TAKE("Replaces", replaces)
            TAKE("Recommends", recommends)
            else if (d822_is(&fl, "Description")) { desc = fl.value; desclen = fl.vlen; }
            else if (d822_is(&fl, "Installed-Size")) p.isize = (uint32_t)strtoul(fl.value, NULL, 10);
            else if (d822_is(&fl, "Essential") && fl.vlen == 3 && !strncmp(fl.value, "yes", 3)) p.flags |= IF_ESSENTIAL;
            else if (d822_is(&fl, "Protected") && fl.vlen == 3 && !strncmp(fl.value, "yes", 3)) p.flags |= IF_PROTECTED;
            else if (d822_is(&fl, "Multi-Arch")) {
                if (fl.vlen == 4 && !strncmp(fl.value, "same", 4)) p.flags |= IF_MA_SAME;
                else if (fl.vlen == 7 && !strncmp(fl.value, "foreign", 7)) p.flags |= IF_MA_FOREIGN;
                else if (fl.vlen == 7 && !strncmp(fl.value, "allowed", 7)) p.flags |= IF_MA_ALLOWED;
            } else if (d822_is(&fl, "Priority")) {
                static const char *const names[] = { "required", "important", "standard", "optional", "extra" };
                for (int i = 0; i < 5; i++) if (strlen(names[i]) == fl.vlen && !strncmp(fl.value, names[i], fl.vlen)) p.prio = (uint8_t)(i + 1);
            }
        }
#undef TAKE
        if (!present) continue;
        /* the values are terminated in place (the paragraph was already walked) */
        for (int i = 0; i < ns; i++) { *set[i].dst = set[i].v; ((char *)set[i].v)[set[i].n] = 0; }
        if (desc) {
            const char *nl = memchr(desc, '\n', desclen);
            ((char *)desc)[nl ? (size_t)(nl - desc) : desclen] = 0;
            p.summary = desc;
        }
        if (!p.name || !p.version) continue;
        if (!p.arch) p.arch = "";
        p.native = (strlen(p.arch) == archlen && !strcmp(p.arch, arch)) || !strcmp(p.arch, "all");
        if (n == vcap) {
            vcap = vcap ? vcap * 2 : 1024;
            StPkg *nv = realloc(v, sizeof *nv * (size_t)vcap);
            if (!nv) { free(v); free(buf); return -1; }
            v = nv;
        }
        v[n++] = p;
    }
    qsort(v, (size_t)n, sizeof *v, st_cmp);
    status_free(s);
    s->p = v;
    s->n = n;
    s->buf = buf;
    s->mtime = (long)st.st_mtime;
    s->size = (long)st.st_size;
    return 0;
}

const StPkg *status_find(const Status *s, const char *name, size_t n) {
    int lo = 0, hi = s->n;
    while (lo < hi) {
        int mid = (lo + hi) / 2;
        int c = strncmp(s->p[mid].name, name, n);
        if (!c && s->p[mid].name[n]) c = 1;
        if (c < 0) lo = mid + 1; else hi = mid;
    }
    for (int i = lo; i < s->n; i++) {
        if (strncmp(s->p[i].name, name, n) || s->p[i].name[n]) break;
        if (s->p[i].native) return &s->p[i];
    }
    return NULL;
}
