# Keyboard shortcuts

The desktop's shortcuts live in one place. A key goes first to `wl_key` (`server/src/wl/wl.c`), which
decides whether a focused Wayland app gets it, then to one dispatcher in the session
(`sdk/shortcuts.js`), which runs before every listener. A key it takes reaches nobody else, so one key
never fires twice.

## Axia keyboard profiles

Settings → Keyboard offers **macOS** and **Linux / Windows**, independently of
the visual theme. The choice is stored in `shortcutProfile` and applied live.
Without an explicit choice the profile is macOS. (Until the Lord Classic theme was removed, its users
got Linux / Windows by default; they now keep it only if they chose it.)

| Action | macOS | Linux / Windows |
|---|---|---|
| Search | Super+Space | Super+Space |
| Switch apps | Super+Tab, Super+Shift+Tab | Super+Tab, Super+Shift+Tab; Alt+Tab, Alt+Shift+Tab |
| Cycle this app's windows | Super+grave, Super+Shift+grave | Alt+grave, Alt+Shift+grave |
| Minimize | Super+M | Super+H |
| Close a window | Handled by the app | Alt+F4 |
| Show desktop | Super+F11 | Super+D |
| Files / Settings | Launcher | Super+E / Super+I |
| System Monitor | Launcher | Ctrl+Shift+Esc |
| Area screenshot | Super+Shift+4 | Super+Shift+S |

Super is the Windows/Command key. Ctrl shortcuts continue to belong to the
applications. In the macOS profile, Super editing shortcuts for Linux apps are
translated into Ctrl as before; this translation is disabled for Linux / Windows.
Native apps retain their own Ctrl aliases. This setting configures desktop
bindings; it cannot rewrite the shortcut settings of external applications.

Search and Super+Tab are active by default in both profiles. The switcher
commits when the modifier that opened it is released (Super or Alt).
If Super alone and all Super shortcuts stop responding together, check the
keyboard's hardware Win Lock/game mode; the desktop cannot enable a key that
the keyboard firmware suppresses.

`sdk/shortcutprofile.js` defines the bindings and `sdk/shortcuts.js`
composes them with the theme and optional modules. Desktop navigation uses the
matching profile as well. The C-side fixed grab tables accept `macos`,
`macos-desktops` and `linux`; legacy theme names remain supported.
Changing profiles cancels an open app selector to avoid committing with a
different modifier.

## The dispatcher

`runtime/events.js` calls the filter set with `setKeyFilter` before any `addListener('keydown')`, and only
`startShell` sets it: an isolated app's process has no dispatcher. The tables:

- `BASE` in `sdk/shortcuts.js`: search, keyboard layout, clipboard, emoji, lock, quit and the media keys.
- A theme's table, given to `useShortcuts({ grabs, table, onSuperTap, appsPaste })` from its root component
  (`flowTable` in `themes/default/index.jsx`).
- `addShortcuts(entries)` from a module: Screenshots and the Terminal's drop-down window. `runShortcut(id)`
  runs one of those from a table: the Linux / Windows profile's ❖⇧S is Screenshots' area capture.

Lookup order is modules, theme, base, and the first entry whose `when(e)` allows it wins. When none does the
key goes on to the listeners, so `when` is how a key stays with an app (⌃Space in the Terminal).

```js
{ id: 'minimize', keys: ['Meta+M'], run: minimizeFront, label: 'shell.shortcuts.minimize', group: 'windows' }
```

`run` is a function, never a command string. `keys` uses `Meta`, `Ctrl`, `Alt`, `Shift` and a key name. Letters
match what the layout produces, or the US position on a layout without Latin letters; other keys match
their position. `label` lists the entry in Settings → Keyboard (`shortcutList()`, read only), and `show`
replaces its keys there; an entry with no `label` is not listed. A held key runs once unless the entry says
`repeat: true`, and its repeats and its release are taken as well.

❖ pressed and released alone calls the theme's `onSuperTap`: Lord Flow's launcher.
While a Linux app is fullscreen this tap is ignored, so a game cannot open a hidden launcher and blur
behind itself. Super combinations, including the app switcher and the fullscreen escape shortcut, remain available.

## The keys kept from apps

`rust/axia-shell/src/keygrab.rs` holds fixed tables, one per shortcut profile, written at build time: keyboard keys only, each
a modifier set plus a letter or an evdev code. `wl_key` compares modifiers exactly, so ⌃Tab and plain ⌃Q
always stay with the app. The shell picks a table by name, `lord.wayland.grabKeys('macos' | 'macos-desktops' | 'linux' | '')`
(`flow` and `flow-desktops` are the same as the first two):
any other name is refused, and nothing can add a key. `lord.wayland` does not exist in an isolated app's
process (`js_broker_register`), which has no compositor to change either. `startShell` clears the table,
so a theme that asks for none keeps none.

