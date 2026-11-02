#!/usr/bin/env python3
"""Generates the default LordOS wallpaper (PNG, standard library only).

Usage: python3 tools/gen-wallpaper.py output.png [width height] [palette]
Palettes: aurora, ocean, sunset, graphite.
"""
import math
import struct
import sys
import zlib

# Each palette: base color (r, g, b) and light blobs (x, y, radius, r, g, b), values 0..1.
PALETTES = {
    "aurora": ((0.06, 0.05, 0.20), [
        (0.18, 0.28, 0.55, 0.55, 0.22, 0.90),
        (0.82, 0.22, 0.45, 0.98, 0.33, 0.58),
        (0.62, 0.88, 0.60, 0.16, 0.45, 0.98),
        (0.05, 0.95, 0.40, 0.10, 0.75, 0.85),
    ]),
    "ocean": ((0.02, 0.08, 0.16), [
        (0.25, 0.20, 0.60, 0.05, 0.55, 0.85),
        (0.85, 0.60, 0.55, 0.05, 0.80, 0.70),
        (0.40, 0.95, 0.50, 0.10, 0.30, 0.75),
    ]),
    "sunset": ((0.14, 0.04, 0.10), [
        (0.20, 0.80, 0.60, 0.98, 0.45, 0.20),
        (0.75, 0.30, 0.55, 0.95, 0.25, 0.50),
        (0.50, 0.05, 0.45, 0.55, 0.20, 0.80),
    ]),
    "graphite": ((0.08, 0.08, 0.09), [
        (0.30, 0.30, 0.65, 0.35, 0.36, 0.40),
        (0.80, 0.80, 0.55, 0.22, 0.24, 0.30),
    ]),
}
BASE, BLOBS = PALETTES["aurora"]


def pixel(u, v, aspect):
    r = BASE[0] + 0.08 * u
    g = BASE[1] + 0.04 * v
    b = BASE[2] + 0.14 * (1 - v) - 0.04
    for bx, by, br, cr, cg, cb in BLOBS:
        dx, dy = (u - bx) * aspect, v - by
        k = 1 - math.sqrt(dx * dx + dy * dy) / br
        if k > 0:
            k = k * k * 0.55
            r += cr * k
            g += cg * k
            b += cb * k
    return tuple(min(255, int(c * 255)) for c in (r, g, b))


def main():
    global BASE, BLOBS
    out = sys.argv[1]
    if len(sys.argv) > 4:
        BASE, BLOBS = PALETTES[sys.argv[4]]
    w = int(sys.argv[2]) if len(sys.argv) > 2 else 1920
    h = int(sys.argv[3]) if len(sys.argv) > 3 else 1200
    aspect = w / h
    raw = bytearray()
    prev = bytes(w * 3)
    for y in range(h):
        row = bytearray()
        for x in range(w):
            row.extend(pixel(x / w, y / h, aspect))
        # PNG "Up" filter: compresses gradients much better.
        raw.append(2)
        raw.extend(bytes((row[i] - prev[i]) & 255 for i in range(len(row))))
        prev = bytes(row)

    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)
    print(f"{out}: {w}x{h}, {len(png) // 1024} KB")


if __name__ == "__main__":
    main()
