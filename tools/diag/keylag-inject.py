#!/usr/bin/env python3
"""keylag-inject.py: types through a virtual uinput keyboard at known times (the latency harness,
docs/TROUBLESHOOTING.md). Needs write access to /dev/uinput (root).

    sudo python3 tools/diag/keylag-inject.py [count] [interval_ms]

Each press is printed as "inject t=<CLOCK_MONOTONIC ms>" right after the write; the key is
released 60 ms later. Keys cycle over a..z so a text field shows them.
"""
import fcntl
import os
import random
import struct
import sys
import time

UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502
EV_SYN, EV_KEY = 0, 1
LETTERS = [30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44]


def ev(fd, etype, code, value):
    t = time.time()
    os.write(fd, struct.pack("llHHi", int(t), int((t % 1) * 1e6), etype, code, value))


def main():
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    interval = float(sys.argv[2]) if len(sys.argv) > 2 else 180.0
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
    for code in range(1, 128):
        fcntl.ioctl(fd, UI_SET_KEYBIT, code)
    # struct uinput_user_dev: name[80], input_id (4 x u16), ff_effects_max, absmax/min/fuzz/flat [64] each
    dev = struct.pack("80sHHHHi", b"keylag virtual keyboard", 3, 0x1234, 0x5678, 1, 0) + b"\0" * (4 * 64 * 4)
    os.write(fd, dev)
    fcntl.ioctl(fd, UI_DEV_CREATE)
    time.sleep(2.0)                 # lord-sessiond sees it (inotify) and lends it to the session
    try:
        for i in range(count):
            code = LETTERS[i % len(LETTERS)]
            ev(fd, EV_KEY, code, 1)
            ev(fd, EV_SYN, 0, 0)
            print("inject t=%.3f" % (time.clock_gettime(time.CLOCK_MONOTONIC) * 1e3), flush=True)
            time.sleep(0.06)
            ev(fd, EV_KEY, code, 0)
            ev(fd, EV_SYN, 0, 0)
            time.sleep(max(0.0, (interval + random.uniform(-40, 40)) / 1e3 - 0.06))
    finally:
        time.sleep(0.5)
        fcntl.ioctl(fd, UI_DEV_DESTROY)
        os.close(fd)


if __name__ == "__main__":
    main()
