#!/usr/bin/env python3
"""demo.py — boots the EXPERIMENTAL immutable LordOS image (tools/immutable/build.sh) in QEMU and
runs the A/B demonstration of docs/IMMUTABLE.md over the serial console:

  1. factory image (version 1): /usr is dm-verity + read-only, /etc and /home writable
  2. signed update to version 2 into slot B, restart, boot counting, entry blessed, data kept
  3. update to version 3 (built broken on purpose) into slot A: it fails its health check,
     systemd-boot spends its tries and falls back to version 2 on its own; it is not reinstalled
  3b. a SHA256SUMS changed on the server without the key is refused before anything is written
  4. a corrupted block of the running /usr is refused by dm-verity (I/O error)

Usage (on the build host, Debian 13 with qemu-system-x86 and ovmf, as root: the work folder is
root's and the demo re-publishes the update folder with build.sh):
  sudo tools/immutable/demo.py [work folder]   # default /var/tmp/lordos-immutable
It never touches the built disk.raw: it boots a copy. The transcript goes to <work>/demo/transcript.txt.
The update server is a local HTTP server on the build host (the guest sees it at 10.0.2.2).
"""
import functools
import http.server
import os
import re
import select
import shutil
import subprocess
import sys
import threading
import time

WORK = sys.argv[1] if len(sys.argv) > 1 else "/var/tmp/lordos-immutable"
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(WORK, "out")
DEMO = os.path.join(WORK, "demo")
PORT = 8642
PROMPT = re.compile(rb"root@lordos-immutable:[^#]*# $")
ANSI = re.compile(rb"\x1b\[[0-9;=?]*[A-Za-z]|\x1b[()][A-Z0-9]|\r")

os.makedirs(DEMO, exist_ok=True)
log = open(os.path.join(DEMO, "transcript.txt"), "wb")
T0 = time.time()


def note(msg):
    line = f"\n##### [{time.time() - T0:6.1f}s] {msg}\n"
    sys.stdout.write(line)
    sys.stdout.flush()
    log.write(line.encode())
    log.flush()


