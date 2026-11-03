#!/usr/bin/env python3
"""Exercise the actual sandboxed media helper without a display or GPU.
VP8 is available in Fedora/Copr without the optional Cisco H.264 decoder.
Use --h264 for an additional run on systems providing that decoder.
"""
import argparse
import array
import os
from pathlib import Path
import signal
import socket
import struct
import time

ROOT = Path(__file__).resolve().parents[2]
HEADER = struct.Struct('<II6qII')

def run(wallpaper, fixture):
    parent, child = socket.socketpair(socket.AF_UNIX, socket.SOCK_SEQPACKET)
    pid = os.fork()
    if not pid:
        os.dup2(child.fileno(), 3)
        os.set_inheritable(3, True)
        os.execve(str(ROOT / 'server/build/axia-media'), ['axia-media'], {'LANG': 'C.UTF-8'})
    child.close()
    def send(kind, ident=0, values=(), payload=b'', fds=()):
        body = HEADER.pack(kind, ident, *(list(values) + [0] * (6-len(values))), len(payload), 0) + payload
        rights = [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array('i', fds))] if fds else []
        parent.sendmsg([body], rights)
    started = time.monotonic()
    frames = []
    rings = []
    paused = resumed = False
    pause_at = 0
    pause_frames = 0
    try:
        with (ROOT / 'server/tests/fixtures' / fixture).open('rb') as video:
            params = (f'loop=1\naudio=0\nhw=0\nhwonly=0\nwallpaper={int(wallpaper)}\n'
                      'maxfps=15\nmaxwidth=1280\nmaxheight=720\nthreads=2\nrate=100\n').encode()
            send(1, values=[1], payload=params, fds=[video.fileno()])
        send(2, values=[1920, 1080])
        deadline = started + 3.5
        parent.settimeout(0.15)
        while time.monotonic() < deadline:
            now = time.monotonic()
            if wallpaper and paused and not resumed and now-pause_at >= 0.45:
                assert len(frames) <= pause_frames+1, 'frames continued during pause'
                send(3)
                resumed = True
            try:
                data, ancillary, _, _ = parent.recvmsg(65536, socket.CMSG_SPACE(16))
            except TimeoutError:
                continue
            if not data:
                raise AssertionError('media helper exited before playback completed')
            for level, kind, value in ancillary:
                if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
                    rights = array.array('i'); rights.frombytes(value[:len(value)//4*4])
                    for fd in rights: os.close(fd)
            kind, ident, *rest = HEADER.unpack(data[:HEADER.size])
            values = rest[:6]
            if kind == 74:
                raise AssertionError(data[HEADER.size:].decode(errors='replace'))
            if kind == 65:
                rings.append((values[1], values[2]))
            if kind == 66:
                frames.append((time.monotonic(), values[0]))
                send(9, ident)
                if wallpaper and not paused and len(frames) >= 5:
                    send(4); paused=True; pause_at=time.monotonic();pause_frames=len(frames)
        assert rings and len(frames) >= 8, (rings, len(frames))
        if wallpaper:
            assert all(w <= 1280 and h <= 720 for w,h in rings), rings
            assert paused and resumed
            assert len(frames) <= 15*3.5+2, len(frames)
            assert any(b[1] < a[1] for a,b in zip(frames,frames[1:])), 'video did not loop'
        else:
            assert (1920,1080) in rings, 'ordinary videos were incorrectly capped'
            assert len(frames) > 65, 'ordinary video frame rate was capped'
        print(f'motion playback: fixture={fixture}, wallpaper={wallpaper}, {len(frames)} frames, rings={rings}, software decode, pause/loop checked')
    finally:
        parent.close()
        try: os.kill(pid, signal.SIGTERM)
        except ProcessLookupError: pass
        os.waitpid(pid, 0)

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--h264', action='store_true', help='also test the optional H.264 software decoder')
    args = parser.parse_args()
    for fixture in ['motion-vp8.webm'] + (['motion-h264.mp4'] if args.h264 else []):
        run(True, fixture)
        run(False, fixture)
