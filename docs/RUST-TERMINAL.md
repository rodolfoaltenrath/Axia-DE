# Terminal migration

The Terminal app still has a React window (`apps/terminal/index.jsx`, `Chrome.jsx`,
`Panes.jsx`, `Preferences.jsx`) over JavaScript state (`state.js`, `tmux.js`, `iterm.js`).
Emulation, drawing, the PTY and the mouse/search/selection engine are C
(`server/src/term`); profiles, key bindings and the bundled palettes are Rust
(`model.rs`, `keys.rs`, `data.rs`, `key_data.rs`) behind the thin `profiles.js` and
`keys.js` transports.

The compiled-app policy test requires every file under `apps/terminal/` to be Rust, so
the JSX, JS and `manifest.json` leave in the last stage, in one commit with the registry
change. Until then the React window stays the live app.

## Stages

1. **Host node (done).** `{"kind":"terminal","session":<id>}` is a native-view node.
   The host creates the compositor's terminal node (`NODE_TERM`), styles it like any
   view and binds it to the session the app names; an unknown or malformed id binds
   session 0, and changing the id rebinds the same node (a tab switch). The C side keeps
   one binding path, `js_term_node` (`js.c`), used by the React `session` prop and by the
   host. The renderer paints cells, cursor, selection and images itself; the view adapter
   only names the session. Pointer events reach the app through the ancestor view's
   `onMouseDown/Up/Move`, `onDoubleClick`, `onContextMenu` and `onWheel` (position is
   relative to the view), so the pane view carries them, as `Panes.jsx` does today.
   Tests: `native_view::tests::terminal_nodes_*` (host) and the GPU CI runtime test
   `terminal_node_runtime` (real sessions told apart by their profile background, two
   panes, rebinding, release in light and dark at two widths).
2. **Window base.**
   - **2a (done): the workspace model.** `workspace.rs` is the tree/tab part of `state.js`
     with no I/O: windows → tabs → pane trees → session ids (new tab after the active one,
     split at the focused pane, which tab is active after a close, undo of a closed pane with
     its expiry, pane to tab, zoom, equalize, tab titles from the session title/folder/program,
     font steps, saved arrangements and live layouts rebuilt with new or surviving sessions,
     pane and divider rectangles in flex order, neighbor pane for keyboard navigation).
     Sessions are numbers the transport creates and tells the model about. 12 unit tests.
   - **2b-1 (done): the key path.** A native app used to get only (key, char, modifiers) from the
     shell, which cannot express what a terminal needs: the physical code for shortcuts, the
     base key under Ctrl/Meta, auto-repeat. `NativeApp::key_event(key, chr, flags, code, base,
     repeat)` carries them (default: it calls `key`, so no other app changes); the chain is
     `events.js` → `lord.nativeViews.key` (optional extra arguments) → `axia_view_key_event` →
     `Host::key_event`. Key-up events are still not delivered to native views.
     Test: `native_view::tests::keyboard_apps_receive_the_physical_code…`; the focus, Spotlight
     and screenshot-selector runtime tests pass on the rebuilt compositor.
   - **2b-1b (done):** Shift is not in the key flags (the text fields read the shifted name), so
     `key_event` takes a `KeyFacts { code, base, repeat, shift }`.
   - **2b-2 (done): the window.** `native.rs` / `native_ui.rs`, compiled into
     `axia-app-terminal` and not registered (the React window is still the live app). One
     workspace per shell session (a `thread_local`) shared by every window: a window mounted
     again finds its tabs and shells. It has the tab bar (numbers, bell/activity/failed marks,
     close, new tab), the pane tree as absolutely placed `terminal` nodes with draggable
     dividers, badges and inactive dimming, the status bar, the window title, keyboard focus
     that follows the window and the active pane, the key chain of `index.jsx` (key
     mappings, shortcut actions, then the key as bytes) and the mouse (pane-local, coalesced,
     capture while dragging; the wheel is three lines a notch). Actions done: tabs and panes
     (new, split, close with a five-second undo, next/previous, by direction, move, pane to
     tab, equalize, zoom), font size, timestamps, scrolling, marks, clear/reset, select all.
     29 crate tests (12 for the window).
     **Transport contract** for stage 6: jobs `term.create` ([{cwd,scrollback}] → id),
     `term.configure` ([sid,config]), `term.close` ([sid]), `term.infos` ([[sid…]] →
     [{sid,info}]), `term.events` (→ [{sid,type,data}], buffered while no window is mounted);
     synchronous effects `{kind:"term",op,sid,…}`: `write` (`bytes` or UTF-16 `units`), `key`
     (`key,char,base,mods`), `mouse` (`type,x,y,button,mods`; `rememberCopy` on release: a
     result of 3 means "remember the clipboard"), `wheel`, `focus`, `scroll`, `clear`,
     `select`, `mark`; and `{kind:"closeWindow",id}`.
     **Not yet**: the actions that need the clipboard, popups or bars (copy/paste, find,
     toolbelt, composer, copy mode, replay, command and paste history, titles, logging), links
     on ❖-click, the context menu, the application menus and the close confirmation for busy
     shells (stage 3), broadcast input, tmux (stage 5), saving arrangements and the live
     layout. A session whose last window closed during its undo window is only closed by the
     next window that mounts; the session-level service of stage 6 must run that timer.
