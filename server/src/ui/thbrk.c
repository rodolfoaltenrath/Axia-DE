/* thbrk.c — Thai word breaking, like libthai (brk-maximal.c) but without linking it:
 * reads the libthai-data dictionary (/usr/share/libthai/thbrk.tri, a libdatrie double-array
 * trie) with our own reader and looks for the segmentation by "maximal matching": among the
 * possible paths of dictionary words, the one that reaches farthest with the fewest characters
 * outside words and the fewest breaks. If the dictionary is not there, it breaks where a
 * syllable can start (the same hints libthai uses).
 *
 * The file comes from the system, but everything is still validated: sizes, signatures and indices. */
#include "thbrk.h"

#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define DICT_PATH "/usr/share/libthai/thbrk.tri"
#define MAX_FILE (32u << 20)

typedef struct { int32_t base, check; } Cell;
typedef struct { int32_t data; uint8_t *suffix; } TailBlock;    /* suffix ends in 0 */

static struct {
    int state;                  /* 0 not loaded, 1 loaded, -1 absent */
    uint32_t alpha_begin, alpha_end;
    int32_t *alpha_to_trie;     /* -1 = not in the alphabet */
    Cell *cells;
    int32_t ncells;
    TailBlock *tails;
    int32_t ntails;
    uint8_t *tail_mem;
} D;

static uint32_t rd32(const uint8_t *p) { return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; }

