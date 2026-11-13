#!/bin/sh
# Cost of mouse moves over the Dock (server/tests/dock_bench.jsx), in a private headless session.
# Run from the repository root inside a build tree (npm run build and make -C server first).
set -e
STAGE=${TMPDIR:-/tmp}/axia-dock-bench
rm -rf "$STAGE" && make install PREFIX=/usr DESTDIR="$STAGE" >/dev/null
B=$STAGE/usr/libexec/axia
cp tests/dock-bench.bundle.js "$B/themes/default/theme.bundle.js"
python3 - "$B" <<'PY'
import hashlib, json, sys
b = sys.argv[1]; p = b + '/code-manifest.json'; m = json.load(open(p))
d = open(b + '/themes/default/theme.bundle.js', 'rb').read()
for e in m['files']:
    if e['path'] == 'themes/default/theme.bundle.js':
        e['size'] = len(d); e['sha256'] = hashlib.sha256(d).hexdigest()
json.dump(m, open(p, 'w'))
PY
HOME_DIR=$STAGE/home; mkdir -p "$HOME_DIR" "$STAGE/run"; chmod 700 "$STAGE/run"
(cd "$B/bin" && HOME=$HOME_DIR XDG_RUNTIME_DIR=$STAGE/run ./axia-server --headless 1920x1080 --timeout 20 \
  --theme ../themes/default/theme.bundle.js >/dev/null 2>&1 || true)
grep "dock bench" "$HOME_DIR/.cache/axia/session.log"
