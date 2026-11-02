#!/bin/sh
# selections.sh — clipboard/primary checks for X11 apps, run inside a LordOS session on a test VM
# (DISPLAY and WAYLAND_DISPLAY of the session; needs xclip, wl-clipboard, xterm and
# build/clip-offers). See docs/WAYLAND.md, "X11 apps (Xwayland)", and docs/PRIVACY.md.
#
#   sh tests/xwayland/selections.sh            (from server/)
#
# 1. An X11 app's copy (CLIPBOARD and PRIMARY) reaches Wayland apps.
# 2. Offers are bound to their selection: a Wayland app can't read later copies through an old offer.
# 3. A Wayland copy is not readable by an X11 app while no X window has the keyboard; while one has
#    it, a request from another client's window is refused (a hint only: X11 apps share a display).
cd "$(dirname "$0")/../.." || exit 1
fails=0
ok() { printf '  ok      %s\n' "$1"; }
bad() { printf '  FAILED  %s\n' "$1"; fails=$((fails + 1)); }
xset_sel() { setsid -f sh -c "printf %s '$2' | xclip -selection $1" >/dev/null 2>&1 </dev/null; sleep 1; }

# 1
xset_sel clipboard "x11 clipboard 1"
xset_sel primary "x11 primary 1"
[ "$(timeout 3 wl-paste -n 2>/dev/null)" = "x11 clipboard 1" ] && ok "X11 CLIPBOARD → Wayland" || bad "X11 CLIPBOARD → Wayland"
[ "$(timeout 3 wl-paste -n --primary 2>/dev/null)" = "x11 primary 1" ] && ok "X11 PRIMARY → Wayland" || bad "X11 PRIMARY → Wayland"

# 2
out=$(mktemp)
build/clip-offers "x11 clipboard 2" "x11 primary 2" >"$out" 2>&1 &
pid=$!
i=0; while ! grep -q READY "$out" && [ $i -lt 100 ]; do sleep 0.1; i=$((i + 1)); done
xset_sel clipboard "x11 clipboard 2"
xset_sel primary "x11 primary 2"
wait $pid && ok "offers bound to their selection" || { bad "offers bound to their selection"; cat "$out"; }
rm -f "$out"

# 3
setsid -f sh -c "printf %s 'wayland secret-ish' | wl-copy" >/dev/null 2>&1 </dev/null
sleep 1.5
got=$(timeout 3 xclip -o -selection clipboard 2>/dev/null)
[ "$got" != "wayland secret-ish" ] && ok "background X11 app can't read a Wayland copy" || bad "background X11 app read a Wayland copy"
setsid -f xterm -title selections-test >/dev/null 2>&1 </dev/null
sleep 3
got=$(timeout 3 xclip -o -selection clipboard 2>/dev/null)
# (best effort, not a boundary: X11 doesn't authenticate the requestor; see docs/PRIVACY.md)
[ "$got" != "wayland secret-ish" ] && ok "a paste request from another client's window is refused (best effort)" || bad "another client's window got it"
pkill -f "xterm -title selections-test"

[ $fails -eq 0 ] && echo "selections: PASSED" || echo "selections: $fails FAILED"
exit $fails
