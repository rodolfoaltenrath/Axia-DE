#!/usr/bin/env python3
"""Submit one checked SRPM, then tolerate temporary failures of read-only polling."""
import argparse
import configparser
import json
import os
from pathlib import Path
import re
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import urlopen

COPR_URL = 'https://copr.fedorainfracloud.org'
FAILED = {'failed', 'canceled', 'cancelled'}


def read_state(base_url, build_id, timeout):
    with urlopen(f'{base_url}/api_3/build/{build_id}', timeout=timeout) as response:
        return json.load(response)['state']


def watch(build_id, base_url=COPR_URL, *, fetch=read_state, sleep=time.sleep,
          now=time.monotonic, timeout=7200, max_errors=8):
    deadline = now() + timeout
    errors = 0
    while now() < deadline:
        try:
            state = fetch(base_url, build_id, min(30, deadline - now()))
        except (HTTPError, URLError, TimeoutError, OSError, ValueError, KeyError) as error:
            if isinstance(error, HTTPError):
                error.close()
                if error.code not in {408, 429} and error.code < 500:
                    raise RuntimeError(f'Copr status request failed: HTTP {error.code}') from None
            errors += 1
            if errors >= max_errors:
                raise RuntimeError(f'Copr status unavailable after {errors} consecutive errors; '
                                   f'resume with --build-id {build_id}') from None
            delay = min(15 * 2 ** (errors - 1), 120, max(0, deadline - now()))
            print(f'Copr build {build_id}: status temporarily unavailable; retrying in {delay:g}s', flush=True)
        else:
            errors = 0
            print(f'Copr build {build_id}: {state}', flush=True)
            if state in {'succeeded', 'skipped'}:
                return
            if state in FAILED:
                raise RuntimeError(f'Copr build {build_id} {state}')
            if not isinstance(state, str) or not state or state == 'unknown':
                raise RuntimeError(f'Copr build {build_id}: unexpected state {state!r}')
            delay = min(30, max(0, deadline - now()))
        sleep(delay)
    raise RuntimeError(f'Timed out waiting for Copr build {build_id}; resume with --build-id {build_id}')


def record_build(build_id, base_url, report):
    url = f'{base_url}/coprs/build/{build_id}'
    print(f'Copr build: {url}', flush=True)
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps({'build_id': build_id, 'url': url}, indent=2) + '\n')
    if summary := os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(summary, 'a', encoding='utf-8') as output:
            output.write(f'\nCopr build: [{build_id}]({url})\n')


def publish(client, project, source, report, *, wait=watch):
    owner, name = project.split('/')
    # Never retry a POST: an interrupted response may hide an accepted build.
    client.config['connection_attempts'] = 1
    build = client.build_proxy.create_from_file(owner, name, str(source))
    build_id = int(build.id)
    base_url = client.config['copr_url'].rstrip('/')
    record_build(build_id, base_url, report)
    wait(build_id, base_url)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--packages', type=Path, default=Path('packages'))
    parser.add_argument('--report', type=Path, default=Path('build/copr/build.json'))
    parser.add_argument('--build-id', type=int, help='only watch an existing build; never upload')
    args = parser.parse_args()
    if args.build_id is not None:
        if args.build_id <= 0:
            parser.error('--build-id must be positive')
        record_build(args.build_id, COPR_URL, args.report)
        watch(args.build_id)
        return
    project = os.environ.get('COPR_PROJECT', '')
    if not re.fullmatch(r'@?[A-Za-z0-9_-]+/[A-Za-z0-9_.-]+', project):
        raise RuntimeError('COPR_PROJECT must be OWNER/PROJECT')
    config = os.environ.pop('COPR_CONFIG', '')
    parsed = configparser.ConfigParser(interpolation=None)
    try:
        parsed.read_string(config)
    except configparser.Error:
        raise RuntimeError('COPR_CONFIG must be a valid INI file') from None
    if not parsed.has_section('copr-cli') or any(not parsed['copr-cli'].get(key)
                                                for key in ['login', 'username', 'token']):
        raise RuntimeError('COPR_CONFIG is missing Copr credentials')
    sources = list(args.packages.rglob('*.src.rpm'))
    if len(sources) != 1:
        raise RuntimeError('Expected exactly one checked source RPM')
    from copr.v3 import Client
    with tempfile.TemporaryDirectory() as temporary:
        path = Path(temporary) / 'copr'
        path.touch(mode=0o600)
        path.write_text(config)
        client = Client.create_from_config_file(str(path))
        publish(client, project, sources[0], args.report)


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # API errors can contain server-supplied text; do not print credentials.
        message = str(error) if isinstance(error, RuntimeError) else type(error).__name__
        raise SystemExit(f'Copr publication failed: {message}') from None
