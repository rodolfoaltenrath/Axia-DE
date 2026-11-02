# 0.8.1: per-app network permissions (lord-netperm) on wherever LordOS starts at boot.
# The ISO, the Raspberry Pi image and install.sh enable lord-netperm.service, but the 0.8.0 package
# itself did not, so a system set up with apt alone had the firewall off. Runs once: if an
# administrator disables it afterwards, it stays disabled.
command -v systemctl >/dev/null 2>&1 || exit 0
[ "$(systemctl is-enabled lordos.service 2>/dev/null || true)" = enabled ] || exit 0
systemctl enable lord-netperm.service >/dev/null 2>&1 || true
if [ -d /run/systemd/system ]; then systemctl start lord-netperm.service >/dev/null 2>&1 || true; fi
