import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import socket
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch


SOURCE = Path(__file__).resolve().parents[1] / 'fedora-session.in'
loader = importlib.machinery.SourceFileLoader('session_startup', str(SOURCE))
spec = importlib.util.spec_from_loader(loader.name, loader)
session = importlib.util.module_from_spec(spec)
loader.exec_module(session)


class SessionStartupTests(unittest.TestCase):
    def test_selected_theme_is_passed_directly(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / '.config/axia'
            config.mkdir(parents=True)
            bundle = root / 'themes/custom/theme.bundle.js'
            bundle.parent.mkdir(parents=True)
            bundle.write_text('// test theme')
            (config / 'settings.json').write_text(json.dumps({'theme': 'custom'}))
            env = {'HOME': str(root), 'XDG_CONFIG_HOME': str(root / 'different-config')}
            self.assertEqual(session.theme_args(env, root), ['--theme', str(bundle)])

    def test_invalid_missing_and_unreadable_theme_use_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / '.config/axia'
            config.mkdir(parents=True)
            env = {'HOME': tmp}
            self.assertEqual(session.theme_args(env, root), [])
            # 'classic': the Lord Classic theme was removed; a session that still names it gets Lord Flow
            for value in ['../other', '/tmp/theme', '', 'a' * 65, None, [], 'missing', 'classic']:
                with self.subTest(theme=value):
                    (config / 'settings.json').write_text(json.dumps({'theme': value}))
                    self.assertEqual(session.theme_args(env, root), [])
            for text in ['{broken', 'null', '[]', '{"theme":"' + 'a' * (1024 * 1024) + '"}']:
                (config / 'settings.json').write_text(text)
                self.assertEqual(session.theme_args(env, root), [])

    def test_logs_capture_child_output_and_keep_previous_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = {'HOME': tmp, 'XDG_CACHE_HOME': str(Path(tmp) / 'cache')}
            path = Path(env['XDG_CACHE_HOME']) / 'axia/session.log'
            with session.session_log(env) as stream:
                subprocess.run(['/usr/bin/python3', '-c',
                                'import sys; print("stdout"); print("stderr", file=sys.stderr)'],
                               stdout=stream, stderr=stream, check=True)
            self.assertEqual(set(path.read_text().splitlines()), {'stdout', 'stderr'})
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            with session.session_log(env) as stream:
                stream.write('new session\n')
            self.assertEqual(path.read_text(), 'new session\n')
            self.assertEqual(set(path.with_name('session.previous.log').read_text().splitlines()),
                             {'stdout', 'stderr'})

    def test_log_failure_does_not_prevent_startup(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'not-a-directory'
            path.write_text('keep')
            with patch.object(session.sys, 'stderr'):
                self.assertIsNone(session.session_log({'HOME': tmp, 'XDG_CACHE_HOME': str(path)}))
            self.assertEqual(path.read_text(), 'keep')

    def test_socket_scan_ignores_regular_files_and_stale_locks(self):
        with tempfile.TemporaryDirectory() as tmp, socket.socket(socket.AF_UNIX) as sock:
            root = Path(tmp)
            sock.bind(str(root / 'wayland-0'))
            (root / 'wayland-0.lock').touch()
            self.assertEqual(session.sockets(root), {'wayland-0'})

    def test_wayland_environment_is_published_without_xwayland(self):
        server = Mock(pid=123)
        server.poll.return_value = None
        env = {'HOME': '/tmp', 'XDG_RUNTIME_DIR': '/tmp', 'DBUS_SESSION_BUS_ADDRESS': 'test'}
        with patch.dict(os.environ, env, clear=True), \
                patch.object(session, 'initialize_user_dirs'), \
                patch.object(session, 'session_log', return_value=None), \
                patch.object(session, 'theme_args', return_value=[]), \
                patch.object(session, 'sockets', side_effect=[set(), {'wayland-7'}]), \
                patch.object(session, 'x_display', return_value=''), \
                patch.object(session, 'publish_environment') as publish, \
                patch.object(session, 'wait_for_session', return_value=0), \
                patch.object(session.subprocess, 'Popen', return_value=server), \
                patch.object(session.os, 'geteuid', return_value=1000), \
                patch.object(session.signal, 'signal'), \
                patch.object(session.Path, 'is_file', return_value=False), \
                patch.object(session.time, 'sleep') as sleep:
            self.assertEqual(session.main(), 0)
            publish.assert_called_once()
            self.assertEqual(publish.call_args.args[1:], ('wayland-7', ''))
            sleep.assert_not_called()

    def test_failed_environment_publication_does_not_abort_desktop(self):
        for error in [FileNotFoundError(), subprocess.TimeoutExpired('dbus', 10)]:
            with patch.object(session.subprocess, 'run', side_effect=error), \
                    patch.object(session.sys, 'stderr'):
                env = {}
                session.publish_environment(env, 'wayland-0', '')
                self.assertEqual(env, {'WAYLAND_DISPLAY': 'wayland-0', 'DISPLAY': ''})

    def test_update_notification_is_sent_once_without_ending_session(self):
        server = Mock(pid=123)
        server.poll.return_value = None
        stopped = Mock()
        stopped.wait.side_effect = [False, False, True]
        with patch.object(session, 'executable_replaced', return_value=True), \
                patch.object(session.subprocess, 'run') as run:
            log = Mock()
            session.watch_updates(server, {'LANG': 'pt_BR.UTF-8'}, log, stopped)
            run.assert_called_once()
            self.assertIn('Axia DE foi atualizado', run.call_args.args[0])
            server.terminate.assert_not_called()

    def test_absent_notifier_does_not_end_session(self):
        server = Mock(pid=123)
        server.poll.return_value = None
        stopped = Mock()
        stopped.wait.return_value = False
        with patch.object(session, 'executable_replaced', return_value=True), \
                patch.object(session.subprocess, 'run', side_effect=FileNotFoundError()):
            session.watch_updates(server, {}, Mock(), stopped)

    def test_session_exit_stops_the_update_watcher(self):
        server = Mock(pid=123)
        server.wait.return_value = 7
        self.assertEqual(session.wait_for_session(server, {}, None), 7)

    def test_live_and_missing_processes_are_not_reported_as_replaced(self):
        self.assertFalse(session.executable_replaced(os.getpid()))
        self.assertFalse(session.executable_replaced(999999999))

    def test_replaced_running_executable_is_detected(self):
        with tempfile.TemporaryDirectory() as tmp:
            executable = Path(tmp) / 'sleep'
            shutil.copy2('/usr/bin/sleep', executable)
            with subprocess.Popen([str(executable), '20']) as child:
                try:
                    # Popen has returned from exec, so /proc now refers to sleep.
                    self.assertFalse(session.executable_replaced(child.pid))
                    replacement = Path(tmp) / 'replacement'
                    shutil.copy2('/usr/bin/sleep', replacement)
                    replacement.replace(executable)
                    self.assertTrue(session.executable_replaced(child.pid))
                finally:
                    child.terminate()

    def test_wrapper_starts_a_wayland_only_child_and_captures_its_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bindir = root / 'bin'
            bindir.mkdir()
            runtime = root / 'runtime'
            runtime.mkdir(mode=0o700)
            config = root / '.config'
            config.mkdir()
            (config / 'user-dirs.dirs').touch()
            child = bindir / 'axia-session'
            child.write_text('#!/usr/bin/python3\n'
                             'import os, socket, sys, time\n'
                             's = socket.socket(socket.AF_UNIX)\n'
                             's.bind(os.environ["XDG_RUNTIME_DIR"] + "/wayland-0")\n'
                             'print("test compositor started", file=sys.stderr, flush=True)\n'
                             'time.sleep(0.5)\n')
            child.chmod(0o755)
            publish = bindir / 'dbus-update-activation-environment'
            publish.write_text('#!/usr/bin/python3\n'
                               'import os\n'
                               'from pathlib import Path\n'
                               'Path(os.environ["HOME"], "published").write_text(os.environ["WAYLAND_DISPLAY"])\n')
            publish.chmod(0o755)
            wrapper = root / 'fedora-session'
            wrapper.write_text(SOURCE.read_text().replace('@BINDIR@', str(bindir))
                               .replace('/usr/libexec/polkit-mate-authentication-agent-1', str(root / 'no-agent')))
            env = dict(os.environ, HOME=tmp, XDG_RUNTIME_DIR=str(runtime),
                       XDG_CACHE_HOME=str(root / '.cache'), XDG_CONFIG_HOME=str(config),
                       DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent', PATH=f'{bindir}:/usr/bin')
            options = {}
            if os.geteuid() == 0:
                # The packaging CI runs as root; the desktop must still be
                # exercised as an ordinary user, in its own private directory.
                for path in [root, *root.rglob('*')]:
                    os.chown(path, 65534, 65534)
                options = {'user': 65534, 'group': 65534, 'extra_groups': []}
            result = subprocess.run(['/usr/bin/python3', str(wrapper)], env=env,
                                    capture_output=True, text=True, timeout=5, **options)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / 'published').read_text(), 'wayland-0')
            self.assertIn('test compositor started', (root / '.cache/axia/session.log').read_text())


if __name__ == '__main__':
    unittest.main()
