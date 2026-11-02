#!/bin/bash
# notify.sh — "no notifications" for Linux apps: the notification server (src/hw/notifyd.c) asks
# the bus for the caller's PID, maps it to its app scope and drops the restricted app's
# notifications, still answering with a valid id. Runs the real server (build/notifyd-cli) on a
# private session bus and notify-send inside an app scope. Applies to the next notification.
#
#   bash server/tests/fwlinux/notify.sh      (needs dbus-daemon and notify-send: libnotify-bin)
. "$(dirname "$0")/lib.sh"
D=$(cd "$(dirname "$0")" && pwd)
CLI=$D/../../build/notifyd-cli
[ -x "$CLI" ] || { echo "missing build/notifyd-cli (make -C server test)"; exit 2; }
command -v notify-send >/dev/null || { echo "skipped: needs notify-send (libnotify-bin)"; exit 0; }
APP=desktop:fwnotify
SC=$(scope fwnotify)
BUS=$(mktemp -d)
dbus-daemon --session --address="unix:path=$BUS/bus" --fork --print-pid > "$BUS/pid"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$BUS/bus"
OUT=$BUS/out
server() { "$CLI" "$1" > "$OUT" 2>&1 & for i in $(seq 1 30); do grep -q ready "$OUT" 2>/dev/null && break; sleep 0.1; done; }
send() {   # send <title>: notify-send in the app's scope; prints its exit status and the id it got
    sudo env DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" HOME="$HOME" bash -c \
        'echo $$ > "$0/cgroup.procs"; exec setpriv --reuid='"$U"' --regid='"$G"' --init-groups notify-send -p -a fwnotify "$1" body' "$SC" "$1" 2>&1
    echo "status=$?"
}
shown() { grep -c "\"title\":\"$1\"" "$OUT"; }

echo "1. not restricted: shown"
restrict $APP ""
server 6
r=$(send "visible-1")
[ "$(shown visible-1)" = 1 ] && ok "the notification was shown" || bad "not shown ($(cat "$OUT"))"
echo "$r" | grep -q "status=0" && ok "notify-send succeeded ($(echo $r | tr '\n' ' '))" || bad "notify-send failed: $r"
wait

echo "2. no-notifications: dropped, but the app gets a valid id"
restrict $APP no-notifications
server 6
r=$(send "hidden-1")
[ "$(shown hidden-1)" = 0 ] && ok "the notification was not shown" || bad "it was shown"
echo "$r" | grep -q "status=0" && ok "notify-send still succeeded" || bad "notify-send failed: $r"
id=$(echo "$r" | head -1)
[ "${id:-0}" -gt 0 ] 2>/dev/null && ok "it got a notification id ($id)" || bad "no id: $r"
other=$(notify-send -p -a outside "outside-1" body; echo "status=$?")
[ "$(shown outside-1)" = 1 ] && ok "a process outside the app's scope still notifies" || bad "outside process not shown"
wait

echo "3. lifted"
restrict $APP ""
server 5
send "visible-2" >/dev/null
[ "$(shown visible-2)" = 1 ] && ok "shown again" || bad "not shown"
wait
kill "$(cat "$BUS/pid")" 2>/dev/null
rm -rf "$BUS"
fwlinux_done
