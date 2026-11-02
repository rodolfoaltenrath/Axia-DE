/* lockout.c — failed-login lockout and per-source connection rate (see lockout.h). */
#include "lockout.h"
#include "ipnet.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static RlEntry *find(RlTable *t, const char *key) {
    for (int i = 0; i < RL_MAX; i++) if (t->e[i].key[0] && !strcmp(t->e[i].key, key)) return &t->e[i];
    return NULL;
}

double rl_blocked(const RlTable *t, const char *key, double now) {
    for (int i = 0; i < RL_MAX; i++)
        if (t->e[i].key[0] && !strcmp(t->e[i].key, key)) return t->e[i].until > now ? t->e[i].until - now : 0;
    return 0;
}

double rl_duration(int strikes) { return strikes <= 1 ? 15 * 60.0 : strikes == 2 ? 3600.0 : 24 * 3600.0; }

/* A free slot or, with the table full, the least useful one: never a running block if anything
 * else is left (an attacker cycling addresses must not free their own block), else the block that
 * ends first. */
static RlEntry *slot(RlTable *t, double now) {
    RlEntry *best = NULL;
    for (int i = 0; i < RL_MAX; i++) {
        RlEntry *e = &t->e[i];
        if (!e->key[0]) return e;
        int blocked = e->until > now;
        if (!best) { best = e; continue; }
        int bb = best->until > now;
        if (bb && !blocked) best = e;
        else if (bb == blocked && (blocked ? e->until < best->until : e->last_fail < best->last_fail)) best = e;
    }
    return best;
}

int rl_fail(RlTable *t, const char *key, double now, double *until, int *strikes) {
    RlEntry *e = find(t, key);
    if (!e) {
        e = slot(t, now);
        memset(e, 0, sizeof *e);
        snprintf(e->key, sizeof e->key, "%s", key);
    }
    if (e->fails && now - e->last_fail > RL_WINDOW) e->fails = 0;
    if (e->strikes && now - e->last_strike > RL_WINDOW) e->strikes = 0;
    e->fails++;
    e->last_fail = now;
    if (e->fails < RL_FAILS) return 0;
    e->fails = 0;
    e->strikes++;
    e->last_strike = now;
    e->until = now + rl_duration(e->strikes);
    if (until) *until = e->until;
    if (strikes) *strikes = e->strikes;
    return 1;
}

void rl_success(RlTable *t, const char *key) {
    RlEntry *e = find(t, key);
    if (e) e->fails = 0;
}

int rl_unblock(RlTable *t, const char *cidr) {
    RsCidr c, k;
    if (rs_cidr_parse(cidr, &c)) return 0;
    int n = 0;
    for (int i = 0; i < RL_MAX; i++) {
        RlEntry *e = &t->e[i];
        if (!e->key[0] || rs_key_parse(e->key, &k)) continue;
        /* the request covers the key, or the key (an IPv6 /64) contains the requested address */
        if (rs_cidr_covers(&c, &k) || rs_cidr_covers(&k, &c)) { memset(e, 0, sizeof *e); n++; }
    }
    return n;
}

void rl_expire(RlTable *t, double now) {
    for (int i = 0; i < RL_MAX; i++) {
        RlEntry *e = &t->e[i];
        if (!e->key[0]) continue;
        int live = e->until > now || (e->strikes && now - e->last_strike <= RL_WINDOW) || (e->fails && now - e->last_fail <= RL_WINDOW);
        if (!live) memset(e, 0, sizeof *e);
    }
}

int rl_format(const RlTable *t, double now, char *out, size_t n) {
    int k = snprintf(out, n, "# lord-sessiond: remote desktop lockout (key fails last_fail strikes last_strike until). Do not edit.\n");
    if (k < 0 || (size_t)k >= n) return -1;
    size_t o = (size_t)k;
    for (int i = 0; i < RL_MAX; i++) {
        const RlEntry *e = &t->e[i];
        if (!e->key[0]) continue;
        int live = e->until > now || (e->strikes && now - e->last_strike <= RL_WINDOW) || (e->fails && now - e->last_fail <= RL_WINDOW);
        if (!live) continue;
        k = snprintf(out + o, n - o, "%s %d %.0f %d %.0f %.0f\n", e->key, e->fails, e->last_fail, e->strikes, e->last_strike, e->until);
        if (k < 0 || (size_t)k >= n - o) return -1;
        o += (size_t)k;
    }
    return (int)o;
}

void rl_parse(RlTable *t, const char *text, double now) {
    memset(t, 0, sizeof *t);
    int used = 0;
    const char *p = text ? text : "";
    while (*p && used < RL_MAX) {
        const char *nl = strchr(p, '\n');
        size_t l = nl ? (size_t)(nl - p) : strlen(p);
        char line[160];
        if (l < sizeof line && p[0] != '#') {
            memcpy(line, p, l);
            line[l] = 0;
            char key[48], extra;
            int fails, strikes;
            double lf, ls, until;
            RsCidr c;
            if (sscanf(line, "%47s %d %lf %d %lf %lf %c", key, &fails, &lf, &strikes, &ls, &until, &extra) == 6 && !rs_key_parse(key, &c) &&
                fails >= 0 && fails < RL_FAILS && strikes >= 0 && strikes < 1000 && lf >= 0 && ls >= 0 && until >= 0 &&
                lf <= now + 60 && ls <= now + 60 && until <= now + 2 * RL_WINDOW) {
                char canon[48];
                rs_addr_key(&c.net, canon, sizeof canon);      /* only the canonical spelling */
                if (!strcmp(canon, key) && !find(t, key)) {
                    RlEntry *e = &t->e[used++];
                    snprintf(e->key, sizeof e->key, "%s", key);
                    e->fails = fails;
                    e->strikes = strikes;
                    e->last_fail = lf;
                    e->last_strike = ls;
                    e->until = until;
                }
            }
        }
        p += l;
        if (*p == '\n') p++;
    }
    rl_expire(t, now);
}

double rl_user_delay(int fails) {
    if (fails < 5) return 0;
    int k = fails - 5;
    return k >= 3 ? 8 : (double)(1 << k);
}

int rc_take(RcTable *t, const char *key, double now) {
    RcEntry *e = NULL, *old = &t->e[0];
    for (int i = 0; i < RC_MAX; i++) {
        if (t->e[i].key[0] && !strcmp(t->e[i].key, key)) { e = &t->e[i]; break; }
        if (!t->e[i].key[0]) { if (old->key[0]) old = &t->e[i]; }
        else if (old->key[0] && t->e[i].last < old->last) old = &t->e[i];
    }
    if (!e) {
        e = old;
        memset(e, 0, sizeof *e);
        snprintf(e->key, sizeof e->key, "%s", key);
        e->tokens = RC_BURST;
        e->last = now;
    }
    double dt = now - e->last;
    if (dt > 0) e->tokens += dt * RC_RATE;
    if (e->tokens > RC_BURST) e->tokens = RC_BURST;
    e->last = now;
    if (e->tokens < 1) return 0;
    e->tokens -= 1;
    return 1;
}
