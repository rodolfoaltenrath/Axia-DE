#!/usr/bin/env python3
"""Records the screen of a Proxmox VM as a PNG sequence (for tools/demo/make-video.sh).

Usage (on the Proxmox host):  python3 record.py <vmid> <folder> [fps=5]
Runs until <folder>/STOP is created, or 30 minutes at most. For each frame it sends a `screendump` over the VM's QMP
socket and disconnects right away: that way it does not block the socket for Proxmox (`qm` uses it).
It records each frame's time in <folder>/frames.txt to keep the real speed."""
import json, os, socket, sys, time

vmid, out = sys.argv[1], sys.argv[2]
fps = float(sys.argv[3]) if len(sys.argv) > 3 else 5.0
sock_path = f'/var/run/qemu-server/{vmid}.qmp'
os.makedirs(out, exist_ok=True)


def qmp(cmd):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(3)
    s.connect(sock_path)
    f = s.makefile('rwb')
    f.readline()                                                  # greeting
    for c in ({'execute': 'qmp_capabilities'}, cmd):
        f.write((json.dumps(c) + '\n').encode()); f.flush()
        while True:
            r = json.loads(f.readline())
            if 'return' in r or 'error' in r:
                break
    s.close()
    return r


n, t0 = 0, time.time()
with open(os.path.join(out, 'frames.txt'), 'w') as log:
    while not os.path.exists(os.path.join(out, 'STOP')) and time.time() - t0 < 1800:
        start = time.time()
        name = f'f{n:06d}.png'
        try:
            r = qmp({'execute': 'screendump', 'arguments': {'filename': os.path.join(out, name), 'format': 'png'}})
            if 'return' in r:
                log.write(f'{start - t0:.3f} {name}\n'); log.flush()
                n += 1
        except OSError:
            time.sleep(0.2)                                       # the VM is rebooting
        time.sleep(max(0.0, 1.0 / fps - (time.time() - start)))
print(f'{n} frames in {time.time() - t0:.0f} s')
