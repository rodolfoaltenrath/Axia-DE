#!/bin/sh
# imager_write_test.sh — the imager-write / imager-fetch actions against a VIRTUAL USB stick (docs/IMAGER.md,
# "Tests"). Never run it against a real disk: it erases the stick. Needs root, build/disk-cli and a
# stick made from a file with the kernel's USB gadget (it is removable and on USB, like a real one):
#
#   truncate -s 4G /var/tmp/stick.img
#   modprobe dummy_hcd; modprobe g_mass_storage file=/var/tmp/stick.img removable=1
#   sudo tests/imager_write_test.sh <user> <sdX> /var/tmp/stick.img <signed release dir in the user's home> [internal sdY]
#
# The release dir holds SHA256SUMS, SHA256SUMS.sig and a small official .img.xz they list (any real
# LordOS release works). The script checks first that <sdX> is that gadget and that its size is the
# backing file's, and stops otherwise.
set -u
U=${1:?user}; D=${2:?sdX}; BACK=${3:?backing file}; REL=${4:?release dir}
CLI=$(dirname "$0")/../build/disk-cli
H=$(getent passwd "$U" | cut -d: -f6)
W=$H/.cache/imager-write-test
pass=0; fail=0
ok() { pass=$((pass + 1)); echo "  ok   $1"; }
bad() { fail=$((fail + 1)); echo "  FAIL $1"; }

# ---- the target must be the virtual stick ----
m=$(sed 's/ *$//' /sys/block/$D/device/model 2>/dev/null); v=$(sed 's/ *$//' /sys/block/$D/device/vendor 2>/dev/null)
sz=$(( $(cat /sys/block/$D/size) * 512 )); want=$(stat -c %s "$BACK")
echo "target /dev/$D: vendor=$v model=$m size=$sz (backing file $BACK, $want bytes)"
[ "$v" = Linux ] && [ "$m" = "File-Stor Gadget" ] && [ "$sz" = "$want" ] || { echo "not the virtual stick: stopping"; exit 2; }