3. **Bars and dialogs.**
   - **3a (done):** toasts (four at most, four seconds), the close confirmation for panes and
     tabs that run a program (Escape cancels, Enter confirms, the dialog takes every key),
     copy, paste, paste on one line, paste quoted for the shell, select the last output, and the
     find bar (plain search by the engine; regular expressions in Rust over the logical lines,
     handed to the engine as (line, first character, length) triples; case, next/previous,
     Shift+Enter, follows the focused pane). The workspace entry of a window now also keeps
     what it shows on top of the panes. 37 crate tests.
     New transport jobs: `term.copy`, `term.selectOutput`, `term.search`, `term.searchMove`,
     `term.logicalText`, `term.setMatches`, `clipboard.read` (→ {text}), and
     `clipboard.remember` (put the text on the clipboard and in its history). The `paste` effect
     carries `text`. Rust's `regex` has no look-around or back-references, unlike the
     JavaScript expressions of the React window: such a pattern is reported as invalid.
     A field's `onKey` hook now also reports Shift.
   - **3b (done):** the list popups (autocomplete from the words on screen, command history,
     paste history, all keyboard driven: arrows cycle, Enter accepts, Escape closes, any other
     key is swallowed), open quickly (filters the sessions of every window by title and brings
     the window of the one chosen forward), edit the tab title (empty = automatic), the composer
     (Enter sends a line, Shift+Enter sends the text unrun, Escape closes), broadcast input to
     every pane of the tab (typing, paste, composer), the toolbelt (jobs, recent commands,
     clipboard history, recent folders quoted for `cd`, profiles, notes; it takes 240 px from
     the panes), instant replay (◀ ▶ and the arrow keys, Escape/Enter back to now; the slider
     of the React bar is not ported: the position is a counter) and copy mode (its keys go to
     the engine one job at a time; status 2 copies, any non-zero status ends it). The shared
     workspace also keeps the recent folders, the command history and the notes. 46 crate tests.
     More transport: jobs `term.cursorWord`, `term.words`, `term.replay`, `term.copyKey`,
     `clipboard.history` (→ [text…]); effects `copyMode`, `{kind:"focusWindow",id}`. A `write`
     effect carries `text` (ordinary strings) or `bytes`/`units` (key mappings). The captured-output
     section of the toolbelt (profile triggers) waits for the triggers.
   - **3c (done):** the four application menus (Shell, Edit, View, Profiles: resolved labels,
     the first shortcut as hint, checked and disabled items; published once and again only when
     something changes; a click returns as `{kind:"action",action:[cmd,key]}` in `term.events`),
     the pane's context menu (right click), Ctrl/Super-click on a link (`term.urlAt`: a path
     in the home folder opens Files, anything else is copied; with no link the click is the
     program's), the close guard (`windows.guard` [window, busy] tells the transport whether a
     program holds the window; a refused close arrives as `{kind:"closeRequested"}`, asks, and
     confirming sends `closeWindow` with `force`), new window, log the session, export the text,
     an ssh/sftp line another app leaves typed (validated, typed once the shell shows output),
     the profile's "text on open", and the Dock's new-tab action (the window's `args.action`).
     52 crate tests.
     Not ported, on purpose: save/restore arrangements and the live layout (they need the
     settings store: stage 4/6), full screen, "alert when the command finishes", the
     Preferences item, the frame of a new window sized from the profile's columns and rows
     (the shell has no window frame to give), and the Dock's profile entries. The Dock menu is a
     registration: stage 6 declares `dockActions` in the manifest (new window with `new`, new tab).
     A repeated Dock click with identical args is seen once until `sdk/dock.js` adds a nonce.
     Transport: jobs `windows.menus`, `windows.guard`, `windows.open`, `term.urlAt`, `term.log`
     (→ path or null), `term.export` (→ path or null); the lease event `closeRequested`.
4. **Preferences.**
   - **4a (done): the importer.** `iterm.rs` is `iterm.js` in Rust: profile JSON and Dynamic
     Profiles, `.itermcolors` (XML), the full iTerm2 preferences (XML plist and a bounds-checked
     `bplist00` reader, with a depth/cycle guard), Display P3 → sRGB, key combinations and
     actions, triggers, working directory, fonts. The caller injects the translator, the font
     lookup (PostScript name → family) and the id generator through `Env`. 12 unit tests.
   - **4b (done): the native Preferences window.** `prefs.rs` (state and commands), `prefs_ui.rs`
     (the scene) and `prefs_tests.rs`, in the window's own module tree. It is an overlay that
     covers the window (state per window in `WinUi.prefs`; opened by the `preferences` action,
     the Profiles menu and the pane's context menu; Done, or Escape one layer at a time, closes
     it). Sidebar with the profiles (add, duplicate, remove, default ★) and seven sections
     (general, colors, text, window, terminal, keys, triggers), the form of `Preferences.jsx` with
     its labels above the controls under 480 px; controls are toggles, segments, steppers,
     sliders (dragged with the pointer captured), inline pickers, color wells that take
     `#rrggbb`, scheme cards, key-combination buttons and text fields. Every change goes through
     one path: whitelisted and range-checked setters (`clean_field`: unknown, locked or
     mistyped fields and unknown options are refused), the settings updated at once, one
     `settings` effect (`{"kind":"settings","patch":{"terminal":…}}`) and every session of every
     window configured again, once per request. A key press while the Preferences are open never
     reaches the shell: it is captured for a shortcut or a key-map entry (modifiers alone are
     ignored, Escape cancels, a combination moves to the action that captures it and leaves the
     others). The import panel reads a file inside the home folder only (8 MiB at most, text or
     `bplist00`), lists the exports it finds in Downloads, Documents, Desktop and the home,
     converts with `iterm.rs` (translator, font lookup and ids injected) and applies the picked
     profiles, replacing the one with the same iTerm2 identity. `keys.rs` now also takes the
     shortcut overrides as an object (how the settings keep them): before, a shortcut changed
     in the settings was ignored by the native window. 17 crate tests (81 in all).
     Transport added: job `fonts.list` ([] → [{family, ps, mono, color}], asked once), and the
     `settings` effect above (stage 6 must run it through the settings store).
   - **4c (next):** what the JavaScript window did with the profile that is still missing:
     the trigger engine (`state.js` `evalTriggers`: regex on new lines, the nine actions, the
     captured-output section of the toolbelt) and saving/restoring window arrangements and the
     live layout (`saveArrangement`, `restoreArrangement`; `workspace.rs` already rebuilds them).
5. **tmux.** The control-mode client of `tmux.js` (`tmux -CC`).
6. **Registration and removal.** `manifest.rs`, `runtime/native_terminal_services.js`
   (the `term.*` calls Rust names, session lifetime in the window lease), removal of the
   JSX/JS, the real-shell runtime test.

## Resume here

Stages 1–4b are committed on `main` (not pushed; the owner pushes when everything is done). The
next step is 4c (trigger engine, arrangements), then 5 and 6. Test workflow: the Fedora WSL clone (`~/axia-check`), `cargo test -p
axia-app-terminal --locked --offline` from `rust/`; Windows cannot run cargo on this repo. Order
of the remaining apps after the terminal: `player`, `mail`, `editor`. Commits carry no
Co-Authored-By trailer.

## Notes for the next stages

- **Shared state.** `state.js` keeps every window's tabs at module level so minimizing a
  window (which unmounts its view) loses nothing. The Rust model must outlive a view the same
  way: one workspace per shell session (a `thread_local`, as Notes keeps its session), the view
  of a window reading and changing its entry.
- **Input is not a job.** `Jobs` runs one call and polls its result every 50 ms and the lease
  accepts 32 in flight; key bytes, mouse moves and wheel steps would pile up. Session input
  (`term.write/mouse/wheel/focus/scroll`) goes out as synchronous effects the mount executes
  at once (as the capture transport does), and only calls with an answer (`create`, `info`,
  `text`, `search`) are jobs. Mouse moves are coalesced by the model.
- **Events.** `term.onAny` (title, cwd, bell, badge, output, command, exit…) lives in the
  session today. The transport must keep listening while no window is mounted and hand the
  events to the Rust model, which updates titles and the attention marks.

- The window needs the keyboard before the shell does: `NativeApp::key` receives the
  keys the focused native window sees; Rust resolves them with `keys.rs` and the transport
  writes the bytes with `term.write` (the order today's `index.jsx` uses).
- A test session cannot rely on a login shell (the harness user has none, so the shell
  exits at once); a runtime test of the window needs a program that stays alive, or
  `term.create` with an explicit command (not available yet).
- `term.feed` writes to the screen only for tmux gateway sessions.
