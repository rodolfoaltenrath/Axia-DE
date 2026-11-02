/* resolve.h — what has to be installed, upgraded or removed to satisfy a request (like
 * apt's resolver, but simpler: greedy, with bounded backtracking over alternatives). */
#ifndef LORD_PKG_RESOLVE_H
#define LORD_PKG_RESOLVE_H

#include "idx.h"
#include "status.h"

enum { PA_INSTALL = 1, PA_UPGRADE, PA_REMOVE };

typedef struct {
    int action;
    const char *name, *version, *old_version;   /* version: the one that remains (install/upgrade) */
    long rec;                           /* index record to download, or -1 */
    int requested;                      /* requested by the user (not a dependency) */
    int orphan;                         /* dependency left unused (reason "auto") */
    char reason[200];                   /* why (dependency of…, conflict with…) */
} PlanItem;

typedef struct {
    PlanItem *items;
    int n;
    char held[32][64];                  /* upgrades that can't be done now */
    int nheld;
    uint64_t download;                  /* bytes to download */
    int64_t isize_delta;                /* KB by which what's installed changes */
    int ok;
    char err[512];
} Plan;

typedef struct {
    int recommends;                     /* also install the recommends */
    int allow_remove;                   /* may remove packages to resolve conflicts */
    int cascade;                        /* when removing, also remove what depends on it */
    int upgrade_all;                    /* upgrade everything upgradable */
    int orphans;                        /* also remove what was installed as a dependency and is no longer needed */
    const char *const *autos;           /* those installed as a dependency (/var/lib/lordos/pkg/auto) */
    int nautos;
} PlanOpts;

/* install/remove: names (of real packages, or virtual ones for install). */
int  plan_make(const Idx *x, const Status *st, const char *const *install, int ni,
               const char *const *remove, int nr, const PlanOpts *o, Plan *out);
void plan_free(Plan *p);

/* Order for dpkg: indexes of items (install/upgrade) in dependency order, split into
 * batches: each batch is installed with one dpkg --install (a Pre-Depends cuts the batch). group[i]
 * is the batch of the i-th one. Returns the count, or -1. */
int  plan_order(const Idx *x, const Plan *p, int *order, int *group);

/* Candidate version of a package (index record), or -1. inst: the installed one (may be NULL). */
long pkg_candidate(const Idx *x, const char *name, size_t n, const StPkg *inst);

#endif
