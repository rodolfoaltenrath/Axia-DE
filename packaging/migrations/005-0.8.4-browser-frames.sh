# 0.8.4: the preinstalled browsers draw their own bar, with LordOS's window buttons in it (traffic
# lights through LordOS's GTK theme), instead of the LordOS frame on top (docs/WAYLAND.md, "Frame
# and colors"). The ISO up to 0.8.3 wrote the opposite defaults; this changes them where they are
# still exactly what LordOS wrote, so an administrator's own choice stays:
#
# - /etc/chromium/master_preferences: "custom_chrome_frame": false → true, and the GTK theme
#   ("extensions": {"theme": {"system_theme": 1}}) when the file sets no "extensions" yet. These are
#   initial preferences: they only reach Chromium profiles created from now on.
# - /etc/firefox-esr/lordos.js, if it is still the file the ISO wrote: inTitlebar 0 → 1. A default
#   pref, so every profile follows it except one whose user changed «Title Bar» (a user_pref, kept).
#
# Home folders are never touched: an existing Chromium profile keeps the frame it had; its user
# switches with a right click on the tab strip → «Use system title bar and borders» off, and
# Settings → Appearance → Theme → «GTK». LORDOS_MIGRATION_ROOT is for tests only.
R=${LORDOS_MIGRATION_ROOT:-}

CH="$R/etc/chromium/master_preferences"
if [ -f "$CH" ] && grep -q '"custom_chrome_frame": false' "$CH"; then
    t=$(mktemp "$CH.XXXXXX")
    sed 's/"custom_chrome_frame": false/"custom_chrome_frame": true/' "$CH" > "$t"
    if ! grep -q '"extensions"' "$t"; then
        # the file is a JSON object written one key per line (json.dump indent=2): the GTK theme
        # goes in as its first key
        sed '1s/^{$/{\n  "extensions": {\n    "theme": {\n      "system_theme": 1\n    }\n  },/' "$t" > "$t.2" && mv "$t.2" "$t"
    fi
    chmod 644 "$t"
    mv "$t" "$CH"
fi

FF="$R/etc/firefox-esr/lordos.js"
OLD_FF='// LordOS: Firefox windows use the system frame (the theme frame); change it in
// Customize Toolbar > Title Bar, or in about:config.
pref("browser.tabs.inTitlebar", 0);'
if [ -f "$FF" ] && [ "$(cat "$FF")" = "$OLD_FF" ]; then      # exactly the file the ISO wrote
    t=$(mktemp "$FF.XXXXXX")
    printf '%s\n' '// LordOS: Firefox draws its own bar, with the window buttons in LordOS'"'"'s order (see' \
        '// docs/WAYLAND.md); 0 gives it the LordOS frame instead (Customize Toolbar > Title Bar).' \
        'pref("browser.tabs.inTitlebar", 1);' > "$t"
    chmod 644 "$t"
    mv "$t" "$FF"
fi
exit 0
