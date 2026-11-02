# 0.8.2: SATA disk temperatures (Monitor → Disks). The package now ships
# /usr/lib/modules-load.d/lordos-drivetemp.conf, read at boot; on a running system the module is
# loaded once here so the temperatures show without a restart. Harmless where the kernel has no
# drivetemp (the Monitor then says "No sensor").
[ -d /run/systemd/system ] || exit 0
command -v modprobe >/dev/null 2>&1 || exit 0
modprobe -q drivetemp 2>/dev/null || true
