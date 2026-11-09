/* defaults_test.c — the default app for a MIME type (apps_default_for and preference files in
 * rust/axia-shell/src/applications/) and lord.apps.launchWith('', path) on top of it, over a fake XDG tree: a
 * temporary folder stands for "/" (defaults_set_root) with its own /usr/share/applications,
 * /etc/xdg and /usr/share/mime/globs2, and a temporary HOME.
 *
 * Covers: no app → nothing (the old behaviour); the first installed app that declares the type
 * (by desktop id, then mimeinfo.cache order); Hidden, Terminal, a missing TryExec, an Exec
 * without a file code or with a missing program skipped; the system's mimeapps.list/defaults.list
 * (in the specification's order, an uninstalled entry skipped); the user's choice wins, also as
 * "lordos-<app>.desktop"; [Removed Associations]; going back to automatic; ids and paths outside
 * the launcher's folders ignored; registered NoDisplay MIME helpers can open files without appearing
 * in the launcher. The launched app copies the file it was given to a marker. */
#include "apps.h"
#include "fsops.h"
#include "fw/rules.h"
#include "hw/defaults.h"
#include "hw/mimeapps.h"
#include "sec/broker.h"
#include "sec/perms.h"
#include "sec/restrict.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

void brokersrv_revoke(const char *app, const char *perm) { (void)app; (void)perm; }
PermState perm_get(const char *app_id, Perm p) { (void)app_id; (void)p; return PERM_ASK; }

static int n, bad;
#define CHECK(name, cond) do { n++; if (!(cond)) { bad++; printf("✗ %s\n", name); } } while (0)

static char root[64], home[128], sysapps[256], userapps[256];

static void put(const char *path, const char *text) {
    FILE *f = fopen(path, "w");
    if (!f) { perror(path); exit(1); }
    fputs(text, f);
    fclose(f);
}

/* apps_list() rereads a folder when its mtime changes; the kernel's timestamps move in ticks of a few
 * ms, so a file added in the tick of the last scan would not be seen: wait one out first. */
static void tick(void) {
    struct timespec ts = { 0, 30 * 1000 * 1000 };
    nanosleep(&ts, NULL);
}

/* A system (or user) .desktop whose Exec copies the opened file to $HOME/opened-<id>. */
static void app(int user, const char *id, const char *mime, const char *extra, const char *exec) {
    char path[512], text[1024], ex[512];
    tick();
    if (!exec) { snprintf(ex, sizeof ex, "/bin/cp %%f %s/opened-%s", home, id); exec = ex; }
    snprintf(path, sizeof path, "%s/%s.desktop", user ? userapps : sysapps, id);
    snprintf(text, sizeof text, "[Desktop Entry]\nType=Application\nName=%s\nExec=%s\n%s%s%s%s", id, exec,
             mime ? "MimeType=" : "", mime ? mime : "", mime ? "\n" : "", extra ? extra : "");
    put(path, text);
}

static void rm_app(int user, const char *id) {
    char path[512];
    snprintf(path, sizeof path, "%s/%s.desktop", user ? userapps : sysapps, id);
    tick();
    unlink(path);
}

static void sysfile(const char *rel, const char *text) {
    char path[512];
    snprintf(path, sizeof path, "%s%s", root, rel);
    if (text) put(path, text); else unlink(path);
}

static void userlist(const char *text) {
    char path[512];
    snprintf(path, sizeof path, "%s/.config/mimeapps.list", home);
    if (text) put(path, text); else unlink(path);
}

static int is(const char *mime, const char *want, int kind) {
    char id[DESKTOP_ID_MAX] = "";
    int r = apps_default_for(mime, id, sizeof id);
    if (want ? r != kind || strcmp(id, want) : r != 0) {
        printf("  %s: got %d '%s', want %d '%s'\n", mime, r, id, kind, want ? want : "");
        return 0;
    }
    return 1;
}

