/* Rust evdev ABI against mocked kernel/seat capabilities and real touch ownership.
 * Real nonblocking pipes exercise dispatch and descriptor closure; no host input
 * device is opened or grabbed. */
#include "input.h"
#include "tp/tp.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <time.h>

enum { KEYBOARD = 1, MOUSE, TABLET, TOUCHPAD, TOUCHSCREEN, TOKEN, POWER };
static int kinds[4096], writers[4096], closed, grabbed, ungrabbed, resyncs;
static int scan_sources[3] = {-1, -1, -1}, seat_opens;
int __wrap_stat(const char *path, struct stat *out) {
    int index = -1;
    if (sscanf(path, "/dev/input/event%d", &index) == 1 && index >= 0 && index < 3 && scan_sources[index] >= 0)
        return fstat(scan_sources[index], out);
    errno = ENOENT;
    return -1;
}
static void bit(void *data, int code) {
    unsigned long *words = data;
    words[code / (8 * sizeof *words)] |= 1UL << (code % (8 * sizeof *words));
}
int __wrap_ioctl(int fd, unsigned long request, ...) {
    assert(fd >= 0 && fd < 4096);
    va_list args;
    va_start(args, request);
    if (request == EVIOCGRAB) {
        int on = va_arg(args, int);
        if (on) grabbed++; else ungrabbed++;
        va_end(args);
        return 0;
    }
    void *out = va_arg(args, void *);
    va_end(args);
    if (request == EVIOCSCLOCKID) { assert(*(int *)out == CLOCK_MONOTONIC); return 0; }
    if (_IOC_NR(request) == 0x0a) {
        unsigned code = *(unsigned *)out;
        assert(_IOC_SIZE(request) == 11 * sizeof(int));
        int *values = (int *)out + 1;
        for (int i = 0; i < 10; i++) values[i] = code == ABS_MT_TRACKING_ID ? -1 : 100;
        resyncs++; return 0;
    }
    memset(out, 0, _IOC_SIZE(request));
    int kind = kinds[fd];
    switch (_IOC_NR(request)) {
    case 0x20:
        if (kind) bit(out, EV_KEY);
        if (kind == MOUSE) bit(out, EV_REL);
        if (kind == TABLET || kind == TOUCHPAD || kind == TOUCHSCREEN) bit(out, EV_ABS);
        break;
    case 0x21:
        if (kind == KEYBOARD || kind == TOKEN) {
            for (int code = KEY_ESC; code <= KEY_KPDOT; code++) bit(out, code);
        }
        if (kind == MOUSE) bit(out, BTN_LEFT);
        if (kind == TOUCHPAD) { bit(out, BTN_TOOL_FINGER); bit(out, BTN_LEFT); }
        break;
    case 0x23:
        if (kind == TABLET || kind == TOUCHPAD || kind == TOUCHSCREEN) { bit(out, ABS_X); bit(out, ABS_Y); }
        if (kind == TOUCHPAD || kind == TOUCHSCREEN) { bit(out, ABS_MT_SLOT); bit(out, ABS_MT_POSITION_X); bit(out, ABS_MT_POSITION_Y); }
        break;
    case 0x09:
        if (kind == TOUCHSCREEN) bit(out, INPUT_PROP_DIRECT);
        if (kind == TOUCHPAD) { bit(out, INPUT_PROP_POINTER); bit(out, INPUT_PROP_BUTTONPAD); }
        break;
    case 0x6f:
        ((struct input_absinfo *)out)->maximum = 4;
        break;
    case 0x22:
        if (kind == MOUSE) { bit(out, REL_WHEEL_HI_RES); bit(out, REL_HWHEEL_HI_RES); }
        break;
    case 0x06:
        snprintf(out, _IOC_SIZE(request), "%s", kind == TOKEN ? "Yubico YubiKey" : "ABI fixture");
        break;
    case 0x02:
        ((struct input_id *)out)->bustype = BUS_USB;
        break;
    case 0x40:
    case 0x41:
    case 0x75:
    case 0x76:
        ((struct input_absinfo *)out)->minimum = 10;
        ((struct input_absinfo *)out)->maximum = 1010;
        break;
    default: assert(!"unexpected input ioctl");
    }
    return 0;
}
int seat_device_open(const char *path, int flags) {
    int index = -1;
    assert(sscanf(path, "/dev/input/event%d", &index) == 1 && index >= 0 && index < 3);
    assert((flags & (O_NONBLOCK | O_CLOEXEC)) == (O_NONBLOCK | O_CLOEXEC));
    int source = scan_sources[index];
    assert(source >= 0);
    int fd = fcntl(source, F_DUPFD_CLOEXEC, 0);
    assert(fd >= 0 && fd < 4096);
    assert(fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK) == 0);
    kinds[fd] = kinds[source]; writers[fd] = writers[source];
    seat_opens++;
    return fd;
}
void seat_device_close(int fd) { assert(close(fd) == 0); closed++; }
static int device(int kind) {
    int fds[2]; assert(pipe2(fds, O_CLOEXEC) == 0);
    assert(fds[0] < 4096);
    kinds[fds[0]] = kind; writers[fds[0]] = fds[1];
    return fds[0];
}
static int received;
static void event(void *context, const InputDevice *device, const struct input_event *ev) {
    assert(context == &received);
    assert(ev->type == EV_KEY && ev->code == KEY_A && ev->value == received);
    assert(device->typed == (received > 0));
    received++;
}
static void test_classification_and_dispatch(void) {
    Input input;
    int fds[] = {device(KEYBOARD),device(MOUSE),device(TABLET),device(TOUCHPAD),device(TOUCHSCREEN),device(TOKEN),device(POWER)};
    int initial_closed = closed, initial_touches = tp_count(0);
    assert(input_open_fds(&input, fds, 7) == 0);
    assert(input.count == 6 && closed == initial_closed + 1);
    assert(input.dev[0].fd == fds[0] && input.dev[0].keyboard && input.dev[0].bus == BUS_USB);
    assert(input.dev[1].wheel_hires && input.dev[1].hwheel_hires && !input.dev[1].keyboard);
    assert(input.dev[2].has_abs && input.dev[2].ax.minimum == 10 && input.dev[2].ay.maximum == 1010);
    assert(input.dev[3].tp && !input.dev[3].has_abs);
    assert(input.dev[4].tp && !input.dev[4].has_abs);
    assert(tp_kind(input.dev[3].tp) == TP_KIND_TOUCHPAD && tp_kind(input.dev[4].tp) == TP_KIND_TOUCHSCREEN);
    assert(tp_count(0) == initial_touches + 2);
    struct input_event dropped = {.type=EV_SYN,.code=SYN_DROPPED}, report = {.type=EV_SYN,.code=SYN_REPORT};
    for (int i = 3; i <= 4; i++) {
        tp_event_at(input.dev[i].tp, &dropped, 1000); tp_event_at(input.dev[i].tp, &report, 1012);
    }
    assert(resyncs == 6);
    assert(!input.dev[5].keyboard);
    assert(input_has_keyboard(&input));
    struct input_event events[100] = {0};
    for (int i = 0; i < 100; i++) events[i] = (struct input_event){.type=EV_KEY,.code=KEY_A,.value=i};
    assert(write(writers[fds[0]], events, sizeof events) == sizeof events);
    input_dispatch(&input, 0, event, &received);
    assert(received == 100 && input.dev[0].typed);
    assert(input_watch_dispatch(&input, 0) == 0);
    input_remove(&input, -1); input_remove(&input, 6);
    assert(input.count == 6);
    input_remove(&input, 0);
    assert(input.count == 5 && input.dev[0].fd == fds[1]);
    assert(!input_has_keyboard(&input));
    input_close(&input);
    assert(input.count == 0 && closed == initial_closed + 7 && tp_count(0) == initial_touches);
    int was_closed = closed;
    input_close(&input); assert(closed == was_closed);
    for (int i = 0; i < 7; i++) {
        assert(fcntl(fds[i], F_GETFD) == -1 && errno == EBADF);
        close(writers[fds[i]]);
    }
}
static void test_capacity_and_hotplug_lists(void) {
    Input input;
    assert(input_open_fds(&input, NULL, 0) == -1);
    int fds[INPUT_MAX_DEVICES + 1], before = closed;
    for (int i = 0; i < INPUT_MAX_DEVICES + 1; i++) fds[i] = device(MOUSE);
    assert(input_add_fds(&input, fds, INPUT_MAX_DEVICES + 1) == INPUT_MAX_DEVICES);
    assert(input.count == INPUT_MAX_DEVICES && closed == before + 1);
    for (int i = 0; i < input.count; i++) assert(fcntl(input.dev[i].fd, F_GETFL) & O_NONBLOCK);
    input_close(&input);
    assert(closed == before + INPUT_MAX_DEVICES + 1);
    for (int i = 0; i < INPUT_MAX_DEVICES + 1; i++) close(writers[fds[i]]);
}
static void test_scan_deduplicates_and_rescans_only_when_active(void) {
    int mouse = device(MOUSE), keyboard = device(KEYBOARD);
    scan_sources[0] = scan_sources[1] = mouse;
    Input input;
    assert(input_open(&input) == 0 && input.count == 1 && seat_opens == 1);
    scan_sources[2] = keyboard;
    assert(!input_watch_dispatch(&input, 0) && input.count == 1 && seat_opens == 1);
    assert(input_watch_dispatch(&input, 1) && input.count == 2 && seat_opens == 2);
    assert(!input_watch_dispatch(&input, 1) && input.count == 2 && seat_opens == 2);
    input_remove(&input, 0);
    scan_sources[0] = scan_sources[1] = -1;
    assert(!input_watch_dispatch(&input, 1) && input.count == 1 && seat_opens == 2);
    input_close(&input);
    close(writers[mouse]); close(mouse);
    close(writers[keyboard]); close(keyboard);
    scan_sources[2] = -1;
}
int main(void) {
    test_classification_and_dispatch();
    test_capacity_and_hotplug_lists();
    test_scan_deduplicates_and_rescans_only_when_active();
    assert(grabbed == 40 && ungrabbed == 38);
    puts("input Rust ABI: classification, batches, hotplug, bounds and descriptor lifecycle passed");
}
