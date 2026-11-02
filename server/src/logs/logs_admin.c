/* logs_admin.c — logs-system: the system journal for an administrator who is not in the
 * systemd-journal/adm groups (Logs app, docs/SYSTEM.md).
 *
 *   logs-system <query>      key=value lines: since, until (seconds or µs), prio (0..7),
 *                            kernel (0|1), match (unit or identifier: [A-Za-z0-9@._:-]),
 *                            grep (text without control characters), limit (1..5000)
 *
 * Replies with one LOG per entry (JSON from jentry_json, with the message cut so it fits in one
 * protocol line) and finally OK <count>. Reads the system*.journal* files with our own reader
 * (logs/journal.c): it runs neither journalctl nor any other program. The paths don't come from
 * the user: they come from /etc/machine-id and journald's fixed names. */
#include "logs_admin.h"
#include "journal.h"

#include <stdlib.h>
#include <string.h>

#define LINE_MAX_JSON 1000              /* admin_say cuts at 1024 */

void logs_act_system(const AdminReq *r, const char *arg, size_t len) {
    (void)r; (void)len;
    JQuery q;
    if (jq_parse(arg, &q, 5000)) { admin_err("logs.badQuery", NULL); return; }
    char **paths;
    int np = jq_files(JSCOPE_SYSTEM, 0, &paths);
    if (np <= 0) { admin_err("logs.noSystemJournal", NULL); return; }
    JEntry *e = NULL;
    long n = jq_run(paths, np, &q, &e);
    jq_files_free(paths, np);
    if (n < 0) { admin_err("logs.readFailed", NULL); return; }
    char buf[LINE_MAX_JSON];
    long sent = 0;
    for (long i = 0; i < n; i++) {
        static const size_t tries[] = { 700, 300, 80 };
        for (size_t k = 0; k < 3; k++)
            if (jentry_json(&e[i], buf, sizeof buf, tries[k])) { admin_say("LOG", "%s", buf); sent++; break; }
    }
    jq_free(e, n);
    admin_say("OK", "%ld", sent);   /* i18n-ok */
}
