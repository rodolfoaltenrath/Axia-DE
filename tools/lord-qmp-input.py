#!/usr/bin/env python3
"""Sends absolute mouse input to a Proxmox VM via QMP. Usage: vmid W H cmd... (move X Y | down | up | rdown | rup | wheelup | wheeldown | sleep S)"""
import json, socket, sys, time
vmid, W, H, cmds = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4:]
s = socket.socket(socket.AF_UNIX); s.connect(f"/var/run/qemu-server/{vmid}.qmp"); f = s.makefile("rw")
def rpc(cmd, args=None):
    f.write(json.dumps({"execute": cmd, **({"arguments": args} if args else {})}) + "\n"); f.flush()
    while True:
        r = json.loads(f.readline())
        if "return" in r or "error" in r: return r
f.readline(); rpc("qmp_capabilities")
def ev(events): rpc("input-send-event", {"events": events}); time.sleep(0.05)
i = 0
while i < len(cmds):
    c = cmds[i]
    if c == "move":
        x, y = int(cmds[i+1]), int(cmds[i+2]); i += 3
        ev([{"type": "abs", "data": {"axis": "x", "value": x * 32767 // (W - 1)}},
            {"type": "abs", "data": {"axis": "y", "value": y * 32767 // (H - 1)}}])
    elif c in ("down", "up", "rdown", "rup"):
        button = "right" if c.startswith("r") else "left"
        ev([{"type": "btn", "data": {"button": button, "down": c.endswith("down")}}]); i += 1
    elif c in ("wheelup", "wheeldown"):
        b = "wheel-up" if c == "wheelup" else "wheel-down"
        ev([{"type": "btn", "data": {"button": b, "down": True}}])
        ev([{"type": "btn", "data": {"button": b, "down": False}}]); i += 1
    elif c == "sleep":
        time.sleep(float(cmds[i+1])); i += 2
