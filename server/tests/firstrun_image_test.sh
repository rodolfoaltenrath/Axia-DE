#!/bin/sh
# firstrun_image_test.sh — lordos-firstrun end to end on a real LordOS Raspberry Pi image written to a
# VIRTUAL USB stick (docs/IMAGER.md, "Tests"). Root; the stick is checked to be the USB gadget first.
#
#   sudo tests/firstrun_image_test.sh <sdX> <backing file> <lordos-firstrun.txt>
#
# The stick must already hold the image (imager-write). The script puts the first-run file on its boot
# partition the way the imager does, mounts both partitions under /mnt/firstrun-test and runs
# build/lordos-firstrun apply with --root/--boot (useradd/usermod/systemctl --root: nothing runs on
# this machine), then checks every file it should have written, and that the file is gone.
set -u
D=${1:?sdX}; BACK=${2:?backing file}; FILE=${3:?lordos-firstrun.txt}
BIN=$(cd "$(dirname "$0")/.." && pwd)/build/lordos-firstrun
m=$(sed 's/ *$//' /sys/block/$D/device/model 2>/dev/null); sz=$(( $(cat /sys/block/$D/size) * 512 ))
echo "target /dev/$D: model=$m size=$sz (backing file $BACK, $(stat -c %s "$BACK") bytes)"
[ "$m" = "File-Stor Gadget" ] && [ "$sz" = "$(stat -c %s "$BACK")" ] || { echo "not the virtual stick: stopping"; exit 2; }
M=/mnt/firstrun-test
mkdir -p $M/boot $M/root
mount /dev/${D}1 $M/boot && mount /dev/${D}2 $M/root || { echo "cannot mount the image's partitions"; exit 2; }
trap 'umount $M/boot $M/root 2>/dev/null' EXIT
cp "$FILE" $M/boot/lordos-firstrun.txt
cp $M/boot/config.txt /tmp/config.before; cp $M/boot/cmdline.txt /tmp/cmdline.before
pass=0; fail=0
ok() { pass=$((pass + 1)); echo "  ok   $1"; }
bad() { fail=$((fail + 1)); echo "  FAIL $1"; }
chk() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }

