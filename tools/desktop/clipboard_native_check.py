"""Private desktop/Wayland peer probe. Never connects to the live session."""
import json
import os
from pathlib import Path
import subprocess
import time

REPO = Path(__file__).resolve().parents[2]


def check_native(staged, env, opts):
    subprocess.run(['make', '-C', str(REPO / 'server'), 'build/clipboard-native-protocol'], check=True, capture_output=True)
    home = Path(env['HOME'])
    prefs = home / '.config/axia/settings.json'
    prefs.write_text(json.dumps({'schema': 1, 'sessionRestore': 'off', 'welcomeShown': True}))
    if os.geteuid() == 0:
        os.chown(prefs, 65534, 65534)
    args = [str(staged / 'bin/axia-server'), '--headless', '1000x700', '--timeout', '25', '--theme', '../themes/default/theme.bundle.js']
    with subprocess.Popen(args, cwd=staged / 'bin', env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, **opts) as server:
        try:
            sock = home / 'runtime/wayland-0'
            deadline = time.monotonic() + 10
            while not sock.exists():
                if server.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError('native clipboard: private compositor did not start')
                time.sleep(0.02)
            child = subprocess.run([str(REPO / 'server/build/clipboard-native-protocol')],
                                   env=dict(env, WAYLAND_DISPLAY='wayland-0'), cwd=staged / 'bin',
                                   capture_output=True, text=True, timeout=18, **opts)
            assert child.returncode == 0, child.stdout + child.stderr
            assert 'clipboard native protocol: passed' in child.stdout, child.stdout
        finally:
            server.terminate()
            try:
                _, stderr = server.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                _, stderr = server.communicate(timeout=5)
                raise RuntimeError('native clipboard: session exit stalled')
            if server.returncode:
                raise RuntimeError(stderr)
    history = json.loads((home / '.local/share/axia/clipboard/history.json').read_text())
    assert not any(item.get('text') == 'password test' for item in history['items']), 'Wayland secret reached disk history'
    print('clipboard native protocol: passed (large text, MIME forwarding, owner exit, stale offers, PRIMARY, secrets and PNG)')
