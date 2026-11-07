"""Check clipboard XWayland transfers in a temporary desktop installation.

Requires built bundles/server, Xwayland and the X11 development library.
The display comes from the private session log, never from the live desktop.
"""
import json
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import tempfile
import threading
import time

from check import run

REPO = Path(__file__).resolve().parents[2]


def main():
    run('make', '-C', str(REPO / 'server'), 'build/clipboard-x11-protocol')
    with tempfile.TemporaryDirectory(prefix='axia-x11-stage-') as stage, \
            tempfile.TemporaryDirectory(prefix='axia-x11-home-') as tmp:
        run('cargo', 'xtask', 'install', '--prefix', '/usr', '--libexecdir', '/usr/libexec',
            '--destdir', stage, cwd=REPO)
        Path(stage).chmod(0o755)
        staged = Path(stage) / 'usr/libexec/axia'
        home = Path(tmp)
        runtime = home / 'runtime'
        runtime.mkdir(mode=0o700)
        config = home / '.config/axia'
        config.mkdir(parents=True)
        (config / 'settings.json').write_text(json.dumps({
            'schema': 1, 'sessionRestore': 'off', 'welcomeShown': True,
        }))
        opts = {}
        if os.geteuid() == 0:
            os.chown(home, 65534, 65534)
            for path in home.rglob('*'):
                os.chown(path, 65534, 65534)
            opts = dict(user=65534, group=65534, extra_groups=[])
        env = dict(os.environ, HOME=tmp, USER='nobody', XDG_RUNTIME_DIR=str(runtime),
                   XDG_CONFIG_HOME=str(home / '.config'), XDG_DATA_HOME=str(home / '.local/share'),
                   XDG_CACHE_HOME=str(home / '.cache'), XDG_STATE_HOME=str(home / '.local/state'))
        for key in ['SUDO_USER', 'DBUS_SESSION_BUS_ADDRESS', 'WAYLAND_DISPLAY', 'DISPLAY']:
            env.pop(key, None)
        args = [str(staged / 'bin/axia-server'), '--headless', '1000x700', '--timeout', '30',
                '--theme', '../themes/default/theme.bundle.js']
        with subprocess.Popen(args, cwd=staged / 'bin', env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.PIPE, text=True, **opts) as server:
            try:
                with socket.socket(socket.AF_UNIX) as viewer:
                    deadline = time.monotonic() + 10
                    while True:
                        try:
                            viewer.connect(str(runtime / 'axia/remote.sock'))
                            break
                        except (FileNotFoundError, ConnectionRefusedError):
                            assert server.poll() is None and time.monotonic() < deadline, 'private compositor did not start'
                            time.sleep(0.02)
                    payload = struct.pack('<BBHI', 2, 0, 24, 300)
                    viewer.sendall(struct.pack('<IB', len(payload) + 1, 32) + payload)
                    viewer.settimeout(0.2)
                    done = threading.Event()

                    def drain():
                        while not done.is_set():
                            try:
                                if not viewer.recv(65536):
                                    break
                            except socket.timeout:
                                pass
                            except OSError:
                                break

                    reader = threading.Thread(target=drain, daemon=True)
                    reader.start()
                    try:
                        log = home / '.cache/axia/session.log'
                        while True:
                            content = log.read_text() if log.exists() else ''
                            match = re.search(r'xwayland: DISPLAY=(:\d+)', content)
                            if match:
                                break
                            assert time.monotonic() < deadline, content
                            time.sleep(0.02)
                        child = subprocess.run([str(REPO / 'server/build/clipboard-x11-protocol')],
                                               env=dict(env, DISPLAY=match.group(1)), capture_output=True,
                                               text=True, timeout=22, **opts)
                        print(child.stdout + child.stderr)
                        assert child.returncode == 0, log.read_text()
                    finally:
                        done.set()
                        reader.join(timeout=1)
            finally:
                server.terminate()
                try:
                    _, stderr = server.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.communicate(timeout=5)
                    raise RuntimeError('private XWayland session did not exit')
                if server.returncode:
                    raise RuntimeError(stderr)


if __name__ == '__main__':
    main()
