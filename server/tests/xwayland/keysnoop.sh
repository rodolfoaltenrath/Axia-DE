#!/bin/sh
# keysnoop.sh SECONDS — what an X11 keylogger sees: polls the X keyboard's key state (the classic
# XQueryKeymap trick, through `xinput query-state`) and prints how many samples had a key down.
# Run it while typing into a Wayland window (Passwords): it must print 0. Run it while typing into
# an X11 window: it sees the keys (X11 apps share one display). Needs xinput. docs/PRIVACY.md.
secs=${1:-8}
dev=$(xinput list --id-only "xwayland-keyboard:6" 2>/dev/null || xinput list --short | sed -n 's/.*xwayland-keyboard[^=]*id=\([0-9]*\).*/\1/p' | head -1)
[ -n "$dev" ] || { echo "no Xwayland keyboard device"; exit 2; }
end=$(( $(date +%s) + secs )); down=0; samples=0
while [ "$(date +%s)" -lt "$end" ]; do
    samples=$((samples + 1))
    xinput query-state "$dev" 2>/dev/null | grep -q '=down' && down=$((down + 1))
done
echo "samples=$samples with-a-key-down=$down"
