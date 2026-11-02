/* lockout.h — failed-login lockout and connection limits of the remote desktop (docs/REMOTE.md,
 * "Security analysis (0.8.3)"). Plain libc and no clock of its own (the caller passes `now`), so
 * the rules are tested in build/remote-sec-test.
 *
 * Lockout, per source (an IPv4 address, or the /64 of an IPv6 one; ipnet.h rs_addr_key):
 *   - 3 failed logins without a successful one in between (within 24 h) block the source;
 *   - the block lasts 15 minutes, then 1 hour, then 24 hours for a source blocked again within
 *     24 hours of its previous block ("strikes");
 *   - a successful login clears the failure count (not the strikes);
 *   - there is no per-account lockout (it would let anybody lock the real user out): accounts only
 *     get a slowdown (rl_user_delay).
 * The table lives in lord-sessiond's shared memory (root processes only) and is saved to
 * /var/lib/lordos/remote/blocklist (root, 0600) so blocks survive a restart. */
#ifndef LORD_REMOTE_LOCKOUT_H
#define LORD_REMOTE_LOCKOUT_H

#include <stddef.h>

#define RL_MAX 256
#define RL_FAILS 3                      /* failures that block */
#define RL_WINDOW (24 * 3600.0)         /* failures and strikes older than this are forgotten */
#define RL_FILE_DIR "/var/lib/lordos/remote"
#define RL_FILE RL_FILE_DIR "/blocklist"
#define RL_REQUESTS RL_FILE_DIR "/requests"   /* "unblock <cidr>" lines from the admin helper */

typedef struct {
    char key[48];
    int fails, strikes;
    double last_fail, last_strike, until;
} RlEntry;

typedef struct { RlEntry e[RL_MAX]; } RlTable;

/* Seconds the block of that key still lasts (0 = not blocked). */
double rl_blocked(const RlTable *t, const char *key, double now);
/* A failed login. Returns 1 if this failure started a block (*until = its end, *strikes = how many
 * blocks within 24 h, this one included), else 0. */
int    rl_fail(RlTable *t, const char *key, double now, double *until, int *strikes);
void   rl_success(RlTable *t, const char *key);
/* Block duration for the n-th block (1-based) within 24 h. */
double rl_duration(int strikes);
/* Removes the entries whose key lies inside `cidr` (text, ipnet.h); returns how many. */
int    rl_unblock(RlTable *t, const char *cidr);
void   rl_expire(RlTable *t, double now);
/* Text of the blocklist file (lord-sessiond writes it with an atomic rename, 0600). rl_format
 * returns the length or -1; rl_parse replaces the table and skips invalid, non-canonical and expired
 * lines. */
int    rl_format(const RlTable *t, double now, char *out, size_t n);
void   rl_parse(RlTable *t, const char *text, double now);

/* Per-account slowdown, in seconds, before verifying a password after `fails` failures for that
 * name (from any address): none for the first 4, then 1, 2, 4, 8, at most 8. */
double rl_user_delay(int fails);

/* New connections per source: a token bucket (burst RC_BURST, then RC_RATE per second). */
#define RC_MAX 128
#define RC_BURST 20.0
#define RC_RATE 2.0
typedef struct { char key[48]; double tokens, last; } RcEntry;
typedef struct { RcEntry e[RC_MAX]; } RcTable;
int    rc_take(RcTable *t, const char *key, double now);        /* 1 = allowed */

#endif
