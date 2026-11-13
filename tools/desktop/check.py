#!/usr/bin/env python3
"""Check that built/staged artifacts form an independent desktop."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import struct
import zlib
import sys
import time
from clipboard_native_check import check_native

REPO = Path(__file__).resolve().parents[2]


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, capture_output=True, **kwargs).stdout


def run_viewed(args, runtime, **kwargs):
    """Run a private headless session (it paints on its own) until it exits."""
    with subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, **kwargs) as proc:
        try:
            try:
                stdout, stderr = proc.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                raise TimeoutError('Dock smoke session did not finish')
            if proc.returncode:
                raise subprocess.CalledProcessError(proc.returncode, args, stdout, stderr)
        finally:
            if proc.poll() is None:
                proc.terminate()
                proc.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-gpu', action='store_true', help='verify the RPM includes EGL composition')
    args = parser.parse_args()
    symbols = run('nm', str(REPO / 'server/build/axia-server'))
    if args.require_gpu:
        assert ' U eglInitialize' in symbols, 'RPM compositor was built without GPU support'
    for symbol in ['pkg_act_install', 'pkg_act_update', 'install_act_run', 'net_act_restore',
                   'act_langpack', 'js_pkg_register', 'js_imager_register', 'pin_load',
                   'pkg_act_source_add', 'share_act_mount',
                   'clean_act_apt', 'js_clean_register', 'clean_start', 'ssh_act_root']:
        assert symbol not in symbols, f'OS component linked: {symbol}'
    assert 'libseat_open_seat' in symbols, 'desktop must use a host seat backend'
    manifest = json.loads((REPO / 'code-manifest.json').read_text())
    for file in manifest['files']:
        path = REPO / file['path']
        assert path.stat().st_size == file['size']
        assert hashlib.sha256(path.read_bytes()).hexdigest() == file['sha256']
        assert not file['path'].startswith('themes/greeter/')
    for name in ['default', 'classic']:
        bundle = (REPO / f'themes/{name}/theme.bundle.js').read_text()
        assert 'pkg-install-file' not in bundle, 'Debian store bundled into desktop'
        assert 'install-run' not in bundle, 'OS installer bundled into desktop'
    with tempfile.TemporaryDirectory(prefix='axia-stage-') as tmp:
        run('cargo', 'xtask', 'install', '--prefix', '/usr', '--libexecdir', '/usr/libexec',
            '--destdir', tmp, cwd=REPO)
        staged = Path(tmp) / 'usr/libexec/axia'
        assert set(p.name for p in (staged / 'bin').iterdir()) == {
            name for name in run('cargo', 'xtask', 'desktop-binaries', cwd=REPO).split() if (REPO / 'server/build' / name).is_file()}
        for name in ['axia-lan', 'axia-bench', 'axia-sysreport', 'axia-testbench']:
            assert (staged / 'bin' / name).is_file(), f'missing desktop helper: {name}'
        assert not (staged / 'assets/cleaner').exists()
        assert not (staged / 'bin/axia-clean').exists()
        assert (Path(tmp) / 'usr/share/licenses/axia-de/OFL-Noto.txt').is_file()
        assert not (Path(tmp) / 'etc').exists()
        assert not (Path(tmp) / 'usr/lib/systemd').exists()
        assert (Path(tmp) / 'usr/share/wayland-sessions/axia.desktop').is_file()
        for theme in ['Axia', 'Axia-dark']:
            source = REPO / 'assets/gtk' / theme
            installed = Path(tmp) / 'usr/share/themes' / theme
            for file in ['index.theme', 'gtk-3.0/gtk.css']:
                assert (installed / file).read_bytes() == (source / file).read_bytes()
        desktop = (Path(tmp) / 'usr/share/wayland-sessions/axia.desktop').read_text()
        assert 'Exec=/usr/libexec/axia/fedora-session' in desktop
        assert 'DesktopNames=Axia;' in desktop
        wrapper = staged / 'fedora-session'
        assert wrapper.is_file() and os.access(wrapper, os.X_OK)
        assert '@BINDIR@' not in wrapper.read_text()
        assert '/usr/bin/axia-session' in wrapper.read_text()
        assert 'FAILED' not in run(str(staged / 'bin/axia-server'), '--check-code', str(staged))
        Path(tmp).chmod(0o755)
        # An ordinary user can initialize the complete desktop without root or a seat
        # in headless mode. This does not validate DRM/KMS or physical input.
        with tempfile.TemporaryDirectory(prefix='axia-home-') as tmp:
            home = Path(tmp)
            (home / 'runtime').mkdir(mode=0o700)
            config = home / '.config/axia'
            config.mkdir(parents=True)
            env = dict(os.environ, HOME=tmp, USER='nobody', XDG_RUNTIME_DIR=str(home / 'runtime'),
                       XDG_DATA_HOME=str(home / '.local/share'), XDG_CONFIG_HOME=str(home / '.config'),
                       XDG_DATA_DIRS=f"{staged.parents[1] / 'share'}:/usr/local/share:/usr/share",
                       XDG_CACHE_HOME=str(home / '.cache'), XDG_STATE_HOME=str(home / '.local/state'))
            for key in ['SUDO_USER', 'DBUS_SESSION_BUS_ADDRESS', 'WAYLAND_DISPLAY', 'DISPLAY']:
                env.pop(key, None)
            opts = {}
            if os.geteuid() == 0:
                os.chown(home, 65534, 65534)
                os.chown(home / 'runtime', 65534, 65534)
                os.chown(home / '.config', 65534, 65534)
                os.chown(config, 65534, 65534)
                opts = {'user': 65534, 'group': 65534, 'extra_groups': []}
            for theme in ['default', 'classic']:
                # Exercise real app rendering through upstream's session restore:
                # a shell-only startup misses Store/Settings integration errors.
                saved = [{'id': f'app:{app}', 'f': {'x': 20, 'y': 30, 'w': 700, 'h': 500},
                          **({'args': {'section': 'wallpaper'}} if app == 'settings' else {})}
                         for app in ['store', 'settings', 'files', 'calculator']]
                prefs = config / 'settings.json'
                prefs.write_text(json.dumps({'schema': 1, 'debugTiming': True, 'externalWindowStyle': True,
                                             'sessionRestore': 'always',
                                             'sessionSaved': {'clean': True, 'windows': saved}}))
                if os.geteuid() == 0:
                    os.chown(prefs, 65534, 65534)
                # Force Quit is deliberately excluded from session restore. Open its
                # compiled registry entry through the shared host after the theme starts.
                native_target = staged / f'themes/{theme}/theme.bundle.js'
                native_original = native_target.read_bytes()
                native_fixture = native_original + b"\nsetTimeout(() => { const sdk = __lordos.require('@lordos/sdk'); sdk.wm.open('forcequit'); sdk.wm.open('appabout', {args:{key:'calculator'}}); }, 700);\n"
                native_manifest = json.loads((staged / 'code-manifest.json').read_text())
                native_entry = next(e for e in native_manifest['files'] if e['path'] == f'themes/{theme}/theme.bundle.js')
                native_target.write_bytes(native_fixture)
                native_entry.update(size=len(native_fixture), sha256=hashlib.sha256(native_fixture).hexdigest())
                (staged / 'code-manifest.json').write_text(json.dumps(native_manifest))
                try:
                    run(str(staged / 'bin/axia-server'), '--headless', '800x600', '--timeout', '5',
                        '--theme', f'../themes/{theme}/theme.bundle.js',
                        cwd=staged / 'bin', env=env, timeout=25, **opts)
                except subprocess.CalledProcessError as error:
                    log = home / '.cache/axia/session.log'
                    print(error.stderr)
                    if log.exists():
                        print(log.read_text())
                    raise
                native_target.write_bytes(native_original)
                native_entry.update(size=len(native_original), sha256=hashlib.sha256(native_original).hexdigest())
                (staged / 'code-manifest.json').write_text(json.dumps(native_manifest))
                log = (home / '.cache/axia/session.log').read_text()
                toolkit = (home / '.config/glib-2.0/settings/keyfile').read_text()
                assert "gtk-theme='Adwaita" in toolkit, 'standard GTK theme was not selected'
                assert "button-layout=':minimize,maximize,close'" in toolkit, 'standard GTK button order was not synchronized'
                assert f'js: theme ../themes/{theme}/theme.bundle.js loaded' in log, log
                for error in ['ReferenceError:', 'TypeError:', 'SyntaxError:', 'code refused', 'react (error)']:
                    assert error not in log, log
                for app in ['store', 'settings', 'files', 'forcequit', 'calculator', 'appabout']:
                    assert f'timing: {app} window committed' in log, log
            check_native(staged, env, opts)
            # Real PNG with a space in its source name. The bridge must copy, validate
            # and thumbnail it, then persist a theme-independent wallpaper selection.
            def png(w, h, pixels):
                def chunk(kind, value):
                    return struct.pack('>I', len(value)) + kind + value + struct.pack('>I', zlib.crc32(kind + value))
                return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
                        + chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b''))
            inputs = {'wallpaper input.png': png(400, 240, (b'\0' + b'\x20\x60\xa0\xff' * 400) * 240),
                      'bad image.png': b'broken PNG',
                      'huge image.png': png(8192, 8192, b'')}
            for name, data in inputs.items():
                (home / name).write_bytes(data)
            # Exercise the real Rust -> C -> QuickJS -> SDK path, not a JS mock.
            fixture = (REPO / 'tests/switcher-smoke.bundle.js').read_bytes()
            target = staged / 'themes/default/theme.bundle.js'
            target.write_bytes(fixture)
            staged_manifest = json.loads((staged / 'code-manifest.json').read_text())
            for entry in staged_manifest['files']:
                if entry['path'] == 'themes/default/theme.bundle.js':
                    entry['size'] = len(fixture)
                    entry['sha256'] = hashlib.sha256(fixture).hexdigest()
            (staged / 'code-manifest.json').write_text(json.dumps(staged_manifest))
            prefs.write_text(json.dumps({'schema': 1, 'shortcutProfile': 'macos'}))
            run(str(staged / 'bin/axia-server'), '--headless', '1000x700', '--timeout', '5',
                '--theme', '../themes/default/theme.bundle.js',
                cwd=staged / 'bin', env=env, timeout=25, **opts)
            log = (home / '.cache/axia/session.log').read_text()
            assert 'switcher integration: passed' in log, log
            assert 'session power integration: passed' in log, log
            assert 'host power dialog rendering: passed' in log, log
            assert 'wallpaper integration: passed' in log, log
            selected = json.loads(prefs.read_text())
            image = selected['wallpaperUser'][0]
            assert selected['wallpaper'] == image['id']
            assert Path(image['src']).read_bytes() == inputs['wallpaper input.png']
            thumb = Path(image['thumb']).read_bytes()
            width, height = struct.unpack('>II', thumb[16:24])
            assert 0 < width <= 320 and 0 < height <= 180
            assert Path(image['src']).stat().st_mode & 0o777 == 0o600
            # The source can disappear without losing the managed wallpaper.
            (home / 'wallpaper input.png').unlink()
            assert Path(image['src']).is_file()
            for error in ['ReferenceError:', 'TypeError:', 'SyntaxError:', 'react (error)', 'switcher smoke:']:
                assert error not in log, log
            # Real Files UI, document images and disk-notification adapter. Only
            # disk facts/volume replies are mocked; no host disk operation runs.
            fixture = (REPO / 'tests/files-smoke.bundle.js').read_bytes()
            target.write_bytes(fixture)
            for entry in staged_manifest['files']:
                if entry['path'] == 'themes/default/theme.bundle.js':
                    entry['size'] = len(fixture)
                    entry['sha256'] = hashlib.sha256(fixture).hexdigest()
            (staged / 'code-manifest.json').write_text(json.dumps(staged_manifest))
            run(str(staged / 'bin/axia-server'), '--headless', '1000x700', '--timeout', '12',
                '--theme', '../themes/default/theme.bundle.js',
                cwd=staged / 'bin', env=env, timeout=30, **opts)
            log = (home / '.cache/axia/session.log').read_text()
            assert 'files integration: passed' in log, log
            for error in ['files smoke:', 'ReferenceError:', 'TypeError:', 'SyntaxError:', 'react (error)']:
                assert error not in log, log
            # A real legacy history, including PNG, then the full C/Rust/JS path.
            clipdir = home / '.local/share/axia/clipboard'
            clipdir.mkdir(parents=True, exist_ok=True)
            (clipdir / 'img-legacy.png').write_bytes(png(1, 1, b'\0\x20\x60\xa0\xff'))
            (clipdir / 'history.json').write_text(json.dumps({'version': 1, 'items': [
                {'id': 'legacy-text', 'kind': 'text', 'type': 'text', 'text': 'Olá, histórico antigo 👋', 'origin': 'terminal', 'time': time.time() * 1000, 'pinned': True},
                {'id': 'legacy-image', 'kind': 'image', 'type': 'image', 'image': str(clipdir / 'img-legacy.png'), 'origin': 'linux', 'time': time.time() * 1000, 'pinned': False}]}))
            if os.geteuid() == 0:
                for path in [clipdir, clipdir / 'img-legacy.png', clipdir / 'history.json']:
                    os.chown(path, 65534, 65534)
            fixture = (REPO / 'tests/clipboard-smoke.bundle.js').read_bytes()
            target.write_bytes(fixture)
            for entry in staged_manifest['files']:
                if entry['path'] == 'themes/default/theme.bundle.js':
                    entry.update(size=len(fixture), sha256=hashlib.sha256(fixture).hexdigest())
            (staged / 'code-manifest.json').write_text(json.dumps(staged_manifest))
            run_viewed([str(staged / 'bin/axia-server'), '--headless', '1000x700', '--timeout', '15',
                        '--theme', '../themes/default/theme.bundle.js'], home / 'runtime', cwd=staged / 'bin', env=env, **opts)
            log = (home / '.cache/axia/session.log').read_text()
            assert 'clipboard integration: passed' in log, log
            for error in ['clipboard smoke:', 'ReferenceError:', 'TypeError:', 'SyntaxError:', 'react (error)']:
                assert error not in log, log
            # Dock policy, real native thumbnail updates and idle shutdown.
            fixture = (REPO / 'tests/dock-preview-smoke.bundle.js').read_bytes()
            target.write_bytes(fixture)
            for entry in staged_manifest['files']:
                if entry['path'] == 'themes/default/theme.bundle.js':
                    entry['size'] = len(fixture)
                    entry['sha256'] = hashlib.sha256(fixture).hexdigest()
            (staged / 'code-manifest.json').write_text(json.dumps(staged_manifest))
            run_viewed([str(staged / 'bin/axia-server'), '--headless', '1000x700', '--timeout', '14',
                        '--theme', '../themes/default/theme.bundle.js'], home / 'runtime',
                       cwd=staged / 'bin', env=env, **opts)
            log = (home / '.cache/axia/session.log').read_text()
            assert 'dock preview integration: passed' in log, log
            assert 'dock feedback integration: passed' in log, log
            assert 'dock native animation integration: passed' in log, log
            assert 'scheduler compositor integration: passed' in log, log
            for error in ['dock preview smoke:', 'ReferenceError:', 'TypeError:', 'SyntaxError:', 'react (error)']:
                assert error not in log, log
            # The menu bar's menus drawn by the Rust native view, opened and run through JS.
            fixture = (REPO / 'tests/menubar-smoke.bundle.js').read_bytes()
            target.write_bytes(fixture)
            for entry in staged_manifest['files']:
                if entry['path'] == 'themes/default/theme.bundle.js':
                    entry['size'] = len(fixture)
                    entry['sha256'] = hashlib.sha256(fixture).hexdigest()
            (staged / 'code-manifest.json').write_text(json.dumps(staged_manifest))
            run_viewed([str(staged / 'bin/axia-server'), '--headless', '1000x700', '--timeout', '10',
                        '--theme', '../themes/default/theme.bundle.js'], home / 'runtime',
                       cwd=staged / 'bin', env=env, **opts)
            log = (home / '.cache/axia/session.log').read_text()
            assert 'menu bar integration: passed' in log, log
            assert 'title bar integration: passed' in log, log
            assert 'launcher integration: passed' in log, log
            assert 'spotlight integration: passed' in log, log
            for error in ['menu bar smoke:', 'title bar:', 'native launcher:', 'native tree:', 'ReferenceError:', 'TypeError:', 'SyntaxError:', 'react (error)']:
                assert error not in log, log
            # Real isolated app using the same Rust scheduler, driven by actual Wayland callbacks.
            # Mutate only this disposable install's calendar package and its code manifest.
            fixture_app = (REPO / 'server/tests/scheduler_app.js').read_bytes()
            (staged / 'assets/sysapps/calendar/app.js').write_bytes(fixture_app)
            actual_theme = (REPO / 'themes/default/theme.bundle.js').read_bytes()
            actual_theme += b"\nsetTimeout(() => { if (!lord.apps.launchPackage('calendar')) console.error('scheduler app: launch failed'); }, 400);\n"
            target.write_bytes(actual_theme)
            for entry in staged_manifest['files']:
                if entry['path'] == 'assets/sysapps/calendar/app.js':
                    entry.update(size=len(fixture_app), sha256=hashlib.sha256(fixture_app).hexdigest())
                if entry['path'] == 'themes/default/theme.bundle.js':
                    entry.update(size=len(actual_theme), sha256=hashlib.sha256(actual_theme).hexdigest())
            (staged / 'code-manifest.json').write_text(json.dumps(staged_manifest))
            prefs.write_text(json.dumps({'schema': 3, 'sessionRestore': 'off', 'welcomeShown': True}))
            if os.geteuid() == 0:
                os.chown(prefs, 65534, 65534)
            run(str(staged / 'bin/axia-server'), '--headless', '1000x700', '--timeout', '6',
                '--theme', '../themes/default/theme.bundle.js', cwd=staged / 'bin', env=env, timeout=25, **opts)
            log = (home / '.cache/axia/session.log').read_text()
            assert 'scheduler isolated integration: passed' in log, log
            for error in ['scheduler app:', '[error]', 'ReferenceError:', 'TypeError:', 'SyntaxError:', 'react (error)']:
                assert error not in log, log
            child = re.search(r"app 'calendar' started as an isolated process \(pid (\d+)\)", log)
            assert child, log
            try:
                status = (Path('/proc') / child[1] / 'stat').read_text().rsplit(') ', 1)[1].split()
            except FileNotFoundError:
                pass  # Reaped by the host.
            else:
                assert status[0] == 'Z' and int(status[49]) == 0, 'scheduler app did not exit cleanly: ' + log
    print('desktop: no OS backend, manifest valid, installation isolated, headless startup passed')


if __name__ == '__main__':
    main()
