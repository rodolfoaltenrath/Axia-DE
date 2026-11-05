Axia Window Controls
====================

Original Axia DE artwork, distributed under the repository's MIT license.
This is a declarative VS Code product icon theme. No extension code runs.
Only chrome-close, chrome-minimize, chrome-maximize and chrome-restore change;
editor colors and file icons remain under the user's control.

The COLR/CPAL TrueType font is committed so normal builds need no font tooling.
Monochrome outline fallbacks keep controls visible without color-font support.
To regenerate it, install fonttools in an isolated Python environment and run
tools/desktop/build_controls_font.py. The generated font has a stable timestamp.

Axia installs these inert resources per user and supplies theme defaults for
the current light/dark mode. Disabling the integration removes those defaults
while leaving the font available to already-open editor windows.
