#!/bin/sh
# build.sh — EXPERIMENTAL immutable (A/B) LordOS disk image: the prototype of docs/IMMUTABLE.md.
#
# It does not touch the regular ISO (tools/iso) or Raspberry Pi (tools/rpi) builds. What it makes:
# a GPT disk image for UEFI PCs (virtual machines) with
#   ESP (systemd-boot + one Unified Kernel Image per installed version, with boot counting)
#   usr slot A + its dm-verity hash partition     (read-only EROFS, the running version)
#   usr slot B + its dm-verity hash partition     (empty until the first update)
#   root (ext4): /etc, /var, /home — the only writable part, kept across updates
# and the update artifacts (usr image, verity hash, UKI, signed SHA256SUMS) that
# /usr/lib/lordos/immutable/update installs into the other slot.
#
# The payload is a minimal Debian 13 with systemd, NOT the LordOS desktop yet (see docs/IMMUTABLE.md,
# "The prototype"). Updates are signed with a throwaway test key made in the work folder, never with
# a LordOS release key.
#
# Usage (as root, on Debian 13 amd64, from the repository root):
#   sudo tools/immutable/build.sh base    [work]            # debootstrap + kernel + initrd (cached)
#   sudo tools/immutable/build.sh release [work] <version> [bad]  # usr image, verity, UKI for <version>
#   sudo tools/immutable/build.sh disk    [work] <version>  # disk.raw with <version> in slot A
#   sudo tools/immutable/build.sh publish [work] <v>...     # signed SHA256SUMS over those releases
#   sudo tools/immutable/build.sh all     [work]            # base, releases 1, 2 and 3 (bad), disk 1, publish 1 2
# Default work folder: /var/tmp/lordos-immutable. Versions are plain integers here (1, 2, 3).
# Packages it needs: see docs/IMMUTABLE.md, "Building the prototype".
set -eu

