# Search and the advanced clipboard

Both are mounted by the SDK, so **every theme has them** without doing anything:
`startShell` adds `<SpotlightPanel />` and `<ClipboardPanel />`. Search is owned by
`rust/axia-shell/src/search/`; `sdk/rustsearch.jsx` draws its widgets and `sdk/search.js`
connects shared shell services. Clipboard code remains in `sdk/clipboard.js`.

## Search

| | |
|---|---|
| ![apps](search-apps.png) | ![files](search-files.png) |
| «term»: apps first, then folders | «budget»: a file with a text preview; Enter opens it in the Editor |
| ![calc](search-calculator.png) | ![store](search-store.png) |
| «15% of 2400» → 360 (Enter copies it) | Historical LordOS Store integration; unavailable in the standalone Fedora build |
| ![notes](search-notes.png) | |
| «apples» finds the note by its content | |

It opens with **❖Space** or **Ctrl+Space**, or with the ⌕ magnifier in the Lord Flow theme's
menu bar. With the Terminal in front, Ctrl+Space is left to the program (in emacs it sets a
mark); ❖Space works the same.

**Shortcut change:** ❖Space used to switch the keyboard or input method. Now that is
**❖⌥Space** or **Ctrl+⌥Space** (`sdk/keyboard.js`).

| Key | What it does |
|---|---|
| ↑ ↓ | choose |
| Enter | open (in the calculator and the clipboard: copy) |
| ❖Enter | show in Files (files and folders) |
| Tab | go to the next section |
| Esc | close; focus returns to the text field that had it before |

### What it searches

Sections are ordered by their best result; the calculator always goes on top.

