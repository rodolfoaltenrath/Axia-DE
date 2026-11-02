#!/bin/sh
# run-pw-116.sh — prueba de punta a punta del cliente FIDO2 contra el autenticador por software
# (uhid). Desde server/, con build/softfido y build/fido-e2e ya compilados. Necesita sudo.
set -u
cd "$(dirname "$0")/../../server" || exit 1
sudo modprobe uhid 2>/dev/null
sudo ./build/softfido "$@" &
PID=$!
trap 'sudo kill $PID 2>/dev/null; sudo pkill -x softfido 2>/dev/null' EXIT
# esperar el hidraw de "LordOS Soft FIDO2"
for i in $(seq 1 50); do
  if grep -ls "LordOS Soft FIDO2" /sys/class/hidraw/hidraw*/device/uevent >/dev/null 2>&1; then break; fi
  sleep 0.1
done
sleep 0.3
for d in /sys/class/hidraw/hidraw*; do
  if grep -qs "LordOS Soft FIDO2" "$d/device/uevent"; then
    echo "dispositivo: /dev/$(basename "$d")"
    udevadm info -q property "/dev/$(basename "$d")" 2>/dev/null | grep -E 'ID_FIDO_TOKEN|ID_SECURITY_TOKEN' || echo "(udev no le puso ID_FIDO_TOKEN)"
  fi
done
sudo ./build/fido-e2e
RC=$?
exit $RC
