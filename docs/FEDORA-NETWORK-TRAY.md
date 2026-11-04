# Fedora network and background applications

The Fedora session keeps NetworkManager in charge of networking. Axia never
starts its own DHCP client, wpa_supplicant, or privileged network helper.

The menu bar shows the primary physical connection: wired Ethernet or Wi-Fi
with signal strength. Both the connection icon and the quick-settings button
open the glass Control Center. Its network tile lists SSIDs, groups access
points with the same raw SSID and security, and prefers the connected access
point over a stronger duplicate. SSID bytes are retained for connections;
invalid/control characters are replaced only for display. IPv6-only connections
also work: connection state comes from NetworkManager, not an IPv4-address test.

Selecting a network offers connection with a password, a saved profile, or no
password for an open/OWE network. New profiles use NetworkManager's completion
and storage. Updating a saved personal-network password preserves the rest of
its settings, including manual IP configuration. No passwords enter command
arguments, shell commands, logs, Axia settings files, or the Axia keyring.
The secure field clears when its panel closes or the request is submitted.
Authorization is handled by NetworkManager/polkit. Changes to the Wi-Fi radio
require an explicit user click. Opening the panel only requests a scan.

New enterprise/legacy networks require configuration in a system connection
editor first. Existing compatible saved enterprise profiles can be activated;
this panel does not implement an enterprise secret-agent prompt. Hidden SSIDs
are not listed. Association success and Internet availability are different:
joining a hotel/captive network does not guarantee Internet access.

The background-app button opens a separate glass tray. The Rust host implements
`org.kde.StatusNotifierWatcher`, StatusNotifierItem/AppIndicator registrations,
icon pixmaps/raster icon names, activation, and `com.canonical.dbusmenu` menus.
It can also use an existing watcher without stealing its name. Icons disappear
when their bus owner exits. Menus use the same shared glass Menu as Window and
Files, including separators, checkmarks, disabled actions and submenus.
Application-provided menu actions retain their original labels and behavior.
Apps with ItemIsMenu open their menu on the main click.

This is a protocol tray, not a process list: the app must publish a tray item.
Flatpak apps need permission to talk to the watcher, and apps using only legacy
XEmbed are not supported. SVG-only icons fall back to an application initial
unless the app supplies a pixmap/raster icon. The app remains operable.
The implementation never kills, restarts, or closes an application on its own.

## Implementation and resource use

`rust/axia-shell/src/network.rs` owns network policy, credentials, connection
workflow, panel descriptions and vector status icons. `network_bus.rs` provides
typed GIO D-Bus transport. `tray.rs` owns the watcher, app state, menus, actions,
icon decoding and tray layout. The C functions in `ui/js_net.c` pass bounded
JSON requests into Rust; `sdk/network.jsx` and `sdk/tray.jsx` bind Rust-owned
widget descriptions to the existing renderer. No business logic is duplicated
in a new JavaScript/C network implementation.

Each service has one worker and a bounded command queue. All D-Bus calls have
deadlines and run outside the compositor thread. Signals mark cached state
dirty; refreshes are coalesced with a low-frequency recovery refresh. The UI
publishes changed widget descriptions only. Icon pixmaps are dimension/length
checked; file icons are opened nonblocking, limited to 4 MiB and decoded with
16 MiB / 2048-pixel bounds. Rust downsizes icons to 32 pixels before the UI sees
them. Old cached versions are removed, and the renderer releases obsolete tray
images rather than retaining every animated icon frame.

## Validation

Run the ordinary desktop checks with `make check GPU=1` in the Fedora build
environment. Rust policy tests run with `cargo test --manifest-path
rust/Cargo.toml --target-dir server/build/rust --locked --offline`.

After building those test binaries, a host with PyGObject and dbus-daemon can
exercise both protocol integrations on a private bus:

```sh
dbus-run-session -- python3 tools/desktop/network-tray-check.py
```

The fixture substitutes a fake NetworkManager and a fake background app. It
checks scan/radio requests, SSID handling, saved/new activation, password
updates preserving IP settings, authorization errors, watcher registration,
pixmaps, app activation, menu layout/events and cleanup on app termination.
It never changes the real host's networking. CI runs it for the GPU-enabled
matrix entry. Actual associations, driver/rfkill behavior and each application's
tray implementation still require testing on the installed session.

Protocol references: [NetworkManager D-Bus](https://networkmanager.dev/docs/api/latest/spec.html),
[StatusNotifierWatcher](https://specifications.freedesktop.org/status-notifier-item/latest/status-notifier-watcher.html).