static int load(void) {
    int fd = open(DICT_PATH, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    struct stat st;
    uint8_t *f = NULL;
    size_t n = 0;
    if (!fstat(fd, &st) && st.st_size > 0 && (uint64_t)st.st_size <= MAX_FILE) {
        n = (size_t)st.st_size;
        f = malloc(n);
        size_t got = 0;
        while (f && got < n) { ssize_t r = read(fd, f + got, n - got); if (r <= 0) break; got += (size_t)r; }
        if (got != n) { free(f); f = NULL; }
    }
    close(fd);
    if (!f) return -1;
    size_t p = 0;
#define NEED(k) do { if (n - p < (size_t)(k)) goto bad; } while (0)
    /* alphabet: character ranges → trie characters (1, 2, …; 0 is end of word) */
    NEED(8);
    if (rd32(f) != 0xD9FCD9FCu) goto bad;
    uint32_t nranges = rd32(f + 4);
    p = 8;
    if (nranges == 0 || nranges > 1024) goto bad;
    NEED((size_t)nranges * 8);
    uint32_t lo = 0xffffffffu, hi = 0;
    for (uint32_t i = 0; i < nranges; i++) {
        uint32_t b = rd32(f + p + 8 * i), e = rd32(f + p + 8 * i + 4);
        if (b > e || e > 0x10FFFF) goto bad;
        if (b < lo) lo = b;
        if (e > hi) hi = e;
    }
    if (hi - lo > 0xFFFF) goto bad;
    D.alpha_begin = lo;
    D.alpha_end = hi;
    D.alpha_to_trie = malloc(sizeof *D.alpha_to_trie * (hi - lo + 1));
    if (!D.alpha_to_trie) goto bad;
    for (uint32_t c = 0; c <= hi - lo; c++) D.alpha_to_trie[c] = -1;
    int32_t tc = 0;
    for (uint32_t i = 0; i < nranges; i++) {         /* in file order, like libdatrie */
        uint32_t b = rd32(f + p + 8 * i), e = rd32(f + p + 8 * i + 4);
        for (uint32_t a = b; a <= e; a++) {
            if (tc == 0) tc++;
            if (D.alpha_to_trie[a - lo] < 0) D.alpha_to_trie[a - lo] = tc++;
        }
    }
    p += (size_t)nranges * 8;
    /* double array: cell 0 = signature and number of cells; the root is cell 2 */
    NEED(8);
    if (rd32(f + p) != 0xDAFCDAFCu) goto bad;
    D.ncells = (int32_t)rd32(f + p + 4);
    p += 8;
    if (D.ncells < 3 || D.ncells > (int32_t)(MAX_FILE / 8)) goto bad;
    NEED((size_t)(D.ncells - 1) * 8);
    D.cells = malloc(sizeof *D.cells * (size_t)D.ncells);
    if (!D.cells) goto bad;
    D.cells[0].base = D.cells[0].check = 0;
    for (int32_t i = 1; i < D.ncells; i++) {
        D.cells[i].base = (int32_t)rd32(f + p);
        D.cells[i].check = (int32_t)rd32(f + p + 4);
        p += 8;
    }
    /* tails: the rest of each word once it no longer branches */
    NEED(12);
    if (rd32(f + p) != 0xDFFCDFFCu) goto bad;
    D.ntails = (int32_t)rd32(f + p + 8);
    p += 12;
    if (D.ntails < 0 || D.ntails > (int32_t)(MAX_FILE / 10)) goto bad;
    D.tails = calloc((size_t)D.ntails + 1, sizeof *D.tails);
    D.tail_mem = malloc(n - p + (size_t)D.ntails + 1);
    if (!D.tails || !D.tail_mem) goto bad;
    size_t m = 0;
    for (int32_t i = 0; i < D.ntails; i++) {
        NEED(10);
        D.tails[i].data = (int32_t)rd32(f + p + 4);
        int len = (int16_t)((f[p + 8] << 8) | f[p + 9]);
        p += 10;
        if (len < 0) goto bad;
        NEED(len);
        D.tails[i].suffix = D.tail_mem + m;
        memcpy(D.tail_mem + m, f + p, (size_t)len);
        m += (size_t)len;
        D.tail_mem[m++] = 0;
        p += (size_t)len;
    }
#undef NEED
    free(f);
    return 0;
bad:
    free(f);
    free(D.alpha_to_trie); free(D.cells); free(D.tails); free(D.tail_mem);
    memset(&D, 0, sizeof D);
    return -1;
}

int thbrk_available(void) {
    if (!D.state) D.state = load() ? -1 : 1;
    return D.state > 0;
}

/* ---- trie traversal (libdatrie's trie_state_*) ---- */

typedef struct { int32_t index; int16_t suffix_idx; uint8_t is_suffix; } TState;

static int32_t trie_char(uint32_t c) {
    if (!c) return 0;
    return c >= D.alpha_begin && c <= D.alpha_end ? D.alpha_to_trie[c - D.alpha_begin] : -1;
}
static int32_t base_of(int32_t s) { return s >= 0 && s < D.ncells ? D.cells[s].base : 0; }
static int32_t check_of(int32_t s) { return s >= 0 && s < D.ncells ? D.cells[s].check : -1; }
static const uint8_t *suffix_of(int32_t t) { t -= 1; return t >= 0 && t < D.ntails ? D.tails[t].suffix : NULL; }

static int walk(TState *s, uint32_t c) {
    int32_t tc = trie_char(c);
    if (tc < 0) return 0;
    if (!s->is_suffix) {
        int32_t next = base_of(s->index) + tc;
        if (check_of(next) != s->index) return 0;
        s->index = next;
        if (base_of(next) < 0) { s->index = -base_of(next); s->suffix_idx = 0; s->is_suffix = 1; }
        return 1;
    }
    const uint8_t *suf = suffix_of(s->index);
    if (!suf || suf[s->suffix_idx] != tc) return 0;
    if (tc) s->suffix_idx++;
    return 1;
}

static int terminal(const TState *s) {
    if (!s->is_suffix) return check_of(base_of(s->index) + 0) == s->index;
    const uint8_t *suf = suffix_of(s->index);
    return suf && suf[s->suffix_idx] == 0;
}

static TState root(void) { return (TState){ 2, 0, 0 }; }

/* ---- hints: where a syllable can start (libthai's brk_brkpos_hints) ---- */

enum { KO_KAI = 0x0E01, RU = 0x0E24, LU = 0x0E26, WO_WAEN = 0x0E27, O_ANG = 0x0E2D, SARA_A = 0x0E30, MAI_HAN_AKAT = 0x0E31,
       SARA_AA = 0x0E32, SARA_UEE = 0x0E37, SARA_E = 0x0E40, SARA_AE = 0x0E41, MAITAIKHU = 0x0E47, THANTHAKHAT = 0x0E4C };
static int is_cons(uint32_t c) { return c >= 0x0E01 && c <= 0x0E2E && c != RU && c != LU; }
static int is_tone(uint32_t c) { return c >= 0x0E48 && c <= 0x0E4B; }
static int is_upvowel(uint32_t c) { return c == 0x0E31 || (c >= 0x0E34 && c <= 0x0E37); }
static int is_ldvowel(uint32_t c) { return c >= 0x0E40 && c <= 0x0E44; }

static void hints(const uint32_t *s, int len, char *h) {
    memset(h, 0, (size_t)len);
    for (int i = 0; i < len;) {
        if (is_cons(s[i])) {
            if (i + 1 < len && s[i + 1] == THANTHAKHAT) i += 2;
            else if (i + 2 < len && s[i + 2] == THANTHAKHAT) i += 3;
            else if (i + 2 < len && s[i] != KO_KAI && s[i + 1] == MAITAIKHU && (s[i + 2] == O_ANG || s[i + 2] == WO_WAEN)) { h[i] = 1; i += 4; }
            else if ((i > 0 && (s[i - 1] == MAI_HAN_AKAT || s[i - 1] == SARA_UEE)) ||
                     (i > 1 && is_tone(s[i - 1]) && (s[i - 2] == MAI_HAN_AKAT || s[i - 2] == SARA_UEE))) i++;
            else h[i++] = 1;
        } else if (s[i] == SARA_E || s[i] == SARA_AE) {
            h[i] = 1;
            i += 2;
            if (i >= len) break;
            if (s[i] == MAITAIKHU) i += 2;
            else if (is_upvowel(s[i])) {
                i++;
                if (i < len && is_tone(s[i])) i++;
                i++;
            } else if (i + 2 < len && ((s[i + 1] == SARA_AA && s[i + 2] == SARA_A) ||
                                       (s[i] != KO_KAI && s[i + 1] == MAITAIKHU && s[i + 2] != O_ANG && s[i + 2] != WO_WAEN)))
                i += 3;
        } else if (is_ldvowel(s[i])) { h[i] = 1; i += 2; }
        else if (s[i] == RU || s[i] == LU) h[i++] = 1;
        else i++;
    }
}

/* ---- maximal matching (libthai's brk_maximal_do_impl) ---- */

typedef struct Shot {
    struct Shot *next;
    TState st;
    int str_pos, cur, penalty;
    int *brk;
} Shot;

typedef struct { int nbrk; Shot *free_list; int fail; } Env;

static Shot *shot_new(Env *e, const Shot *src) {
    Shot *n = e->free_list;
    if (n) e->free_list = n->next;
    else {
        n = malloc(sizeof *n);
        if (!n) { e->fail = 1; return NULL; }
        n->brk = malloc(sizeof *n->brk * (size_t)(e->nbrk + 1));
        if (!n->brk) { free(n); e->fail = 1; return NULL; }
    }
    n->st = src->st;
    n->str_pos = src->str_pos;
    n->cur = src->cur;
    n->penalty = src->penalty;
    if (src->brk && src->cur) memcpy(n->brk, src->brk, sizeof *n->brk * (size_t)src->cur);
    n->next = NULL;
    return n;
}
static void shot_free(Env *e, Shot *s) { s->next = e->free_list; e->free_list = s; }
static void env_free(Env *e) { while (e->free_list) { Shot *n = e->free_list->next; free(e->free_list->brk); free(e->free_list); e->free_list = n; } }
static Shot *pool_delete(Env *e, Shot *pool, Shot *node) {
    if (pool == node) pool = pool->next;
    else { Shot *p = pool; while (p && p->next != node) p = p->next; if (p) p->next = node->next; }
    shot_free(e, node);
    return pool;
}
static void pool_free(Env *e, Shot *pool) { while (pool) { Shot *n = pool->next; shot_free(e, pool); pool = n; } }
static Shot *pool_get(Shot *pool) {             /* the one that advanced least */
    Shot *chosen = NULL;
    int min = 0x7fffffff;
    for (; pool; pool = pool->next) {
        if (!pool->cur) return pool;
        int p = pool->brk[pool->cur - 1];
        if (p < min) { min = p; chosen = pool; }
    }
    return chosen;
}
static Shot *pool_match(Shot *pool, const Shot *node) {    /* another one that broke at the same place */
    for (; pool; pool = pool->next) {
        if (pool == node) continue;
        if (!node->cur ? !pool->cur : pool->cur > 0 && pool->brk[pool->cur - 1] == node->brk[node->cur - 1]) return pool;
    }
    return NULL;
}
static Shot *root_pool(Env *e, int nbrk) {
    Shot r = { NULL, root(), 0, 0, 0, NULL };
    e->nbrk = nbrk;
    return shot_new(e, &r);
}

#define RECOVERED_WORDS 3

static int recover_try(const uint32_t *ws, int len, const char *h, int words, int *last, Env *e) {
    Env sub = { 0, NULL, 0 };
    Shot *pool = root_pool(&sub, words), *node;
    int ret = 0;
    while (!sub.fail && (node = pool_get(pool))) {
        int keep = 1, term = 0;
        for (;;) {
            do {
                if (!walk(&node->st, ws[node->str_pos++])) { keep = 0; break; }
                term = terminal(&node->st);
                if (node->str_pos >= len) { if (!term) keep = 0; break; }
            } while (!(term && h[node->str_pos]));
            if (!keep) { pool = pool_delete(&sub, pool, node); break; }
            if (node->str_pos < len && !node->st.is_suffix) {
                Shot *nn = shot_new(&sub, node);
                if (nn) { nn->next = pool; pool = nn; node = nn; }
            }
            node->st = root();
            node->brk[node->cur++] = node->str_pos;
            if (node->str_pos == len || node->cur == words) {
                if (node->cur > ret) { ret = node->cur; *last = node->brk[ret - 1]; }
                pool = pool_delete(&sub, pool, node);
                if (ret == words) goto done;
                break;
            }
            Shot *tail = pool, *m;
            while ((m = pool_match(tail, node))) { Shot *nx = m->next; pool = pool_delete(&sub, pool, m); tail = nx; }
        }
    }
done:
    pool_free(&sub, pool);
    env_free(&sub);
    (void)e;
    return ret;
}

typedef struct { int pos, recov; } Hist;

/* From where words can be recognized again (RECOVERED_WORDS in a row) after a
 * run that is not in the dictionary. */
static int recover(const uint32_t *ws, int len, int pos, const char *h, Hist *rh, Env *e) {
    int last = 0;
    while (pos < len && !h[pos]) pos++;
    if (rh->pos == pos) return rh->recov;
    for (int p = pos; p < len; p++)
        if (h[p]) {
            int n = recover_try(ws + p, len - p, h + p, RECOVERED_WORDS, &last, e);
            /* libthai compares ws[last] (last is relative to p) with the end of the text: we keep that */
            if (n == RECOVERED_WORDS || (n > 0 && last >= len)) { rh->pos = pos; rh->recov = p; return p; }
        }
    return -1;
}

int thbrk_chunk(const uint32_t *ws, int len, int *out, int max) {
    if (len <= 1 || max <= 0) return 0;
    char *h = malloc((size_t)len + 1);
    if (!h) return 0;
    hints(ws, len, h);
    if (!thbrk_available()) {                   /* without a dictionary: syllable starts */
        int k = 0;
        for (int i = 1; i < len && k < max; i++) if (h[i]) out[k++] = i;
        free(h);
        return k;
    }
    Env e = { 0, NULL, 0 };
    Shot *pool = root_pool(&e, len + 1), *node;
    int *best = malloc(sizeof *best * (size_t)(len + 1)), nbest = 0, best_pos = 0, best_pen = 0;
    Hist rh = { -1, -1 };
    while (best && !e.fail && (node = pool_get(pool))) {
        int keep = 1, term = 0, recovered = 0, sp = node->str_pos;
        do {
            if (!walk(&node->st, ws[sp++])) {
                term = 0;
                int r = recover(ws, len, node->str_pos + 1, h, &rh, &e);
                if (r != -1) {
                    node->penalty += r;
                    if (node->cur > 0) node->penalty -= node->brk[node->cur - 1];
                    sp = r;
                    recovered = 1;
                } else {
                    node->penalty += len;
                    if (node->cur > 0) node->penalty -= node->brk[node->cur - 1];
                    node->brk[node->cur++] = sp = len;
                    keep = 0;
                }
                break;
            }
            term = terminal(&node->st);
            if (sp >= len) {
                if (!term) {
                    node->penalty += len;
                    if (node->cur > 0) node->penalty -= node->brk[node->cur - 1];
                    node->brk[node->cur++] = len;
                    keep = 0;
                }
                break;
            }
        } while (!(term && h[sp]));
        node->str_pos = sp;
        if (keep && (term || recovered)) {
            if (node->str_pos < len && term && !node->st.is_suffix) {
                Shot *nn = shot_new(&e, node);  /* also the path that follows the longest word */
                if (nn) { nn->next = pool; pool = nn; node = nn; }
            }
            node->st = root();
            node->brk[node->cur++] = node->str_pos;
        }
        if (!keep || node->str_pos == len || node->cur >= len + 1) {
            if (node->str_pos > best_pos || (node->str_pos == best_pos && (node->penalty < best_pen ||
                                             (node->penalty == best_pen && node->cur <= nbest)))) {
                memcpy(best, node->brk, sizeof *best * (size_t)node->cur);
                nbest = node->cur;
                best_pos = node->str_pos;
                best_pen = node->penalty;
            }
            pool = pool_delete(&e, pool, node);
        } else {
            Shot *tail = pool, *m;
            while ((m = pool_match(tail, node))) {
                Shot *nx = m->next, *del;
                if (m->penalty < node->penalty || (m->penalty == node->penalty && m->cur < node->cur)) {
                    del = node;
                    if (nx == node) nx = node->next;
                    node = m;
                } else del = m;
                pool = pool_delete(&e, pool, del);
                tail = nx;
            }
        }
    }
    pool_free(&e, pool);
    env_free(&e);
    int k = 0;
    for (int i = 0; best && i < nbest && k < max; i++) if (best[i] > 0 && best[i] < len) out[k++] = best[i];
    free(best);
    free(h);
    return k;
}

/* libthai's break classes (brk-ctype.c) for the Thai block: only letters and marks
 * (THAI) go to the dictionary; ๆ and ฯ are not separated from what precedes them; digits and ฿ follow the
 * number rules. */
enum { C_THAI, C_ALPHA, C_NUM, C_NBB, C_NUM_CUR, C_TERM, C_OTHER };
static int th_class(uint32_t c) {
    if (c == 0x0E2F || c == 0x0E46) return C_NBB;
    if (c == 0x0E3F) return C_NUM_CUR;
    if (c == 0x0E4F) return C_ALPHA;
    if (c >= 0x0E50 && c <= 0x0E59) return C_NUM;
    if (c == 0x0E5A || c == 0x0E5B) return C_TERM;
    if ((c >= 0x0E01 && c <= 0x0E3A) || (c >= 0x0E40 && c <= 0x0E4E)) return C_THAI;
    return C_OTHER;
}

int thbrk_is_thai(uint32_t c) { return th_class(c) == C_THAI; }

int thbrk_pair(uint32_t a, uint32_t b) {
    int x = th_class(a), y = th_class(b);
    if (x == C_OTHER || y == C_OTHER) return -1;
    /* libthai's _break_table, only these rows and columns (I = indirect: without a space, no) */
    static const uint8_t t[6][6] = {
        /*            THAI ALPHA NUM NBB NUM_CUR TERM */
        /* THAI */    { 0,  1,    0,  0,  1,      0 },
        /* ALPHA */   { 1,  0,    0,  0,  0,      0 },
        /* NUM */     { 1,  0,    0,  0,  0,      0 },
        /* NBB */     { 1,  1,    1,  0,  1,      0 },
        /* NUM_CUR */ { 1,  0,    0,  0,  0,      0 },
        /* TERM */    { 1,  1,    1,  0,  1,      0 },
    };
    return t[x][y];
}

#define MAX_ACRONYM_FRAG 3

static void chunk_breaks(const uint32_t *s, int a, int b, uint8_t *bk) {
    int len = b - a;
    for (int k = a + 1; k < b; k++) bk[k] = 2;
    int *pos = len > 1 ? malloc(sizeof *pos * (size_t)len) : NULL;
    int m = pos ? thbrk_chunk(s + a, len, pos, len) : 0;
    for (int k = 0; k < m; k++) bk[a + pos[k]] = 1;
    free(pos);
}

void thbrk_text(const uint32_t *s, int n, uint8_t *bk) {
    /* between letters and ๆ, digits or ฿: libthai's table */
    for (int i = 1; i < n; i++) {
        int pr = thbrk_pair(s[i - 1], s[i]);
        if (pr >= 0 && !(thbrk_is_thai(s[i - 1]) && thbrk_is_thai(s[i]))) bk[i] = pr ? 1 : 2;
    }
    for (int i = 0; i < n;) {
        if (!thbrk_is_thai(s[i])) { i++; continue; }
        /* abbreviations like libthai: up to 3 letters followed by a period ("พ.ศ.", "กอ.รมน.") are not broken */
        int chunk = i, ae = i, p = i;
        while (p < n) {
            if (thbrk_is_thai(s[p])) {
                if (ae > chunk && p - ae > MAX_ACRONYM_FRAG) break;
                p++;
            } else if (s[p] == '.' && p - ae <= MAX_ACRONYM_FRAG) ae = ++p;
            else break;
        }
        if (ae > chunk) {
            for (int k = chunk + 1; k < ae; k++) bk[k] = 2;
            if (ae < n && thbrk_is_thai(s[ae])) bk[ae] = 1;     /* after the period it can break */
            i = ae;
            continue;
        }
        chunk_breaks(s, chunk, p, bk);
        i = p;
    }
}