[ "$(id -u)" = 0 ] || { echo "build.sh: run as root (sudo)" >&2; exit 1; }
HERE=$(cd "$(dirname "$0")" && pwd)
CMD=${1:-}
[ $# -gt 0 ] && shift
WORK=/var/tmp/lordos-immutable
case "${1:-}" in /*) WORK=$1; shift ;; esac
MIRROR=${MIRROR:-http://deb.debian.org/debian}
SUITE=trixie
BASE="$WORK/base"
OUT="$WORK/out"
KEYS="$WORK/keys"
# Slot sizes are fixed for the life of an installation: a later release must fit. The usr image of
# this prototype is ~0.3 GB; the LordOS desktop's is ~2.5 GB (docs/IMMUTABLE.md, "Disk layouts").
SLOT_MB=${SLOT_MB:-1024}
HASH_MB=${HASH_MB:-64}
ROOT_MB=${ROOT_MB:-1024}
UKI_TRIES=${UKI_TRIES:-3}

for t in debootstrap mkfs.erofs veritysetup systemd-repart ukify mcopy mkfs.vfat gpg; do
    command -v "$t" >/dev/null || { echo "build.sh: missing $t (docs/IMMUTABLE.md, building the prototype)" >&2; exit 1; }
done

say() { echo "== $*"; }

umount_base() {
    for m in dev/pts dev proc sys run; do
        mountpoint -q "$BASE/$m" 2>/dev/null && umount -l "$BASE/$m" || true
    done
}

# hex root hash → the two partition UUIDs the Discoverable Partitions Specification derives from it
# (first 128 bits: data partition, last 128 bits: verity partition)
uuid_of() { echo "$1" | sed 's/^\(.\{8\}\)\(.\{4\}\)\(.\{4\}\)\(.\{4\}\)\(.\{12\}\).*/\1-\2-\3-\4-\5/'; }

test_key() {
    [ -s "$KEYS/update-keyring.gpg" ] && return 0
    say "throwaway update signing key (TEST ONLY) in $KEYS"
    mkdir -p "$KEYS"; chmod 700 "$KEYS"
    GNUPGHOME="$KEYS/gnupg"; export GNUPGHOME; mkdir -p "$GNUPGHOME"; chmod 700 "$GNUPGHOME"
    gpg --batch --quiet --passphrase '' --quick-gen-key 'LordOS immutable prototype TEST key' ed25519 sign 1y
    gpg --batch --export > "$KEYS/update-keyring.gpg"
}

cmd_base() {
    test_key
    umount_base
    trap umount_base EXIT
    rm -rf "$BASE"; mkdir -p "$BASE" "$OUT"
    say "debootstrap $SUITE (minbase)"
    debootstrap --variant=minbase --include=systemd,systemd-sysv,udev,kmod,dbus \
        "$SUITE" "$BASE" "$MIRROR" > "$WORK/debootstrap.log" 2>&1 || { tail -20 "$WORK/debootstrap.log"; exit 1; }
    for m in dev dev/pts proc sys run; do mount --bind "/$m" "$BASE/$m"; done
    cat > "$BASE/etc/apt/sources.list" <<EOF
deb $MIRROR $SUITE main
deb $MIRROR $SUITE-updates main
deb http://security.debian.org/debian-security $SUITE-security main
EOF
    # dracut first, so linux-image-amd64 doesn't pull initramfs-tools: Debian's initramfs-tools cannot
    # set up a dm-verity /usr from usrhash=, dracut's systemd-veritysetup module can
    chroot "$BASE" sh -c 'export DEBIAN_FRONTEND=noninteractive
        apt-get update -q && apt-get install -y -q --no-install-recommends dracut dracut-core \
          systemd-cryptsetup cryptsetup-bin systemd-boot systemd-resolved \
          e2fsprogs fdisk util-linux gpgv curl ca-certificates iproute2 procps less \
        && apt-get install -y -q --no-install-recommends linux-image-amd64 && apt-get clean' \
        > "$WORK/apt.log" 2>&1 || { tail -30 "$WORK/apt.log"; exit 1; }
    KVER=$(find "$BASE/lib/modules" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort -V | tail -1)
    echo "$KVER" > "$WORK/kver"

    say "configuration (kernel $KVER)"
    echo lordos-immutable > "$BASE/etc/hostname"
    printf '127.0.0.1 localhost\n127.0.1.1 lordos-immutable\n' > "$BASE/etc/hosts"
    : > "$BASE/etc/machine-id"
    # PROTOTYPE ONLY: root logs in on the serial console without a password, for the scripted demo
    mkdir -p "$BASE/etc/systemd/system/serial-getty@ttyS0.service.d"
    # shellcheck disable=SC2016 # $TERM is for systemd to expand, not this shell
    printf '[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --keep-baud 115200 %%I $TERM\n' \
        > "$BASE/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf"
    mkdir -p "$BASE/etc/systemd/network"
    printf '[Match]\nName=en*\n[Network]\nDHCP=yes\n' > "$BASE/etc/systemd/network/20-dhcp.network"
    chroot "$BASE" systemctl enable systemd-networkd systemd-resolved >/dev/null 2>&1 || true
    # the ESP is mounted at /efi (systemd-bless-boot and the updater look there)
    mkdir -p "$BASE/efi"
    printf 'PARTLABEL=lordos-esp /efi vfat umask=0077,x-systemd.automount 0 2\n' > "$BASE/etc/fstab"

    # LordOS's pieces: updater, health check (boot assessment), the update key
    install -D -m 0755 "$HERE/files/update" "$BASE/usr/lib/lordos/immutable/update"
    install -D -m 0755 "$HERE/files/health-check" "$BASE/usr/lib/lordos/immutable/health-check"
    install -D -m 0644 "$HERE/files/lordos-health.service" "$BASE/usr/lib/systemd/system/lordos-health.service"
    install -D -m 0644 "$HERE/files/lordos-health-failed.service" "$BASE/usr/lib/systemd/system/lordos-health-failed.service"
    install -D -m 0644 "$KEYS/update-keyring.gpg" "$BASE/usr/share/lordos/immutable/update-keyring.gpg"
    # enabled from /usr (a vendor .requires link), not /etc: a unit a later release adds is
    # enabled by that release's image alone
    mkdir -p "$BASE/usr/lib/systemd/system/boot-complete.target.requires"
    ln -sf ../lordos-health.service "$BASE/usr/lib/systemd/system/boot-complete.target.requires/lordos-health.service"
    ln -sf ../systemd-boot-check-no-failures.service \
        "$BASE/usr/lib/systemd/system/boot-complete.target.requires/systemd-boot-check-no-failures.service"

    say "initrd (dracut, systemd + dm-verity + EROFS)"
    chroot "$BASE" dracut --force --no-hostonly --kver "$KVER" \
        --add "systemd systemd-veritysetup dm" --omit "plymouth" \
        --add-drivers "erofs dm-verity ext4 vfat virtio_blk virtio_scsi virtio_pci sd_mod nvme ahci" \
        /boot/initrd.lordos > "$WORK/dracut.log" 2>&1 || { tail -30 "$WORK/dracut.log"; exit 1; }
    umount_base
    trap - EXIT
    # factory copy of /etc: what a release ships as defaults, for the three-way /etc merge
    # (docs/IMMUTABLE.md, "/etc"); the prototype only records it
    rm -rf "$BASE/usr/share/factory/etc"; mkdir -p "$BASE/usr/share/factory"
    cp -a "$BASE/etc" "$BASE/usr/share/factory/etc"
    say "base ready: $BASE ($(du -sh "$BASE/usr" | cut -f1) in /usr)"
}

cmd_release() {
    V=${1:?version}; BAD=${2:-}
    case "$V" in *[!0-9]*|'') echo "build.sh: version must be an integer" >&2; exit 1 ;; esac
    [ -d "$BASE/usr" ] || { echo "build.sh: run 'base' first" >&2; exit 1; }
    KVER=$(cat "$WORK/kver")
    R="$OUT/$V"; T="$WORK/tree-$V"
    rm -rf "$R" "$T"; mkdir -p "$R" "$T"
    say "release $V${BAD:+ (bad: fails its health check)}"
    cp -a "$BASE/usr" "$T/usr"
    # identity of this image version (os-release IMAGE_ID / IMAGE_VERSION, as systemd expects)
    sed -i '/^IMAGE_ID=/d;/^IMAGE_VERSION=/d' "$T/usr/lib/os-release"
    printf 'IMAGE_ID=lordos-immutable\nIMAGE_VERSION=%s\n' "$V" >> "$T/usr/lib/os-release"
    printf 'LordOS immutable prototype, image version %s, built %s\n' "$V" "$(date -u +%FT%TZ)" \
        > "$T/usr/lib/lordos/immutable/VERSION"
    if [ "$BAD" = bad ]; then : > "$T/usr/lib/lordos/immutable/simulate-broken-release"; fi

    # read-only EROFS (lz4hc: fast to decompress on low-end CPUs), fixed timestamps and UUID so the
    # same tree gives the same bytes
    mkfs.erofs -zlz4hc,12 -T0 -U "6c6f7264-6f73-4000-8000-$(printf %012d "$V")" \
        "$R/lordos_usr_$V.erofs" "$T/usr" > "$WORK/erofs-$V.log" 2>&1 || { tail "$WORK/erofs-$V.log"; exit 1; }
    veritysetup format "$R/lordos_usr_$V.erofs" "$R/lordos_usr_$V.verity" \
        --root-hash-file="$R/lordos_usr_$V.roothash" > "$WORK/verity-$V.log"
    H=$(cat "$R/lordos_usr_$V.roothash")
    U1=$(uuid_of "$H"); U2=$(uuid_of "$(echo "$H" | cut -c33-64)")
    D=$(stat -c %s "$R/lordos_usr_$V.erofs"); HS=$(stat -c %s "$R/lordos_usr_$V.verity")
    [ "$D" -le $((SLOT_MB * 1048576)) ] && [ "$HS" -le $((HASH_MB * 1048576)) ] \
        || { echo "build.sh: release $V does not fit the slots ($D / $HS bytes)" >&2; exit 1; }

    # The kernel command line is sealed into the UKI: usrhash pins this exact /usr; the verity
    # devices are found by the partition UUIDs derived from it, so the UKI picks its own slot.
    CMDLINE="usrhash=$H systemd.verity_usr_data=PARTUUID=$U1 systemd.verity_usr_hash=PARTUUID=$U2"
    CMDLINE="$CMDLINE mount.usr=/dev/mapper/usr mount.usrfstype=erofs mount.usrflags=ro"
    CMDLINE="$CMDLINE root=PARTLABEL=lordos-root rw console=ttyS0,115200 panic=10 rd.emergency=reboot rd.shell=0"
    ukify build --linux="$BASE/boot/vmlinuz-$KVER" --initrd="$BASE/boot/initrd.lordos" \
        --cmdline="$CMDLINE" --os-release="@$T/usr/lib/os-release" --uname="$KVER" \
        --output="$R/lordos_$V.efi" > "$WORK/ukify-$V.log" 2>&1 || { tail "$WORK/ukify-$V.log"; exit 1; }
    rm -rf "$T"
    say "release $V: usr $(du -h "$R/lordos_usr_$V.erofs" | cut -f1), verity $(du -h "$R/lordos_usr_$V.verity" | cut -f1), UKI $(du -h "$R/lordos_$V.efi" | cut -f1), usrhash $H"
}