| Table | Keys |
|---|---|
| `macos` | ❖Tab, ❖⇧Tab (hold), ❖M, ❖H, ❖⌥H, ❖⌥D, ❖,, ❖⌃← ❖⌃→ |
| `macos-desktops` | The same plus ⌃ ← → ↑ ↓: Settings → Keyboard's Desktops arrows on |
| `linux` | ⌥Tab, ⌥⇧Tab (hold), ⌥\`, ⌥F4, ⌃⇧Esc, ❖A, N, D, E, H, I, R, ❖⇧S, ❖ arrows, ❖1…9, ❖⌃D, ❖⌃F4, ❖⌃← ❖⌃→ |

A Linux app never gets a ❖ combination, so for ❖ the table matters to isolated LordOS apps: without it those
would receive the key and the theme would act as well. `system_meta_key` in `wl.c` still lists the ❖ keys
every theme keeps.

**Hold.** The switcher needs the modifier's release. After a *hold* key, every other key goes to the shell
until the modifier that started it is up, while modifier keys go to both, so an app keeps a correct
state whichever window ends up in front. A key pressed before the switcher opened still gets its release
in the app. A fullscreen Linux app is temporarily minimized before the selector is drawn, releasing
its pointer lock. Choosing another app leaves it minimized; returning to it restores fullscreen.
Esc cancels the selection and restores the original fullscreen app as well.
The compositor suspends presentation while minimized without changing the client's fullscreen mode
or dimensions. This avoids a windowed/fullscreen resize cycle on every switch; the app may still
throttle rendering when unfocused. Explicitly leaving fullscreen restores its original window size.

## ❖ as Ctrl in Linux apps (Lord Flow)

With the `flow` table, ❖C, V, X, A, Z, F, T, W, N, L, R, P and S with no other modifier reach the focused Linux
app as Ctrl+key, or Ctrl+Shift+key if its `.desktop` lists the `TerminalEmulator` category (`sdk/wayland.js`
passes that with `lord.wayland.focus`). `send_remapped` follows `wl_send_paste`: only that client, with
Super taken out of the modifiers it sees, and the real press and release, so the app repeats on its own.
`wl_key` returns 2, which the shell receives as `forwarded`: nothing runs, but ❖ no longer counts as pressed
alone. So with a Linux app in front ❖L is that app's, and the screen locks with ❖⌃Q; ❖V pastes and the
clipboard is ❖⇧V. Isolated LordOS apps keep their real ❖ keys.

With `appsPaste` (Lord Flow) ❖V also goes to a LordOS app inside the session, where it pastes, and opens the
clipboard panel only with no window in front. Isolated apps never receive ❖V, so there it stays the panel.

## Desktops

`flowDesktopKeys` (macOS profile) and `linuxDesktopKeys` (Linux / Windows profile) in `sdk/desktops.js` join the table; [DESKTOPS.md](DESKTOPS.md)
has the keys. In Lord Flow ⌃ arrows are word movement in apps, so they are an option, on by default: off, their
entries stop applying and leave Settings → Keyboard's list (`listed`), and `useShortcuts` asks for the `flow` table
(its `grabs` can change while the theme runs), so the keys reach the apps. ⌃⇧ arrows always stay with the app.

A key pressed with no Linux app in front and released with one (the key that switched to its desktop) has its
release delivered to that app, and the dispatcher never sees it: a new press of a key still marked counts as a press.

## The app switcher

Axia's selection model is implemented in Rust and embedded in axia-server;
see [RUST.md](RUST.md) for ownership, build and test details. The SDK provides
window metadata and renders the overlay with the shared Dock glass material.
The older theme-specific table descriptions below describe the legacy aliases;
the explicit keyboard profile overrides their system bindings.

`sdk/appswitcher.jsx`, mounted by each theme inside its `ThemeProvider`, in the overlay layer. Apps are
grouped as the Dock groups them (`runningApps`), most recently used first. It opens on the previous app, and
the dispatcher gives it every key while open: Tab, ⇧Tab and the arrows move, Enter or the modifier's release
switches, Esc cancels. Switching brings the app's top window, on this desktop if it has one. It lists the apps of every
desktop, with the desktop's name, and choosing one elsewhere jumps there. ❖\` and ❖⇧\` in Lord Flow go through the
visible windows of the app in front on this desktop.

## Testing

`build/keygrab-test` (`make -C server test`) checks the tables. Keys were tried on a VM with real key events
over a Linux app (Mousepad, foot), an app inside the session (Terminal) and an isolated app (Calendar).
