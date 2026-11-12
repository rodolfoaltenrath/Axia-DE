/* Timing-only Rust trace through the native ABI, with a deterministic clock.
 * Each case runs in a fresh child so no trace state crosses privacy boundaries. */
#include "keylag.h"
#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static double tick_ms = 100000.0;
int __wrap_clock_gettime(clockid_t clock, struct timespec *out) {
    double value = tick_ms + (clock == CLOCK_REALTIME ? 900000.0 : 0.0);
    out->tv_sec = (time_t)(value / 1000.0);
    out->tv_nsec = (long)((value - out->tv_sec * 1000.0) * 1000000.0);
    tick_ms += 0.125;
    return 0;
}
static void receive(int value) {
    double real_ms = tick_ms + 900000.0;
    struct timeval tv = {.tv_sec=(time_t)(real_ms/1000.0)};
    tv.tv_usec = (long)((real_ms - tv.tv_sec * 1000.0) * 1000.0);
    keylag_rx(value, &tv);
}
static char *read_log(const char *path) {
    FILE *file = fopen(path, "rb"); assert(file);
    assert(!fseek(file, 0, SEEK_END)); long size = ftell(file); assert(size >= 0);
    rewind(file); char *text = calloc((size_t)size + 1, 1); assert(text);
    assert(fread(text, 1, (size_t)size, file) == (size_t)size);
    fclose(file); return text;
}
static void frame(void) {
    keylag_flushed(); keylag_commit(); keylag_render(); keylag_presented(100001000000ULL);
}
static void scenario(int mode) {
    char directory[] = "/tmp/axia-keylatency-ffi-XXXXXX";
    assert(mkdtemp(directory));
    char path[512]; snprintf(path, sizeof path, "%s/axia-keylag.log", directory);
    assert(!setenv("XDG_RUNTIME_DIR", directory, 1));
    assert(!setenv("LORD_INPUT_DEBUG", "1", 1));
    keylag_init(1); assert(!keylag_enabled());
    keylag_rx(1, NULL); // Disabled tracing must not dereference an event.
    keylag_init(0); assert(keylag_enabled());
    if (mode == 0) {
        keylag_rx(2, NULL); // Autorepeat must not dereference an event either.
        receive(1); keylag_sent_wl(0);
        receive(1); keylag_handled(); keylag_sent_wl(1);
        assert(access(path, F_OK) == -1);
        tick_ms = 100000.0;
        receive(1); keylag_sent_wl(1); frame();
        struct stat st; assert(!stat(path, &st) && (st.st_mode & 0777) == 0600);
        char *text = read_log(path);
        assert(!strcmp(text, "keylag start: times in ms after the kernel's evdev timestamp\nkeylag n=1 ev=99999.875 press rx=0.12 sent=0.38 flush=0.50 commit=0.62 shown=1.12\n"));
        free(text);
        tick_ms = 220000.25; keylag_presented(100001000000ULL); assert(keylag_enabled());
        tick_ms = 220000.5; keylag_presented(100001000000ULL); assert(!keylag_enabled());
        text = read_log(path); assert(strstr(text, "keylag stop: 120 s window over\n")); free(text);
    } else if (mode == 1) {
        for (int i = 0; i < 33; i++) { receive(1); keylag_sent_wl(1); }
        tick_ms += 2001.0; keylag_presented(100001000000ULL);
        char *text = read_log(path);
        unsigned lines = 0; for (const char *p = text; *p; p++) lines += *p == '\n';
        assert(lines == 34);
        assert(strstr(text, "n=33 ") && strstr(text, "flush=-1.00 commit=-1.00 shown=-1.00"));
        free(text);
    } else {
        for (int i = 0; i < 4000; i++) { receive(i & 1); keylag_sent_wl(1); frame(); }
        assert(!keylag_enabled());
        char *text = read_log(path);
        unsigned lines = 0; for (const char *p = text; *p; p++) lines += *p == '\n';
        assert(lines == 4002 && strstr(text, "n=4000 ") && strstr(text, "keylag stop: line limit\n"));
        free(text);
    }
    unlink(path); rmdir(directory);
}
int main(void) {
    for (int mode = 0; mode < 3; mode++) {
        pid_t child = fork(); assert(child >= 0);
        if (!child) { scenario(mode); _exit(0); }
        int status; assert(waitpid(child, &status, 0) == child);
        assert(WIFEXITED(status) && WEXITSTATUS(status) == 0);
    }
    puts("key latency Rust ABI: privacy gates, exact timing, queue, expiry and line cap passed");
}
