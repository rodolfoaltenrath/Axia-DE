# 0.8.3: Debian's smartd stays off (the package now ships a drop-in for smartmontools.service with
# ConditionPathExists=/etc/lordos/smartd-enabled, docs/SYSTEM.md "smartd"). A condition is only
# checked at start, so a smartd already running from an earlier smartmontools install is stopped
# here once. Nothing to do where smartmontools isn't installed or an administrator opted in.
[ -d /run/systemd/system ] || exit 0
command -v systemctl >/dev/null 2>&1 || exit 0
[ -e /etc/lordos/smartd-enabled ] && exit 0
systemctl daemon-reload 2>/dev/null || true
if systemctl is-active --quiet smartmontools.service 2>/dev/null; then
    systemctl stop smartmontools.service 2>/dev/null || true
fi
exit 0
