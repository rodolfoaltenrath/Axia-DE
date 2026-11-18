#!/usr/bin/env python3
"""Measure a private software-rendered session's main-loop polling and RSS."""
import argparse
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import statistics
import subprocess
import tempfile
import time


def poll_count(text, since, until):
    count = 0
    blocked = False
    for line in text.splitlines():
        match = re.match(r'^(\d+\.\d+)\s+(poll|ppoll|epoll_wait|epoll_pwait)\(.*\)\s+=\s+', line)
        if match and '<unfinished ...>' not in line:
            start = float(match[1])
            if since <= start < until:
                count += 1
            elapsed = re.search(r'<(\d+\.\d+)>\s*$', line)
            if elapsed and start <= since and start + float(elapsed[1]) >= until:
                blocked = True
    if not count and not blocked:
        raise RuntimeError('no complete main-thread poll calls in the collection window')
    return count


def evaluate(samples, expected, max_polls, max_rss):
    if len(samples) != expected or not samples:
        raise RuntimeError('incomplete sample set')
    polls = statistics.median(s['poll_calls_per_second'] for s in samples)
    rss = statistics.median(s['rss_bytes_median'] for s in samples)
    exceeded = []
    if polls > max_polls:
        exceeded.append('poll_calls_per_second')
    if rss > max_rss:
        exceeded.append('rss_bytes_median')
    return {'poll_calls_per_second': polls, 'rss_bytes_median': rss,
            'exceeded': exceeded}


def identity():
    if os.geteuid() == 0:
        import pwd
        user = pwd.getpwnam('nobody')
        return user.pw_uid, user.pw_gid
    return os.geteuid(), os.getegid()


def owned_directory(path, uid, gid):
    path.mkdir(mode=0o700, parents=True)
    os.chown(path, uid, gid)


def collect(installation, run, artifacts, warmup, duration):
    uid, gid = identity()
    home = run / 'home'
    for directory in (home, home / '.config', home / '.config/axia', run / 'runtime'):
        owned_directory(directory, uid, gid)
    settings = home / '.config/axia/settings.json'
    settings.write_text(json.dumps({'welcomeShown': True, 'theme': 'default', 'mode': 'dark',
                                    'sessionRestore': 'off'}))
    os.chown(settings, uid, gid)
    trace = run / 'poll.trace'
    trace.touch()
    os.chown(trace, uid, gid)
    env = dict(os.environ, HOME=str(home), XDG_RUNTIME_DIR=str(run / 'runtime'),
               XDG_CONFIG_HOME=str(home / '.config'), XDG_CACHE_HOME=str(home / '.cache'),
               XDG_DATA_HOME=str(home / '.local/share'), XDG_STATE_HOME=str(home / '.local/state'),
               LORD_GPU='0', AXIA_NET_OFFLINE='1')
    # The probe must not connect to an owner's desktop, PipeWire or session bus.
    for key in ('WAYLAND_DISPLAY', 'DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'PIPEWIRE_REMOTE',
                'PULSE_SERVER', 'LORDOS_SESSIOND', 'LORD_HWSCAN_ROOT'):
        env.pop(key, None)

    def demote():
        if os.geteuid() == 0:
            os.setgroups([])
            os.setgid(gid)
            os.setuid(uid)
        os.nice(19)

    command = ['strace', '-qq', '-ttt', '-T', '-e',
               'trace=poll,ppoll,epoll_wait,epoll_pwait', '-o', str(trace),
               str(installation / 'bin/axia-server'), '--headless', '1180x700',
               '--timeout', str(math.ceil(warmup + duration + 15)),
               '--theme', str(installation / 'themes/default/theme.bundle.js'),
               '--fonts', str(installation / 'assets/fonts')]
    proc = None
    child_pid = None
    artifacts.mkdir(parents=True, exist_ok=True)
    try:
        with (run / 'stderr.log').open('w') as stderr:
            proc = subprocess.Popen(command, cwd=installation / 'bin', env=env,
                                    stdout=subprocess.DEVNULL, stderr=stderr,
                                    preexec_fn=demote, start_new_session=True)
            time.sleep(warmup)
            if proc.poll() is not None:
                raise RuntimeError('private session exited during warmup; inspect stderr.log')
            children = Path(f'/proc/{proc.pid}/task/{proc.pid}/children').read_text().split()
            if len(children) != 1:
                raise RuntimeError('cannot identify the private compositor under strace')
            child_pid = int(children[0])
            if Path(f'/proc/{child_pid}/comm').read_text().strip() != 'axia-server':
                raise RuntimeError('strace child is not axia-server')
            start = time.monotonic()
            since = time.time()
            rss = []
            while time.monotonic() - start < duration:
                if proc.poll() is not None:
                    raise RuntimeError('private session exited before collection completed')
                status = Path(f'/proc/{child_pid}/status').read_text()
                rss.append(int(re.search(r'^VmRSS:\s+(\d+)', status, re.M)[1]) * 1024)
                time.sleep(min(1, max(0, duration - (time.monotonic() - start))))
            elapsed = time.monotonic() - start
            until = time.time()
            os.kill(child_pid, signal.SIGTERM)
            proc.wait(timeout=8)
            if proc.returncode:
                raise RuntimeError(f'private session/strace exit status {proc.returncode}')
        log = home / '.cache/axia/session.log'
        text = log.read_text()
        if ('js: theme ' not in text or ' loaded in ' not in text
                or re.search(r'js: (?:exception:|TypeError:|ReferenceError:|SyntaxError:)', text)):
            raise RuntimeError('theme did not load correctly; inspect session.log')
        count = poll_count(trace.read_text(), since, until)
        return {'uid': uid, 'duration_seconds': elapsed, 'poll_calls': count,
                'poll_calls_per_second': count / elapsed,
                'rss_bytes_median': statistics.median(rss), 'rss_bytes_max': max(rss),
                'rss_samples': len(rss), 'command': command, 'exit_code': proc.returncode}
    finally:
        if proc is not None:
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait(timeout=3)
        for source in (trace, run / 'stderr.log', home / '.cache/axia/session.log'):
            if source.is_file():
                shutil.copyfile(source, artifacts / source.name)