| Section | Source | On open |
|---|---|---|
| Calculator | expressions (`2*(3+4)`, `2^10`, `sqrt(2)`, `pi*2`, `15% de 2400`, decimal comma) with our own parser, no `eval`; conversions (`10 km a mi`, `72 f a c`, `1,5 gb a mb`, `5 lb to kg`, `3 h en min`) | copies the result |
| Apps | `useLauncherApps()`: LordOS, `.lpk` packages and Linux apps | `launchApp` |
| Settings | the Settings sections and their translated keywords (`settings.keywords.*`) | `wm.open('settings', { args: { section } })` |
| Actions | dark or light mode; log out | the action |
| Files | names in the home folder, indexed on a Rust worker (up to 6 levels, no hidden files or symlink traversal) | folders in Files, text in the Editor, the rest in Files with the file selected |
| Notes | `~/Notes/*/nota.md` (title from the Notes app's YAML header, and the content); the section only appears if the Notes app is present | `wm.open('notes', { args: { slug } })` |
| Clipboard | bounded mirror of text history, released after the last search client closes | copies again |
| Help | localized Help articles read by the Rust worker (see `SYSTEM.md`, "Help") | the article in Help |

**Relevance:**
- For the name: prefix > word > substring > fuzzy (the letters in order).
- The description and the keywords only count if the text really appears, and they weigh less.
- Files are compared without the extension.
- What is opened often moves up, and usage loses weight after 30 days. It is stored in
  `~/.local/share/axia/spotlight.json` (the last 300 entries).

### Resident Rust search

The search core shares the desktop process. Opening a panel creates a small client;
closing it drops its results, selection, preview and clipboard mirror. There is no
separate search daemon. The single I/O thread starts on first use and sleeps on a
condition variable when idle. The UI only polls while work is outstanding; closing
the last client cancels polling. Typing, ranking apps, calculations, keyboard selection
and widget geometry are Rust operations. Filesystem scans and previews never run on
the UI thread. Superseded queries and previews carry cancellation generations, so
late replies cannot replace the current query or selection.

Resource limits:

- Eight simultaneous search clients; sixteen replaceable worker jobs/replies.
- File index streamed from `$XDG_CACHE_HOME/axia/search/files.jsonl` (fallback
  `~/.cache/axia/search`): at most 16 MiB, 100,000 entries, six levels and a three-second
  scan budget. The index is not retained as an in-memory filename collection.
  Inotify watches up to 512 directories and invalidates on the next query; the maximum
  age is 120 seconds, or ten seconds when the watch limit is reached.
- Six app/file results, four settings/clipboard/help results and five notes.
  Notes scan at most 400 directories and read at most 64 KiB per note; Help reads at
  most 500 articles per language, each at most 256 KiB.
- Text preview reads at most 9,600 bytes and displays at most 2,400 characters.
  Directory previews keep twelve entries from at most 10,000 examined entries.
- Images are decoded on the worker from regular files up to 32 MiB, with dimension
  and decoder allocation limits (10,000 pixels per axis, 64 MiB). This is a decoder
  budget, not a hard process RSS ceiling. Only thumbnails up to 232 × 160 pixels reach
  the renderer. Changing selection/closing removes temporary thumbnails and releases
  their reference-counted native pixels.
- Clipboard transport copies at most 500 texts and one million UTF-16 code units;
  the native mirror accepts at most 2 MiB of original UTF-8 text. Very large histories
  can therefore be only partially searchable. Copying a result uses the original
  clipboard entry. The mirror is discarded on last close.
- Usage history retains at most 300 keys and 64 KiB of key text; writes remain atomic.

`sdk/spotlight.js` has been physically removed. The active JavaScript adapters retain
rendering, localization and callable app/service objects; they contain no duplicate
calculator, ranking engine, filesystem index or panel layout. Shared controls and
the C compositor remain active parts of the DE.

Extensions can still use `registerSpotlightSource(id, fn)`, returning a disposer.
Provider callbacks are deferred until after the immediate input update; they should
remain fast, since JavaScript callbacks still run on the UI thread. Built-in Help
does not use such a callback. The Classic menu uses `searchEverything.prepare()` to
acquire an explicit lease, passes it to `results(query, lease)` and calls
`lease.close()` on unmount; its async results share the same Rust search core.

After building the desktop and installing the locked frontend dependencies:

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline search::
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust --locked --offline --test search_runtime -- --ignored --nocapture
```

The integration test is orchestrated and asserted in Rust. Its temporary embedded
module drives the existing React/QuickJS UI. It checks the real shortcut and button,
Enter/reveal, calculator/conversion, apps/files/images/notes/help/clipboard/providers,
160 open/search/close cycles, released native pixels and absence of closed-panel
polling. The visible text and geometry are checked against fixed expectations from
the validated UI, without loading the removed engine. `AXIA_SEARCH_OBSERVATIONS`
saves JSON; calculator and conversion cases also have native Rust unit tests.
The `activeBytes` diagnostic measures serialized active result/query/preview payload,
not allocator usage; process RSS and managed image bytes are checked separately.
Headless timing measures key dispatch to a native text update, not presented-frame
latency. Appearance, blur, typing and RAM still need validation on the Fedora machine.

**Preview (on the right):**
- the app's icon;
- the image;
- the first lines of a text file;
- the contents of a folder;
- the note;
- the calculator's result, large.

## Advanced clipboard

| | |
|---|---|
| ![full](clipboard-full-panel.png) | ![quick](clipboard-quick-panel.png) |
| ❖⇧V: filters with counters, color swatch, code in monospace, image preview and actions | ❖V: the latest items, after restarting axia-server (the pinned link is on top) |
| ![paste](clipboard-paste.png) | ![settings](clipboard-settings.png) |
| Chosen with ❖V and the 8 key: «360» was pasted into the Settings search box, which had focus | Settings → Clipboard |

There is a single one for the whole session: the Terminal, LordOS's text fields and Linux apps
(the compositor acts as the bridge; see `WAYLAND.md`), and since 0.8.3 X11 apps through Xwayland
(the X window manager, `wl/xwm.c`, offers LordOS's clipboard to X only while an X window has the
keyboard). The history stores everything copied in
any of them: texts, links, colors, code, paths and images. For each entry it stores the origin
and the time.

- **❖V, quick panel:** the latest items, with search. Enter, click or 1–9 choose.
- **❖⇧V, full panel:**
  - filters: All, Text, Links, Images, Code, Colors, Files, Pinned;
  - preview, origin, date and length;
  - Paste, Pin and Delete; Del deletes; «Delete all» keeps the pinned items.
- **Paste.** Choosing something copies it again and pastes it where you were:
  - into the LordOS text field that had focus;
  - or into the frontmost Linux app, with a Ctrl+V that the compositor sends only to that app
    (`lord.wayland.paste`).

  If there was nowhere to paste, it stays copied and says so.
- **Kind of each entry** (`clipboardKind`):
  - link: `http(s)://`, `www.`, `mailto:`;
  - color: `#rgb`, `#rrggbb`, `rgb()`, `hsl()`;
  - file: an absolute path or `~/…`;
  - code: several lines with code syntax or indentation;
  - image or text.

### Where it is stored

- **Files:**
  - `~/.local/share/axia/clipboard/history.json`, with the entries;
  - `img-*.png` next to it, with the images.
- **How images are stored:** the compositor leaves the latest image in
  `$XDG_RUNTIME_DIR/lordos/clip-N.png`, which is lost on reboot. That's why
  `lord.clipboard.saveImage(path)` copies it to the home folder. It validates the path with the
  same rules as `lord.fs.write`: inside the home folder and never in the protected folders.
- **Copying an image again:** `lord.clipboard.useImage(path)` only accepts those two places, the
  compositor's previews and the history's images.
- **Deleting:** `lord.clipboard.deleteSaved(name)` deletes only `img-*.png` names in that folder.
  There is no general file deletion.
- **Settings → Clipboard:**

  | Option | Default |
  |---|---|
  | Save the history or not (if off, it stays in memory only) | on |
  | Maximum number of items | 500 |
  | Space for images | 200 MB |
  | Delete old items (pinned ones don't expire) | after 30 days |
  | Clear on log out | off |
  | Don't save what certain apps copy (by their Wayland app_id) | none |

  They are stored in `settings.clipboard`.

### Privacy

Sensitive content never enters the history, but it can still be pasted:

- **Linux apps that mark the copy as secret.** They offer the `x-kde-passwordManagerHint`
  format, the KDE and GNOME convention used by KeePassXC and other password managers. The
  compositor detects it (`wl.c`), and `lord.clipboard.state()` answers `sensitive: true`.
- **LordOS.** `clipboard.set(text, { secure: true })` copies without saving. `TextInput`'s
  `secure` fields (passwords) don't allow copying.
- **Apps excluded in Settings.** They don't end up in the history.

`lord.clipboard.state()` also returns `origin`: the app_id of the Linux app that copied,
`terminal` or `lordos`.

### Tested (VM 119 `lordos-spot`)

- **Copies from a Wayland app** with `wl-copy` (`wl-clipboard`, for the tests only): a link, a
  color, a path, code, text and two PNG images. Each one got its kind, and the images were saved
  as `img-*.png`.
- **Secret:** `tests/clip_secret.c` offers text + `x-kde-passwordManagerHint`, like KeePassXC.
  The text could be pasted with `wl-paste` and didn't end up in `history.json`.
- **Restart:** after restarting axia-server, the history was still there, with the pinned items
  on top.
- **Full panel:** filters, pin and delete work.
- **Paste:** ❖V and the 8 key pasted into a LordOS text field.
- **Search:** apps, settings, files (which opened in the Editor), notes, the Store, the
  calculator and the conversions.

### Pending

- Pasting into a Linux app was only tested through code: a screenshot with a GTK app in front is
  still missing.
- Formats that aren't text or PNG (HTML, `text/uri-list` with files): today they pass between
  Linux apps while the source app stays open, but they don't go into the history.
- Search: searching file contents (today, names only), contacts and mail (the apps exist since
  M9, but they are not search sources yet), and a «Lock screen» action (locking exists, ❖L, but
  Search only offers dark/light mode and log out).
- Known issue in 0.8.5: searching "displays" lists the Help article on touchpads and touch screens
  before Settings → Displays.