class Guest:
    def __init__(self, disk, vars_fd):
        cmd = [
            "qemu-system-x86_64", "-enable-kvm", "-machine", "q35", "-cpu", "host", "-smp", "2",
            "-m", "1024", "-display", "none", "-serial", "stdio", "-monitor", "none",
            "-drive", "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd",
            "-drive", f"if=pflash,format=raw,file={vars_fd}",
            "-drive", f"file={disk},format=raw,if=virtio,cache=unsafe",
            "-nic", "user,model=virtio-net-pci",
        ]
        self.p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.buf = b""

    def expect(self, pattern, timeout=180):
        rx = re.compile(pattern) if isinstance(pattern, bytes) else pattern
        end = time.time() + timeout
        while True:
            m = rx.search(self.buf)
            if m:
                out = self.buf[: m.end()]
                self.buf = self.buf[m.end():]
                return out
            left = end - time.time()
            if left <= 0:
                raise TimeoutError(f"timeout waiting for {rx.pattern!r}; last output: {self.buf[-600:]!r}")
            r, _, _ = select.select([self.p.stdout], [], [], min(left, 1.0))
            if r:
                chunk = os.read(self.p.stdout.fileno(), 65536)
                if not chunk:
                    raise EOFError("qemu exited")
                clean = ANSI.sub(b"", chunk)
                log.write(clean)
                log.flush()
                self.buf += clean

    def send(self, text):
        self.p.stdin.write(text.encode())
        self.p.stdin.flush()

    def run(self, command, timeout=300):
        # the guest's echo is off (login()); the transcript shows each command as "$ <command>"
        log.write(f"$ {command}\n".encode())
        self.send(command + "\n")
        out = self.expect(PROMPT, timeout)
        return out.decode(errors="replace")

    def login(self, timeout=240):
        """Waits for a fresh boot's automatic serial login and turns the echo off."""
        self.expect(rb"login: root \(automatic login\)", timeout)
        self.expect(PROMPT, 60)
        self.send("stty -echo; bind 'set enable-bracketed-paste off'; export SYSTEMD_COLORS=0 SYSTEMD_PAGER=\n")
        self.expect(PROMPT, 30)

    def reboot(self):
        log.write(b"$ systemctl reboot\n")
        self.send("systemctl reboot\n")

    def stop(self):
        self.p.kill()
        self.p.wait()


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def serve(folder):
    handler = functools.partial(QuietHandler, directory=folder)

    srv = http.server.ThreadingHTTPServer(("0.0.0.0", PORT), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv


def publish(*versions):
    subprocess.run(["sudo", "env", "PATH=/usr/sbin:/usr/bin:/sbin:/bin", os.path.join(HERE, "build.sh"),
                    "publish", WORK, *[str(v) for v in versions]], check=True, stdout=subprocess.DEVNULL)


def main():
    disk = os.path.join(DEMO, "disk.raw")
    vars_fd = os.path.join(DEMO, "OVMF_VARS.fd")
    note("copying the factory image (version 1) and fresh UEFI variables")
    subprocess.run(["cp", "--sparse=always", os.path.join(OUT, "disk.raw"), disk], check=True)
    shutil.copy("/usr/share/OVMF/OVMF_VARS_4M.fd", vars_fd)
    publish(1, 2)
    serve(os.path.join(OUT, "updates"))
    url = f"http://10.0.2.2:{PORT}"

    g = Guest(disk, vars_fd)
    try:
        note("1. first boot of the factory image")
        g.login()
        for c in ["cat /usr/lib/lordos/immutable/VERSION",
                  "/usr/lib/lordos/immutable/update --status",
                  "ls /efi > /dev/null; findmnt -lno TARGET,SOURCE,FSTYPE,OPTIONS | grep -E '^(/|/usr|/efi) ' | cut -c1-110",
                  "veritysetup status usr | sed -n '1,4p;/root hash/p'",
                  "touch /usr/bin/evil; echo exit=$?",
                  "echo 'kept across updates' > /etc/lordos-demo.conf; mkdir -p /home/demo && echo 'my file' > /home/demo/notes.txt; echo ok",
                  "systemctl is-active lordos-health systemd-bless-boot; echo"]:
            g.run(c)

        note("2. signed update to version 2 (into slot B)")
        g.run(f"/usr/lib/lordos/immutable/update {url}")
        g.run("/usr/lib/lordos/immutable/update --status")
        g.reboot()
        g.login()
        note("booted after the update")
        for c in ["cat /usr/lib/lordos/immutable/VERSION",
                  "cat /etc/lordos-demo.conf /home/demo/notes.txt",
                  "systemctl is-active lordos-health systemd-bless-boot; journalctl -b -u systemd-bless-boot -o cat | tail -3",
                  "/usr/lib/lordos/immutable/update --status"]:
            g.run(c)

        note("3. a broken release: version 3 (fails its health check) into slot A")
        publish(1, 2, 3)
        g.run(f"/usr/lib/lordos/immutable/update {url}")
        g.run("/usr/lib/lordos/immutable/update --status")
        g.reboot()
        for attempt in (1, 2, 3):
            g.expect(rb"lordos: this version failed its health check", 240)
            note(f"version 3 failed its health check (attempt {attempt}); the system restarts itself")
            g.expect(rb"Command line: usrhash=", 120)
        g.login(300)
        note("booted on its own after the failed attempts")
        for c in ["cat /usr/lib/lordos/immutable/VERSION",
                  "/usr/lib/lordos/immutable/update --status",
                  "cat /etc/lordos-demo.conf /home/demo/notes.txt",
                  "journalctl -b -1 -u lordos-health -o cat --no-pager | grep health-check",
                  "journalctl --list-boots --no-pager | tail -5",
                  f"/usr/lib/lordos/immutable/update {url}"]:
            g.run(c)

        note("3b. a hostile mirror: SHA256SUMS changed on the server (a version 9 added) without the key")
        sums = os.path.join(OUT, "updates", "SHA256SUMS")
        subprocess.run(["sudo", "sh", "-c", f"echo '{'0' * 64}  lordos_9.efi' >> {sums}"], check=True)
        g.run(f"/usr/lib/lordos/immutable/update {url}; echo exit=$?")
        g.run("/usr/lib/lordos/immutable/update --status")
        publish(1, 2, 3)

        note("4. tampering: corrupt 8 MiB of the running /usr slot's blocks underneath dm-verity")
        for c in ["DATA=$(veritysetup status usr | awk '/data device/ {print $3}'); echo $DATA",
                  "dd if=/dev/urandom of=$DATA bs=1M count=8 seek=40 conv=fsync status=none; echo 3 > /proc/sys/vm/drop_caches; echo written",
                  "find /usr -type f -exec cat {} + > /dev/null 2> /tmp/err; echo read-errors=$(grep -c 'Input/output error' /tmp/err)",
                  "dmesg | grep -m3 'is corrupted'"]:
            g.run(c, 600)
        note("done")
    finally:
        g.stop()
    print(f"transcript: {os.path.join(DEMO, 'transcript.txt')}")


if __name__ == "__main__":
    main()
