/* ss_memback.h — in-memory keyring for testing secretsvc.c (dbus_test and secretsvc-demo). */
#ifndef SS_MEMBACK_H
#define SS_MEMBACK_H
#include "vault/secretsvc.h"

#define MB_MAX 512
typedef struct {
    int used;
    char id[40];
    char *label;
    char **names, **values;
    int n;
    uint8_t *sec;
    size_t len;
    char ctype[64];
    uint64_t created, modified;
} MbItem;

typedef struct {
    MbItem it[MB_MAX];
    int locked, counter;
    unsigned pending;        /* unlock prompt in progress (0 = none) */
    double pending_at;       /* monotonic ms at which it completes */
    char last_caller[256];
    unsigned last_pid;
} MemBack;

extern const SsBackend memback_ops;
void memback_init(MemBack *b, int locked);
/* completes the pending prompt once 1 s has passed: unlocks and calls ss_prompt_done */
void memback_tick(MemBack *b);
#endif
