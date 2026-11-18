#!/usr/bin/env python3
"""Run a build and report sampled process-tree RSS (Linux, no extra packages)."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def processes():
    result = {}
    for directory in Path('/proc').iterdir():
        if not directory.name.isdigit():
            continue
        try:
            stat = (directory / 'stat').read_text()
            end = stat.rindex(')')
            fields = stat[end + 2:].split()
            result[int(directory.name)] = (
                int(fields[1]), int(fields[21]) * os.sysconf('SC_PAGE_SIZE'),
                stat[stat.index('(') + 1:end],
            )
        except (OSError, ValueError, IndexError):
            pass  # A process may exit while /proc is being read.
    return result


def sample(root, table):
    descendants = {root}
    pending = set(table) - descendants
    while True:
        children = {pid for pid in pending if table[pid][0] in descendants}
        if not children:
            break
        descendants.update(children)
        pending.difference_update(children)
    tree_rss = sum(table[pid][1] for pid in descendants if pid in table)
    rustc_rss = max((table[pid][1] for pid in descendants
                     if pid in table and table[pid][2] == 'rustc'), default=0)
    return tree_rss, rustc_rss


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('a build command is required after --')
    start = time.monotonic()
    peak_tree = peak_rustc = 0
    child = subprocess.Popen(command)
    try:
        while True:
            tree, rustc = sample(child.pid, processes())
            peak_tree, peak_rustc = max(peak_tree, tree), max(peak_rustc, rustc)
            if child.poll() is not None:
                break
            time.sleep(0.2)
    except BaseException:
        child.terminate()
        child.wait()
        raise
    finally:
        report = {
            'command': command, 'exit_code': child.returncode,
            'elapsed_seconds': round(time.monotonic() - start, 2),
            'sample_interval_seconds': 0.2,
            'peak_process_tree_rss_bytes': peak_tree,
            'peak_single_rustc_rss_bytes': peak_rustc,
            'note': 'Sampled RSS; shared pages may be counted in multiple processes. '
                    'This is not total runner memory or an exact high-water mark.',
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2), flush=True)
        if peak_tree > 11 * 1024**3:
            print('::warning::Build process RSS exceeded 11 GiB; review memory growth.', flush=True)
        if peak_rustc > 5 * 1024**3:
            print('::warning::A rustc process exceeded 5 GiB; consider another crate split.', flush=True)
    return child.returncode if child.returncode >= 0 else 128 - child.returncode


if __name__ == '__main__':
    sys.exit(main())
