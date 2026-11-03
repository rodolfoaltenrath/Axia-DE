#!/usr/bin/env python3
"""Build a source RPM with locked frontend bundles and a growing snapshot release."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def snapshot_spec(text, version, stamp, commit):
    match = re.search(r'^Version:[ \t]+(\S+)[ \t]*$', text, re.M)
    if not match or match[1] != version:
        raise ValueError('package.json and the RPM spec must have the same version')
    match = re.search(r'^Release:[ \t]+(\d+)%\{\?dist\}[ \t]*$', text, re.M)
    if not match:
        raise ValueError('expected a numeric RPM Release followed by %{?dist}')
    if not re.fullmatch(r'\d{20}', stamp) or not re.fullmatch(r'[0-9a-f]{12}', commit):
        raise ValueError('invalid snapshot timestamp or commit')
    # Write the release into the spec stored in the SRPM. Command-line RPM macros
    # alone would disappear when Copr rebuilds that SRPM on another machine.
    release = f'{match[1]}.{stamp}.git{commit}'
    return text[:match.start()] + f'Release:        {release}%{{?dist}}' + text[match.end():], release


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--outdir', required=True)
    args = parser.parse_args()
    out = Path(args.outdir).resolve()
    out.mkdir(parents=True, exist_ok=True)
    version = json.loads((ROOT / 'package.json').read_text())['version']
    commit = subprocess.check_output(['git', 'rev-parse', '--short=12', 'HEAD'], cwd=ROOT, text=True).strip()
    stamp = datetime.now(timezone.utc).strftime('%Y%m%d%H%M%S%f')
    spec, release = snapshot_spec((ROOT / 'packaging/rpm/axia-de.spec').read_text(), version, stamp, commit)
    subprocess.run(['sh', 'tools/desktop/dist.sh'], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix='axia-srpm-') as tmp:
        top = Path(tmp)
        (top / 'SPECS').mkdir()
        specfile = top / 'SPECS/axia-de.spec'
        specfile.write_text(spec)
        subprocess.run([
            'rpmbuild', '-bs', '--define', f'_topdir {top}',
            '--define', f'_sourcedir {ROOT / "dist"}',
            '--define', f'_srcrpmdir {out}', str(specfile),
        ], cwd=ROOT, check=True)
    print(f'Source RPM ready: axia-de {version}-{release} ({commit})')


if __name__ == '__main__':
    main()