static int opened(const char *id) {
    char path[512];
    snprintf(path, sizeof path, "%s/opened-%s", home, id);
    for (int i = 0; i < 100; i++) {
        if (!access(path, F_OK)) return 1;
        struct timespec ts = { 0, 20 * 1000 * 1000 };
        nanosleep(&ts, NULL);
    }
    return 0;
}

int main(void) {
    snprintf(root, sizeof root, "/tmp/lord-defaults-XXXXXX");
    if (!mkdtemp(root)) { perror("mkdtemp"); return 1; }
    snprintf(home, sizeof home, "%s/home", root);
    const char *dirs[] = { "/usr", "/usr/share", "/usr/share/applications", "/usr/share/mime", "/usr/local", "/usr/local/share",
                           "/usr/local/share/applications", "/etc", "/etc/xdg", "/opt", "/home", "/home/.config", "/home/.local",
                           "/home/.local/share", "/home/.local/share/applications", NULL };
    char path[512];
    for (int i = 0; dirs[i]; i++) { snprintf(path, sizeof path, "%s%s", root, dirs[i]); mkdir(path, 0700); }
    snprintf(sysapps, sizeof sysapps, "%s/usr/share/applications", root);
    snprintf(userapps, sizeof userapps, "%s/.local/share/applications", home);
    setenv("HOME", home, 1);
    unsetenv("XDG_CONFIG_HOME");
    unsetenv("LORDOS_SESSIOND");
    defaults_set_root(root);
    char real[PATH_MAX];
    if (!realpath(home, real)) { perror(home); return 1; }
    fsops_init(real, "nobody-here");            /* the roots launchWith accepts */
    sysfile("/usr/share/mime/globs2", "50:text/html:*.html\n50:text/plain:*.txt\n50:image/png:*.png\n50:application/x-rpm:*.rpm\n");
    restrict_set_store(fw_store_new());
    char page[256];
    snprintf(page, sizeof page, "%s/page.html", home);
    put(page, "<p>hi</p>\n");

    char id[DESKTOP_ID_MAX];

    /* 1. nothing installed opens it: nothing (the old behaviour) */
    CHECK("no app: none", is("text/html", NULL, 0));
    CHECK("no app: launchWith('') refuses", apps_launch_with("", page) != 0);

    /* 2. the apps that can't be used are skipped */
    app(0, "aa-hidden", "text/html;", "Hidden=true\n", NULL);
    app(0, "ab-nodisplay", "text/html;", "NoDisplay=true\n", NULL);
    app(0, "ac-terminal", "text/html;", "Terminal=true\n", NULL);
    app(0, "ad-tryexec", "text/html;", "TryExec=/nonexistent/lord-browser\n", NULL);
    app(0, "ae-nofile", "text/html;", NULL, "/bin/cp /etc/hostname /tmp/lord-defaults-never");
    app(0, "af-noprog", "text/html;", NULL, "/nonexistent/lord-browser %u");
    app(0, "ag-other", "text/plain;", NULL, NULL);
    app(0, "ah-onlyshow", "text/html;", "OnlyShowIn=GNOME;\n", NULL);
    CHECK("unusable apps: none", is("text/html", NULL, 0));
    snprintf(path, sizeof path, "%s/opt/applications", root);
    mkdir(path, 0700);
    snprintf(path, sizeof path, "%s/opt/applications/outside.desktop", root);
    put(path, "[Desktop Entry]\nType=Application\nName=Outside\nExec=/bin/cp %f /tmp/x\nMimeType=text/html;\n");
    sysfile("/usr/share/applications/mimeinfo.cache", "[MIME Cache]\ntext/html=ghost.desktop;outside.desktop;aa-hidden.desktop;\n");
    sysfile("/usr/share/applications/defaults.list", "[Default Applications]\ntext/html=outside.desktop;../../opt/applications/outside.desktop;\n");
    CHECK("ids outside the launcher's folders: none", is("text/html", NULL, 0));

    /* 3. the first installed app that declares the type: by desktop id without a cache entry */
    app(0, "zeta-browser", "text/html;x-scheme-handler/https;", NULL, NULL);
    app(0, "beta-browser", "text/html;", NULL, NULL);
    CHECK("first declaring app, by id", is("text/html", "beta-browser.desktop", APPS_DEFAULT_DECLARED));
    CHECK("it declares only what it declares", is("image/png", NULL, 0));
    CHECK("launchWith('') opens it", apps_launch_with("", page) == 0 && opened("beta-browser"));
    CHECK("a path outside home is refused", apps_launch_with("", "/etc/hostname") != 0);
    /* mimeinfo.cache order comes first */
    sysfile("/usr/share/applications/mimeinfo.cache", "[MIME Cache]\ntext/html=ghost.desktop;zeta-browser.desktop;beta-browser.desktop;\n");
    CHECK("mimeinfo.cache order", is("text/html", "zeta-browser.desktop", APPS_DEFAULT_DECLARED));
    /* the user's own folder: its cache is read first; a user app shadows a system one of the same id */
    app(1, "user-browser", "text/html;", NULL, NULL);
    snprintf(path, sizeof path, "%s/mimeinfo.cache", userapps);
    put(path, "[MIME Cache]\ntext/html=user-browser.desktop;\n");
    CHECK("the user's mimeinfo.cache first", is("text/html", "user-browser.desktop", APPS_DEFAULT_DECLARED));
    unlink(path);
    rm_app(1, "user-browser");

    /* 4. what the system marks as default */
    sysfile("/usr/share/applications/defaults.list", "[Default Applications]\ntext/html=beta-browser.desktop;\n");
    CHECK("defaults.list", is("text/html", "beta-browser.desktop", APPS_DEFAULT_LISTED));
    sysfile("/usr/share/applications/mimeapps.list", "[Default Applications]\ntext/html=ghost.desktop;aa-hidden.desktop;zeta-browser.desktop;\n");
    CHECK("mimeapps.list before defaults.list, uninstalled entries skipped", is("text/html", "zeta-browser.desktop", APPS_DEFAULT_LISTED));
    app(0, "mail-only", "x-scheme-handler/mailto;", NULL, NULL);
    sysfile("/etc/xdg/mimeapps.list", "[Default Applications]\ntext/html=mail-only.desktop;\n");
    CHECK("/etc/xdg first; a default need not declare the type", is("text/html", "mail-only.desktop", APPS_DEFAULT_LISTED));
    sysfile("/etc/xdg/mimeapps.list", "[Default Applications]\ntext/html=lordos-photos.desktop;\n");
    CHECK("a system lordos- entry is not installed: skipped", is("text/html", "zeta-browser.desktop", APPS_DEFAULT_LISTED));
    sysfile("/etc/xdg/mimeapps.list", NULL);

    /* 5. the user's choice wins */
    userlist("[Default Applications]\ntext/html=beta-browser.desktop;\n");
    CHECK("the user's choice", is("text/html", "beta-browser.desktop", APPS_DEFAULT_USER));
    CHECK("lord.mime.get sees it", defaults_lookup("text/html", id, sizeof id) == 0 && !strcmp(id, "beta-browser.desktop"));
    userlist("[Default Applications]\ntext/html=lordos-editor.desktop;\n");
    CHECK("the user's lordos- choice", is("text/html", "lordos-editor.desktop", APPS_DEFAULT_USER));
    CHECK("launchWith('') leaves it to the theme", apps_launch_with("", page) != 0);
    userlist("[Default Applications]\ntext/html=ghost.desktop;\n");
    CHECK("an uninstalled user choice falls through", is("text/html", "zeta-browser.desktop", APPS_DEFAULT_LISTED));
    userlist("[Removed Associations]\ntext/html=zeta-browser.desktop;\n");
    CHECK("the user's Removed Associations apply to the system's lists", is("text/html", "beta-browser.desktop", APPS_DEFAULT_LISTED));
    userlist("[Added Associations]\ntext/html=mail-only.desktop;\n");
    CHECK("Added Associations after every default", is("text/html", "zeta-browser.desktop", APPS_DEFAULT_LISTED));
    sysfile("/usr/share/applications/mimeapps.list", NULL);
    sysfile("/usr/share/applications/defaults.list", NULL);
    CHECK("Added Associations before the declaring apps", is("text/html", "mail-only.desktop", APPS_DEFAULT_LISTED));

    /* 6. choosing and going back to automatic */
    userlist("# kept\n[Default Applications]\ntext/plain=ag-other.desktop;\n");
    CHECK("set", defaults_set("text/html", "beta-browser.desktop") == 0);
    CHECK("set: explicit", is("text/html", "beta-browser.desktop", APPS_DEFAULT_USER));
    CHECK("unset", defaults_unset("text/html") == 0);
    CHECK("unset: automatic again", is("text/html", "zeta-browser.desktop", APPS_DEFAULT_DECLARED));
    CHECK("unset kept the other lines", defaults_lookup("text/plain", id, sizeof id) == 0 && !strcmp(id, "ag-other.desktop"));
    userlist(NULL);
    CHECK("unset with no file", defaults_unset("text/html") == 0);
    CHECK("unset refuses a bad type", defaults_unset("../x") != 0);
    CHECK("launchWith('') opens the automatic app", apps_launch_with("", page) == 0 && opened("zeta-browser"));

    /* Package installers use NoDisplay: MIME associations can use them without launcher entries. */
    app(0, "package-installer", "application/x-rpm;", "NoDisplay=true\n", NULL);
    sysfile("/usr/share/applications/mimeinfo.cache", "[MIME Cache]\napplication/x-rpm=package-installer.desktop;\n");
    CHECK("NoDisplay MIME handler resolves", is("application/x-rpm", "package-installer.desktop", APPS_DEFAULT_DECLARED));
    const DesktopApp *listed;
    int count = apps_list("en", &listed), hidden_listed = 0;
    for (int i = 0; i < count; i++) if (!strcmp(listed[i].id, "package-installer")) hidden_listed = 1;
    CHECK("NoDisplay helper stays out of launcher", !hidden_listed);
    CHECK("NoDisplay helper cannot be launched without a file", apps_launch("package-installer") != 0);
    char package[256];
    snprintf(package, sizeof package, "%s/package with spaces.rpm", home);
    put(package, "harmless file association test\n");
    CHECK("RPM opens registered helper with the intact filename", apps_launch_with("", package) == 0 && opened("package-installer"));
    app(1, "package-installer", "application/x-rpm;", "Hidden=true\nNoDisplay=true\n", NULL);
    CHECK("Hidden user entry masks system handler", is("application/x-rpm", NULL, 0));
    CHECK("Hidden user entry cannot launch explicitly", apps_launch_with("package-installer", package) != 0);
    app(1, "package-installer", "application/x-rpm;", "NoDisplay=true\nTryExec=/nonexistent/installer\n", NULL);
    CHECK("NoDisplay still honors TryExec", is("application/x-rpm", NULL, 0));
    app(1, "package-installer", "application/x-rpm;", "NoDisplay=true\n", NULL);
    snprintf(path, sizeof path, "%s/opened-package-installer", home); unlink(path);
    CHECK("user NoDisplay MIME handler launches", apps_launch_with("", package) == 0 && opened("package-installer"));

    char cmd[600];
    snprintf(cmd, sizeof cmd, "rm -rf '%s'", root);
    if (system(cmd)) {}
    printf(bad ? "defaults: %d of %d failed\n" : "defaults: %d/%d ok\n", bad ? bad : n, n);
    return bad ? 1 : 0;
}
