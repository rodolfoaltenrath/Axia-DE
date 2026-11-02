/* demo.c — test module for the WebAssembly interpreter (tests/wasm_test.c).
 * Build (in the VM): see tests/wasm/build.sh. No libc: just the compiler. */
typedef unsigned int u32;
typedef unsigned long long u64;

__attribute__((import_module("env"), import_name("log"))) void host_log(const char *s, int n);

#define EXPORT(name) __attribute__((export_name(#name)))

EXPORT(fib) int fib(int n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }

EXPORT(fact64) long long fact64(int n) { long long r = 1; for (int i = 2; i <= n; i++) r *= i; return r; }

static unsigned char sieve[2000001];
EXPORT(primes) int primes(int n) {
    if (n > 2000000) n = 2000000;
    __builtin_memset(sieve, 1, (unsigned)n + 1);
    int count = 0;
    for (int i = 2; i <= n; i++) {
        if (!sieve[i]) continue;
        count++;
        for (long long j = (long long)i * i; j <= n; j += i) sieve[j] = 0;
    }
    return count;
}

static int data[4096];
static u32 seed = 12345;
static u32 rnd(void) { seed = seed * 1103515245u + 12345u; return seed >> 8; }
static int cmp_int(int a, int b) { return a < b ? -1 : a > b; }
typedef int (*cmpfn)(int, int);
static void qsort_int(int *a, int lo, int hi, cmpfn cmp) {
    while (lo < hi) {
        int p = a[(lo + hi) / 2], i = lo, j = hi;
        while (i <= j) {
            while (cmp(a[i], p) < 0) i++;
            while (cmp(a[j], p) > 0) j--;
            if (i <= j) { int t = a[i]; a[i] = a[j]; a[j] = t; i++; j--; }
        }
        if (j - lo < hi - i) { qsort_int(a, lo, j, cmp); lo = i; }
        else { qsort_int(a, i, hi, cmp); hi = j; }
    }
}
static volatile cmpfn the_cmp = cmp_int;       /* forces call_indirect */
EXPORT(sort_check) int sort_check(int n) {
    if (n > 4096) n = 4096;
    for (int i = 0; i < n; i++) data[i] = (int)(rnd() % 100000) - 50000;
    qsort_int(data, 0, n - 1, the_cmp);
    for (int i = 1; i < n; i++) if (data[i - 1] > data[i]) return 0;
    return 1;
}

static u32 crc32(const unsigned char *p, int n) {
    u32 c = 0xffffffffu;
    for (int i = 0; i < n; i++) {
        c ^= p[i];
        for (int k = 0; k < 8; k++) c = (c >> 1) ^ (0xedb88320u & -(c & 1));
    }
    return ~c;
}
EXPORT(crc_demo) int crc_demo(int unused) { (void)unused; return (int)crc32((const unsigned char *)"LordOS", 6); }

static int f0(int x) { return x; }
static int f1(int x) { return x + 1; }
static int f2(int x) { return x * 2; }
static int f3(int x) { return x * x; }
static int (*volatile table[4])(int) = { f0, f1, f2, f3 };
EXPORT(dispatch) int dispatch(int i) { return table[i & 3](i); }

EXPORT(fdemo) int fdemo(int n) {
    double r = 1;
    for (int i = 2; i <= n; i++) r *= (double)i;
    float f = (float)r;
    return (int)f;
}

EXPORT(greet) int greet(int unused) {
    (void)unused;
    static const char msg[] = "hola desde C!";
    host_log(msg, sizeof msg - 1);
    return sizeof msg - 1;
}

EXPORT(grow_test) int grow_test(int k) {
    unsigned long before = __builtin_wasm_memory_size(0);
    if (__builtin_wasm_memory_grow(0, (unsigned long)k) != (long)before) return 0;
    if (__builtin_wasm_memory_size(0) != before + (unsigned long)k) return 0;
    volatile unsigned char *last = (unsigned char *)((before + (unsigned long)k) * 65536 - 1);
    *last = 42;
    return *last == 42;
}

static unsigned char buf_a[1000], buf_b[1000];
EXPORT(bulk_test) int bulk_test(int unused) {
    (void)unused;
    __builtin_memset(buf_a, 7, sizeof buf_a);
    __builtin_memcpy(buf_b, buf_a, sizeof buf_a);
    __builtin_memmove(buf_b + 1, buf_b, 500);
    for (int i = 0; i < 1000; i++) if (buf_b[i] != 7) return 0;
    return 1;
}

EXPORT(divide) int divide(int a, int b) { return a / b; }

__attribute__((noinline)) EXPORT(recurse) int recurse(int n) {
    if (n == 0) return 0;
    return recurse(n + 1) * 3 + 1;
}
