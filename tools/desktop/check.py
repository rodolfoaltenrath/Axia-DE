#!/usr/bin/env python3
"""Check that built/staged artifacts form an independent desktop."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import struct
import zlib
import sys

from install import install, REPO, DESKTOP_BINARIES


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, capture_output=True, **kwargs).stdout


def main():
    symbols = run('nm', str(REPO / 'server/build/axia-server'))
    for symbol in ['pkg_act_install', 'pkg_act_update', 'install_act_run', 'net_act_restore',
                   'act_langpack', 'js_pkg_register', 'js_imager_register', 'pin_load',
                   'pkg_act_source_add', 'hw_act_wallpaper_get', 'share_act_mount',
                   'clean_act_apt', 'ssh_act_root']:
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
        install('/usr', '/usr/libexec', tmp)
        staged = Path(tmp) / 'usr/libexec/axia'
        assert set(p.name for p in (staged / 'bin').iterdir()) == {
            name for name in DESKTOP_BINARIES if (REPO / 'server/build' / name).is_file()}
        for name in ['axia-clean', 'axia-lan', 'axia-bench', 'axia-sysreport', 'axia-testbench']:
            assert (staged / 'bin' / name).is_file(), f'missing desktop helper: {name}'
        assert (staged / 'assets/cleaner').is_dir()
        assert (Path(tmp) / 'usr/share/licenses/axia-de/OFL-Noto.txt').is_file()
        assert not (Path(tmp) / 'etc').exists()
        assert not (Path(tmp) / 'usr/lib/systemd').exists()
        assert (Path(tmp) / 'usr/share/wayland-sessions/axia.desktop').is_file()
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
                         for app in ['store', 'settings', 'files']]
                prefs = config / 'settings.json'
                prefs.write_text(json.dumps({'schema': 1, 'debugTiming': True,
                                             'sessionRestore': 'always',
                                             'sessionSaved': {'clean': True, 'windows': saved}}))
                if os.geteuid() == 0:
                    os.chown(prefs, 65534, 65534)
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
                log = (home / '.cache/axia/session.log').read_text()
                assert f'js: theme ../themes/{theme}/theme.bundle.js loaded' in log, log
                for error in ['ReferenceError:', 'TypeError:', 'SyntaxError:', 'code refused']:
                    assert error not in log, log
                for app in ['store', 'settings', 'files']:
                    assert f'timing: {app} window committed' in log, log
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
            # Offline catalog fixture: exercise Rust's asynchronous store and the real
            # SHA-256 callback, without depending on GitHub/network availability in CI.
            video = (REPO / 'server/tests/fixtures/motion-h264.mp4').read_bytes()
            sha = hashlib.sha256(video).hexdigest()
            cached = home / '.local/share/axia/motion' / f'{sha}.mp4'
            cached.parent.mkdir(parents=True, exist_ok=True)
            cached.write_bytes(video)
            cached.chmod(0o600)
            if os.geteuid() == 0:
                for path in [home / '.local', home / '.local/share', home / '.local/share/axia', cached.parent, cached]:
                    os.chown(path, 65534, 65534)
            catalog = {'format': 1, 'base': 'https://raw.githubusercontent.com/lordbasex/LordOS-wallpapers/main', 'items': [{'id': 'testclip', 'variants': [
                {'name': '1080.h264', 'file': 'testclip/1080.h264.mp4', 'sha256': sha, 'size': len(video)}]}]}
            (staged / 'assets/wallpapers/motion.json').write_text(json.dumps(catalog))
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
            assert 'wallpaper integration: passed' in log, log
            assert 'motion storage integration: passed' in log, log
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
    if (REPO / 'server/build/axia-media').is_file():
        print(run(sys.executable, str(REPO / 'server/tests/motion_playback_test.py')), end='')
    print('desktop: no OS backend, manifest valid, installation isolated, headless startup passed')


if __name__ == '__main__':
    main()
