#!/bin/sh
# run-pw-116.sh — our own Secret Service against the real bus and libsecret (secret-tool).
# Usage (on the VM, from server/, with build/secretsvc-demo already built):
#   sh tests/secretsvc/run-pw-116.sh [--capture tests/secretsvc/libsecret-capture.bin]
# Needs libsecret-tools (secret-tool) and dbus-bin (dbus-send, dbus-monitor).
# Leaves the bus as it was: no demo keeps the name org.freedesktop.secrets.
set -u
cd "$(dirname "$0")/../.." || exit 1
: "${XDG_RUNTIME_DIR:=/run/user/$(id -u)}"
export XDG_RUNTIME_DIR
: "${DBUS_SESSION_BUS_ADDRESS:=unix:path=$XDG_RUNTIME_DIR/bus}"
export DBUS_SESSION_BUS_ADDRESS
CAP=""
[ "${1:-}" = "--capture" ] && CAP="$2"
LOG=$(mktemp /tmp/ss-demo-XXXXXX.log)
ok=0; bad=0
pass() { ok=$((ok+1)); echo "ok   $1"; }
fail() { bad=$((bad+1)); echo "FAIL $1"; }
owner() { dbus-send --session --print-reply --dest=org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q "boolean true"; }

if owner; then echo "org.freedesktop.secrets already has an owner: not testing"; exit 2; fi
pgrep -f "[g]nome-keyring-daemon" >/dev/null && echo "warning: gnome-keyring-daemon is running"
[ -x build/secretsvc-demo ] || { echo "build/secretsvc-demo is missing"; exit 2; }

start() { build/secretsvc-demo "$@" --seconds 60 >>"$LOG" 2>&1 & DEMO=$!; i=0; while ! owner && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done; }
stop() { kill "$DEMO" 2>/dev/null; wait "$DEMO" 2>/dev/null; }
MON=""
if [ -n "$CAP" ]; then dbus-monitor --session --binary "type='method_call',destination='org.freedesktop.secrets'" >"$CAP" 2>/dev/null & MON=$!; sleep 0.3; fi

# --- unlocked ---
start
owner && pass "the demo owns org.freedesktop.secrets" || fail "the demo did not take the name"
build/secretsvc-demo --seconds 2 >>"$LOG" 2>&1; [ $? -eq 3 ] && pass "a second service sees the name taken (ss_start = 1)" || fail "second service"
printf 'contraseña ñandú 123' | secret-tool store --label="Prueba LordOS" service lordos-demo user ana && pass "secret-tool store" || fail "secret-tool store"
v=$(secret-tool lookup service lordos-demo user ana); [ "$v" = "contraseña ñandú 123" ] && pass "secret-tool lookup" || fail "secret-tool lookup ($v)"
printf 'otra' | secret-tool store --label="Segunda" service lordos-demo user beto
secret-tool search --all service lordos-demo >/tmp/ss-search.$$ 2>&1
grep -q "label = Prueba LordOS" /tmp/ss-search.$$ && grep -q "label = Segunda" /tmp/ss-search.$$ && pass "secret-tool search --all (2 items)" || { fail "secret-tool search"; cat /tmp/ss-search.$$; }
grep -q "secret = contraseña ñandú 123" /tmp/ss-search.$$ && pass "search returns the secrets" || fail "search without secrets"
printf 'cambiada' | secret-tool store --label="Prueba LordOS" service lordos-demo user ana
v=$(secret-tool lookup service lordos-demo user ana); [ "$v" = "cambiada" ] && pass "storing again replaces" || fail "replace ($v)"
secret-tool clear service lordos-demo user ana; v=$(secret-tool lookup service lordos-demo user ana); [ -z "$v" ] && pass "secret-tool clear" || fail "clear ($v)"
secret-tool lock --collection=login && pass "secret-tool lock" || fail "secret-tool lock"
v=$(timeout 15 secret-tool lookup service lordos-demo user beto); [ "$v" = "otra" ] && pass "locked lookup: Unlock with a prompt, then the secret" || fail "lookup after lock ($v)"
stop

# --- locked: the prompt accepts itself after one second ---
start --locked
printf 'con aviso' | timeout 15 secret-tool store --label="Bloqueado" service lordos-demo user carla && pass "store with the keyring locked (unlock prompt)" || fail "locked store"
v=$(timeout 15 secret-tool lookup service lordos-demo user carla); [ "$v" = "con aviso" ] && pass "lookup after the prompt" || fail "locked lookup ($v)"
stop
[ -n "$MON" ] && { kill "$MON" 2>/dev/null; wait "$MON" 2>/dev/null; }

grep -q "OpenSession dh-ietf1024-sha256-aes128-cbc-pkcs7" "$LOG" && pass "libsecret negotiated dh-ietf1024-sha256-aes128-cbc-pkcs7" || echo "note: libsecret did not use DH"
grep -q "OpenSession plain" "$LOG" && echo "note: there were plain sessions too"
grep -q "Unlock → prompt" "$LOG" && pass "Unlock went through a Prompt" || echo "note: there was no Prompt"   # matches the "Unlock → prompt" debug line of src/vault/secretsvc.c
owner && fail "someone still owns the name" || pass "the name is free"
echo "log: $LOG"
rm -f /tmp/ss-search.$$
echo "secretsvc with secret-tool: $ok passed, $bad failed"
[ $bad -eq 0 ]
