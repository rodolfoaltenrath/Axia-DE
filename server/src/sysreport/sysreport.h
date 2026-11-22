/* sysreport.h — System Information's collector (0.8.5, see docs/BENCHMARK.md): what hardware this
 * machine has and what the kernel made of it, read without root from world-readable files (sysfs,
 * /proc, the device tree, os-release, the kernel config). Used by `axia-sysreport` and `axia-fetch`
 * and by `lord.sysinfo`; nothing is written or sent anywhere. All of it is Rust
 * (rust/axia-sysreport); this is its C ABI.
 *
 * Privacy by construction: the report is built from a fixed list of fields, which have no place for
 * serial numbers, UUIDs, MAC or IP addresses, host or user names, Wi-Fi network names or home-folder
 * paths (network interfaces are reported by kind and driver, not by name, as a name can carry the MAC).
 * The Rust tests fill fake systems with such values and check that none reaches the JSON, the
 * Markdown or the summary.
 *
 * `root` prefixes every path ("" or NULL for this machine; tests point it at a fake tree). Facts that
 * only the running system can give (the Landlock ABI) are left unknown under a fake root. `lordos` is
 * LordOS's version, which the caller knows. Every string returned is malloc'd (the caller frees it),
 * NULL on out-of-memory. */
#ifndef LORD_SYSREPORT_H
#define LORD_SYSREPORT_H

/* The report as JSON (schema 1) or as a Markdown summary. */
char *sysreport_json(const char *root, const char *lordos);
char *sysreport_markdown(const char *root, const char *lordos);

/* `axia-fetch`: the Axia mark and a summary of this computer for a terminal, plus a few live facts
 * (uptime, memory and disk in use, packages, shell). The user and host names appear only on screen and
 * only without share, so a screenshot made for sharing carries neither. color: 0 plain, 1 256 colors,
 * 2 true color. */
char *sysreport_fetch(const char *root, const char *lordos, int share, int color);
/* How to color stdout: opt is "always", "never", "auto" or NULL (auto: a terminal, no NO_COLOR, TERM
 * not "dumb"; true color when COLORTERM says so). The result is a color for sysreport_fetch. */
int sysreport_color_mode(const char *opt, int is_tty);

/* Whether the kernel command line holds the whole word w (or w=… when w ends in '='). */
int sysreport_cmdline_has(const char *cmdline, const char *w);

#endif