"$BIN" apply $M/boot/lordos-firstrun.txt --root $M/root --boot $M/boot; rc=$?
echo "exit status $rc"
R=$M/root
chk "exit 0 (applied, with a user)" '[ $rc = 0 ]'
chk "the first-run file is gone" '[ ! -e $M/boot/lordos-firstrun.txt ]'
chk "hostname" 'grep -qx pi-living $R/etc/hostname && grep -q "^127.0.1.1	pi-living$" $R/etc/hosts'
chk "time zone" '[ "$(readlink $R/etc/localtime)" = /usr/share/zoneinfo/America/Argentina/Buenos_Aires ] && grep -qx America/Argentina/Buenos_Aires $R/etc/timezone'
chk "language (/etc/default/locale links to /etc/locale.conf)" 'grep -qx LANG=es_ES.UTF-8 $R/etc/default/locale'
chk "keyboard" 'grep -q "XKBLAYOUT=\"latam\"" $R/etc/default/keyboard'
chk "Wi-Fi country in cmdline.txt (one line)" 'grep -q "cfg80211.ieee80211_regdom=AR$" $M/boot/cmdline.txt && [ "$(wc -l < $M/boot/cmdline.txt)" = 1 ]'
chk "cmdline.txt keeps everything else" '[ "$(sed "s/ cfg80211.ieee80211_regdom=AR//" $M/boot/cmdline.txt)" = "$(cat /tmp/cmdline.before)" ]'
W=$R/etc/lordos/net/wifi/43617361204c6f726420e29da4
chk "Wi-Fi network saved (hidden, WPA2)" 'grep -q "\"security\":\"psk\",\"hidden\":true" $W.json'
chk "Wi-Fi key is the derived PSK, 0600" 'grep -qx "psk=2909c9807b542655513df419543d91e3c831ad3bff2e95ff6d2eb80cc3209424" $W.key && [ "$(stat -c %a $W.key)" = 600 ]'
chk "no Wi-Fi password anywhere" '! grep -rq "hunter2-wifi" $R/etc $M/boot 2>/dev/null'
chk "user ana created with the hash" 'grep -qF "ana:\$6\$imagertestsalt1\$" $R/etc/shadow && grep -q "^ana:x:[0-9]*:[0-9]*:Ana Pérez:/home/ana:/bin/bash$" $R/etc/passwd'
chk "ana is an administrator (sudo) and in audio/video/render" 'for g in sudo audio video render input; do grep -q "^$g:.*[:,]ana\(,\|$\)" $R/etc/group || exit 1; done'
chk "home folders in Spanish" '[ -d $R/home/ana/Escritorio ] && [ -d $R/home/ana/Descargas ]'
chk "settings.json: language, keyboard, Classic, light, violeta, wallpaper, updates off" 'grep -q "\"theme\": \"classic\"" $R/home/ana/.config/lordos/settings.json && grep -q "\"accent\": \"violeta\"" $R/home/ana/.config/lordos/settings.json && grep -q "\"autoCheck\": false" $R/home/ana/.config/lordos/settings.json && grep -q "\"layout\": \"latam\"" $R/home/ana/.config/lordos/settings.json'
U=$(grep "^ana:" $R/etc/passwd | cut -d: -f3)
chk "her files belong to her" '[ "$(stat -c %u $R/home/ana/.config/lordos/settings.json)" = "$U" ] && [ "$(stat -c %u $R/home/ana/.ssh/authorized_keys)" = "$U" ]'
chk "authorized_keys 0600 in a 0700 .ssh" '[ "$(stat -c %a $R/home/ana/.ssh)" = 700 ] && [ "$(stat -c %a $R/home/ana/.ssh/authorized_keys)" = 600 ] && grep -q "^ssh-ed25519 " $R/home/ana/.ssh/authorized_keys'
chk "linger (PipeWire before login)" '[ -e $R/var/lib/systemd/linger/ana ]'
chk "automatic login" 'grep -qx AUTOLOGIN=ana $R/etc/lordos/session.conf'
chk "SSH enabled, keys only" '[ -L $R/etc/systemd/system/multi-user.target.wants/ssh.service ] && grep -qx "PasswordAuthentication no" $R/etc/ssh/sshd_config.d/05-lordos-keys-only.conf'
chk "remote desktop on, port 5673, local network only" 'grep -qx enabled=1 $R/etc/lordos/remote/config && grep -qx port=5673 $R/etc/lordos/remote/config && grep -qx net=lan $R/etc/lordos/remote/config'
chk "fan profile performance (fan.json root-owned 0644)" 'grep -q "\"profile\":\"performance\"" $R/etc/lordos/fan.json && [ "$(stat -c %u%a $R/etc/lordos/fan.json)" = 0644 ]'
chk "performance governor at boot" 'grep -q "scaling_governor - - - - performance" $R/etc/tmpfiles.d/lordos-cpu-governor.conf'
chk "mild overclock block in config.txt, the rest untouched" 'grep -qx arm_freq=2600 $M/boot/config.txt && [ "$(sed "/^# >>> lordos-firstrun/,/^# <<< lordos-firstrun/d" $M/boot/config.txt)" = "$(cat /tmp/config.before)" ]'
# a second run with a broken file changes nothing and still deletes it
printf 'version=1\nhostname=OTHER\n' > $M/boot/lordos-firstrun.txt
"$BIN" apply $M/boot/lordos-firstrun.txt --root $M/root --boot $M/boot > /tmp/fr2.log; rc=$?
chk "a broken file: exit 1, nothing applied, file deleted" '[ $rc = 1 ] && grep -qx pi-living $R/etc/hostname && [ ! -e $M/boot/lordos-firstrun.txt ]'
echo "firstrun_image_test: $pass passed, $fail failed"
[ "$fail" = 0 ]
