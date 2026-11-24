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
2. **Window base.** Windows, tabs, the pane tree, sessions and the status bar, with the
   state of `state.js` as a Rust model.
3. **Bars and dialogs.** Search, toolbelt, context menu, replay, autocomplete, paste and
   command history, the Dock menu.
4. **Preferences.** Profiles, schemes and key bindings; the importer of `iterm.js`
   (JSON profiles, `.itermcolors`, the iTerm2 plist) in Rust.
5. **tmux.** The control-mode client of `tmux.js` (`tmux -CC`).
6. **Registration and removal.** `manifest.rs`, `runtime/native_terminal_services.js`
   (the `term.*` calls Rust names, session lifetime in the window lease), removal of the
   JSX/JS, the real-shell runtime test.

## Notes for the next stages

- The window needs the keyboard before the shell does: `NativeApp::key` receives the
  keys the focused native window sees; Rust resolves them with `keys.rs` and the transport
  writes the bytes with `term.write` (the order today's `index.jsx` uses).
- A test session cannot rely on a login shell (the harness user has none, so the shell
  exits at once); a runtime test of the window needs a program that stays alive, or
  `term.create` with an explicit command (not available yet).
- `term.feed` writes to the screen only for tmux gateway sessions.