IMG=$(ls "$REL"/*.img.xz | head -1); SUMS=$REL/SHA256SUMS; SIG=$REL/SHA256SUMS.sig
run() { "$CLI" -u "$U" -n imager-write "$1" 2>&1 | grep -v ' progress ' | tail -1; }
id="$D\n$sz\n$m\n\n"   # disk, size, model, serial (empty for the gadget)
expect() {  # expect <code> <description> <arg>
    out=$(run "$3")
    case "$out" in *"$1"*) ok "$2 → $1" ;; *) bad "$2: wanted $1, got: $out" ;; esac
}

install -d -o "$U" -g "$U" "$W"
cp "$SUMS" "$W/SHA256SUMS"; cp "$SIG" "$W/SHA256SUMS.sig"
sed '1s/^./0/' "$SUMS" > "$W/SHA256SUMS.tampered"
printf 'NzFKWqpHNft14vKPCOK6q0DaQNef9jic0/7TUa7wkocU0g2AnRb7Gg2MBWljOsKwUK/0ceCAlkuBpRmjtSqPAw==\n' > "$W/wrong.sig"   # 0.8.0's, a real release signature over another list
head -c 1048576 /dev/urandom > "$W/custom.img"
truncate -s $((sz + 1048576)) "$W/huge.img"
printf 'not xz at all' > "$W/broken.img.xz"
ln -sf /etc/shadow "$W/link.img"
chown -R "$U:$U" "$W"

echo "refusals (the stick must not be touched):"
INT=${5:-}   # optional: an internal, non-system scratch disk to show the removable-only rule
if [ -n "$INT" ]; then
    out=$("$CLI" -u "$U" imager-write "$INT\n1\nx\n\ncustom\n$W/custom.img" 2>&1 | tail -1)
    case "$out" in *imager.notRemovable*) ok "an internal disk ($INT), even for an administrator → imager.notRemovable" ;; *) bad "internal disk: $out" ;; esac
fi
"$CLI" -u "$U" imager-write "sda\n1\nx\n\ncustom\n$W/custom.img" 2>&1 | tail -1 | grep -q 'disk.systemDisk\|imager.notRemovable' && ok "the system disk as an administrator" || bad "the system disk as an administrator"
expect imager.diskChanged "a different size" "$D\n$((sz - 512))\n$m\n\ncustom\n$W/custom.img"
expect imager.diskChanged "a different model" "$D\n$sz\nOther Stick\n\ncustom\n$W/custom.img"
expect imager.diskChanged "a different serial" "$D\n$sz\n$m\nXYZ\ncustom\n$W/custom.img"
expect imager.badSignature "a tampered SHA256SUMS" "${id}official\n$IMG\n$W/SHA256SUMS.tampered\n$W/SHA256SUMS.sig"
expect imager.badSignature "a signature from another release" "${id}official\n$IMG\n$W/SHA256SUMS\n$W/wrong.sig"
expect imager.noSignature "a missing signature" "${id}official\n$IMG\n$W/SHA256SUMS\n$W/nothing.sig"
expect imager.notListed "an image that SHA256SUMS does not list" "${id}official\n$W/custom.img\n$W/SHA256SUMS\n$W/SHA256SUMS.sig"
expect disk.imageTooLarge "an image larger than the stick" "${id}custom\n$W/huge.img"
expect imager.xzDamaged "a broken .img.xz" "${id}custom\n$W/broken.img.xz"
expect disk.imageOpenFailed "a link to /etc/shadow" "${id}custom\n$W/link.img"
expect disk.imageOpenFailed "a file outside the home folder" "${id}custom\n/etc/shadow.img"
expect imager.badImageName "a file that is not an image" "${id}custom\n/etc/shadow"
expect admin.invalidArgument "official without the signature lines" "${id}official\n$IMG"
expect admin.invalidArgument "a control character" "$D\n$sz\n$m$(printf '\001')\n\ncustom\n$W/custom.img"

echo "writing:"
out=$(run "${id}custom\n$W/custom.img")
h=$(sha256sum < "$W/custom.img" | cut -d' ' -f1)
case "$out" in *"imager.written sha256=$h"*) ok "a custom 1 MiB .img, verified ($h)" ;; *) bad "custom .img: $out" ;; esac
[ "$(head -c 1048576 /dev/$D | sha256sum | cut -d' ' -f1)" = "$h" ] && ok "read back independently" || bad "read back independently"

out=$(run "${id}official\n$IMG\n$W/SHA256SUMS\n$W/SHA256SUMS.sig")
h=$(xz -dc "$IMG" | sha256sum | cut -d' ' -f1)
case "$out" in *"imager.written sha256=$h"*official=1*) ok "the official $(basename "$IMG"), signature and hash checked, verified" ;; *) bad "official: $out" ;; esac
n=$(xz --robot -l "$IMG" | awk '/^totals/ {print $5}')
[ "$(head -c "$n" /dev/$D | sha256sum | cut -d' ' -f1)" = "$h" ] && ok "read back independently ($n bytes)" || bad "read back independently"

udevadm settle
expect disk.isPartition "a partition (the image's first one)" "${D}1\n$sz\n$m\n\ncustom\n$W/custom.img"
echo "cancelling in the middle:"
( sleep 6; "$CLI" -u "$U" -n disk-cancel "$D" > /dev/null ) &
out=$(run "${id}official\n$IMG\n$W/SHA256SUMS\n$W/SHA256SUMS.sig")
wait
case "$out" in *imager.cancelledWiped*) ok "cancelled → imager.cancelledWiped" ;; *) bad "cancel: $out" ;; esac
[ "$(head -c 1048576 /dev/$D | tr -d '\0' | wc -c)" = 0 ] && ok "the first MiB is wiped (no half-written partition table)" || bad "first MiB not wiped"
"$CLI" -u nobody -n disk-cancel "$D" 2>&1 | grep -q 'disk.nothingRunning\|disk.otherUser' && ok "another user cannot cancel" || bad "another user cancel"

rm -rf "$W"
echo "imager_write_test: $pass passed, $fail failed"
[ "$fail" = 0 ]