cmd_disk() {
    V=${1:?version}
    R="$OUT/$V"
    [ -s "$R/lordos_$V.efi" ] || { echo "build.sh: run 'release $V' first" >&2; exit 1; }
    H=$(cat "$R/lordos_usr_$V.roothash")
    U1=$(uuid_of "$H"); U2=$(uuid_of "$(echo "$H" | cut -c33-64)")
    S="$WORK/stage"; DEF="$WORK/repart.d"
    rm -rf "$S" "$DEF"; mkdir -p "$S/esp/EFI/BOOT" "$S/esp/EFI/systemd" "$S/esp/EFI/Linux" "$S/esp/loader" "$S/rootpart" "$DEF"
    say "disk image with release $V in slot A"
    cp /usr/lib/systemd/boot/efi/systemd-bootx64.efi "$S/esp/EFI/BOOT/BOOTX64.EFI"
    cp /usr/lib/systemd/boot/efi/systemd-bootx64.efi "$S/esp/EFI/systemd/systemd-bootx64.efi"
    printf 'timeout 3\neditor no\nauto-entries no\nauto-firmware no\n' > "$S/esp/loader/loader.conf"
    # the factory image's own version is already known good: no boot counter
    cp "$R/lordos_$V.efi" "$S/esp/EFI/Linux/lordos_$V.efi"
    # root partition: everything but /usr; merged-/usr symlinks (Debian 13 is merged-/usr only)
    (cd "$BASE" && tar --exclude=./usr/* --exclude=./proc/* --exclude=./sys/* --exclude=./dev/* \
        --exclude=./run/* --exclude=./boot/* -cf - .) | tar -C "$S/rootpart" -xf -
    cat > "$DEF/10-esp.conf" <<EOF
[Partition]
Type=esp
Label=lordos-esp
Format=vfat
CopyFiles=/esp:/
SizeMinBytes=512M
SizeMaxBytes=512M
EOF
    cat > "$DEF/20-usr-a.conf" <<EOF
[Partition]
Type=usr
Label=lordos_usr_$V
UUID=$U1
CopyBlocks=$R/lordos_usr_$V.erofs
SizeMinBytes=${SLOT_MB}M
SizeMaxBytes=${SLOT_MB}M
EOF
    cat > "$DEF/21-usr-a-verity.conf" <<EOF
[Partition]
Type=usr-verity
Label=lordos_usr_v_$V
UUID=$U2
CopyBlocks=$R/lordos_usr_$V.verity
SizeMinBytes=${HASH_MB}M
SizeMaxBytes=${HASH_MB}M
EOF
    cat > "$DEF/30-usr-b.conf" <<EOF
[Partition]
Type=usr
Label=_empty
SizeMinBytes=${SLOT_MB}M
SizeMaxBytes=${SLOT_MB}M
EOF
    cat > "$DEF/31-usr-b-verity.conf" <<EOF
[Partition]
Type=usr-verity
Label=_empty
SizeMinBytes=${HASH_MB}M
SizeMaxBytes=${HASH_MB}M
EOF
    cat > "$DEF/40-root.conf" <<EOF
[Partition]
Type=root
Label=lordos-root
Format=ext4
CopyFiles=/rootpart:/
SizeMinBytes=${ROOT_MB}M
EOF
    rm -f "$OUT/disk.raw"
    systemd-repart --empty=create --size=auto --sector-size=512 --dry-run=no --no-pager --definitions="$DEF" \
        --copy-source="$S" "$OUT/disk.raw" > "$WORK/repart.log" 2>&1 || { tail -30 "$WORK/repart.log"; exit 1; }
    rm -rf "$S"
    sfdisk -l "$OUT/disk.raw"
    say "disk ready: $OUT/disk.raw ($(du -h --apparent-size "$OUT/disk.raw" | cut -f1), $(du -h "$OUT/disk.raw" | cut -f1) on disk)"
}

cmd_publish() {
    [ $# -gt 0 ] || { echo "build.sh: publish needs the versions to offer" >&2; exit 1; }
    P="$OUT/updates"
    rm -rf "$P"; mkdir -p "$P"
    for V in "$@"; do
        [ -s "$OUT/$V/lordos_$V.efi" ] || { echo "build.sh: no release $V" >&2; exit 1; }
        cp "$OUT/$V"/lordos_* "$P/"
    done
    chmod 0644 "$P"/lordos_*
    (cd "$P" && sha256sum lordos_* > SHA256SUMS)
    GNUPGHOME="$KEYS/gnupg"; export GNUPGHOME
    gpg --batch --yes --detach-sign --output "$P/SHA256SUMS.gpg" "$P/SHA256SUMS"
    gpgv --keyring "$KEYS/update-keyring.gpg" "$P/SHA256SUMS.gpg" "$P/SHA256SUMS" 2>&1 | tail -1
    say "update server folder: $P (serve it over HTTP; the guest reaches the host at 10.0.2.2)"
    ls -l "$P"
}

case "$CMD" in
    base) cmd_base ;;
    release) cmd_release "$@" ;;
    disk) cmd_disk "$@" ;;
    publish) cmd_publish "$@" ;;
    all) cmd_base; cmd_release 1; cmd_release 2; cmd_release 3 bad; cmd_disk 1; cmd_publish 1 2 ;;
    *) sed -n '2,24p' "$0"; exit 2 ;;
esac
