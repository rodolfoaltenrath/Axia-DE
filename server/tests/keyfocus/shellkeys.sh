#!/bin/sh
# shellkeys.sh SECONDS — does a Linux app still get the keys while LordOS's search, a sheet or a
# system prompt is open? (0.8.2 security fix: runtime/index.js counts what is open in the overlay,
# modal and system layers and sdk/wayland.js then gives the Wayland keyboard focus to nobody.)
#
# Runs tools/diag/keylag-client (a Wayland window that prints one line per wl_keyboard.key press,
# never the key itself) for SECONDS and prints how many key presses it received. Use it inside a
# LordOS session, with the client's window in front:
#   1. type while the window has the keyboard           → keys > 0
#   2. open the search (or a keyring/permission prompt), type into it → keys = 0
#   3. close it and type again                          → keys > 0
# The keys can be typed by hand or sent from the host (qm sendkey / QMP). docs/PRIVACY.md.
secs=${1:-10}
client=${KEYLAG_CLIENT:-$(dirname "$0")/../../../tools/diag/keylag-client}
[ -x "$client" ] || { echo "build tools/diag/keylag-client first (see its header) or set KEYLAG_CLIENT" >&2; exit 2; }
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0}
out=$(mktemp)
timeout "$secs" "$client" > "$out" 2>/dev/null
echo "keys=$(grep -c 'client key' "$out")"
rm -f "$out"
