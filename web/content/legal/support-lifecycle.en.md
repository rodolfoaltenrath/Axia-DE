This page explains which LordOS versions receive fixes, how updates reach you, and what we plan for after version 1.0.

## Current status: beta

LordOS **0.8.0 beta** is the first public release (0.7.0 was never published). What each release contains is in its notes on [GitHub Releases](https://github.com/lordbasex/LordOS/releases). Beta releases can change behaviour, settings and file formats between versions, and they are not meant for critical work. See the [Terms of use](/legal/terms/).

## Which versions get fixes

Until 1.0, **only the latest beta release** receives fixes, including security fixes. When a new beta comes out, the previous one stops receiving fixes, so please upgrade to the newest release. We don't backport fixes to older betas.

## How updates are delivered

LordOS has two kinds of updates:

- **The Debian base.** The kernel, drivers, libraries and every Debian package get their updates, including security updates from `security.debian.org`, from Debian. Install them from the Store's Updates tab (or Settings → Updates), or with `apt` in the Terminal. These keep coming as long as Debian supports Debian 13.
- **LordOS itself.** New LordOS versions are published on [GitHub Releases](https://github.com/lordbasex/LordOS/releases) as a `.deb` package, a PC ISO and a Raspberry Pi 5 image, each signed (see the [Code signing](/legal/code-signing/) page). On Debian, running the one-line installer again (`curl -fsSL https://lordos.org/install.sh | sh`) upgrades LordOS; you can also install a downloaded `.deb` from the Store. A built-in updater for LordOS itself is not available yet.

LordOS never checks for or downloads updates on its own: you decide when.

## After 1.0 (planned)

This is our plan, not a promise, and it may change before 1.0:

- each stable release will receive fixes, including security fixes, until the next stable release, plus an overlap window so you have time to upgrade;
- the Debian base will follow Debian's own lifecycle: when Debian ends security support for a release, LordOS will move to the next Debian release before that date;
- critical security fixes will be released as soon as they are ready, outside the normal schedule.

## End-of-support announcements

Before a version stops receiving fixes, we will announce it on lordos.org and on GitHub, with the date and the version to upgrade to.

## Contact

Questions: [our contact form](https://www.iperfex.com/contacto/?lang=en). Security problems: see the [Security policy](/legal/security/).