def positive(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError('must be a positive finite number')
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--installation', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--warmup', type=positive, default=8)
    parser.add_argument('--duration', type=positive, default=20)
    parser.add_argument('--samples', type=int, default=3)
    parser.add_argument('--max-polls-per-second', type=positive, default=15)
    parser.add_argument('--max-rss-mib', type=positive, default=120)
    parser.add_argument('--report-only', action='store_true', help='collect calibration data without enforcing budgets')
    args = parser.parse_args()
    if args.samples < 1 or args.samples % 2 != 1:
        parser.error('--samples must be a positive odd number')
    report = {'status': 'invalid', 'samples': [], 'scope': 'main thread, software, offline',
              'budgets': {'poll_calls_per_second': args.max_polls_per_second,
                          'rss_bytes_median': args.max_rss_mib * 1024**2},
              'enforced': not args.report_only}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    try:
        source = args.installation.resolve(strict=True)
        for name in ('bin/axia-server', 'themes/default/theme.bundle.js', 'assets/fonts'):
            if not (source / name).exists():
                raise RuntimeError(f'incomplete installation: {name}')
        # A private readable copy avoids chmod on the checkout or runner's installation.
        with tempfile.TemporaryDirectory(prefix='axia-idle-', dir='/var/tmp') as directory:
            workspace = Path(directory)
            workspace.chmod(0o755)
            staged = workspace / 'installation'
            shutil.copytree(source, staged)
            for index in range(args.samples):
                run = workspace / f'sample-{index + 1}'
                run.mkdir(mode=0o755)
                artifacts = args.output.parent / (args.output.stem + '-samples') / run.name
                report['samples'].append(collect(staged, run, artifacts, args.warmup, args.duration))
        report['summary'] = evaluate(report['samples'], args.samples,
                                     args.max_polls_per_second, args.max_rss_mib * 1024**2)
        report['status'] = 'exceeded' if report['summary']['exceeded'] else 'passed'
    except Exception as error:
        report['error'] = f'{type(error).__name__}: {error}'
    finally:
        args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report.get('summary', {'error': report.get('error')})))
    return int(report['status'] == 'invalid' or (report['status'] == 'exceeded' and not args.report_only))


if __name__ == '__main__':
    raise SystemExit(main())
