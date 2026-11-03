# Arquivos

The launcher name remains **Arquivos** in Portuguese (**Files** in English).
Its description is “Axia file explorer”. The app and native open/save/folder
choosers share one Rust implementation in `rust/axia-shell/src/files/`.

## Architecture

- `model.rs` owns each window's tabs, location/history, selection, search,
  clipboard actions, preferences and auxiliary dialogs. A browser and its file
  chooser have separate handles and cannot change each other's selection.
- `ui.rs` describes the complete interface as compositor widgets: sidebar,
  toolbars, file views, menus, information, previews and confirmations.
- `operations.rs`, `policy.rs` and `trash.rs` implement copy/move/rename,
  permissions, bounded directory walks and freedesktop Trash. `compat.rs`
  retains `fsops.h`'s C ABI for existing desktop consumers (including Cleaner
  and backup). The previous `server/src/fsops.c` implementation was removed.
- `thumbnails.rs` generates thumbnails lazily, outside the interface thread.
- `network.rs` uses Fedora's GIO/GVfs and a private worker context to connect
  to network folders. Authentication and trust questions use the same Rust
  dialog system. Passwords are not put in command arguments or preferences.
- `server/src/ui/js_files.c` transports JSON between QuickJS and Rust.
  `sdk/rustfiles.jsx` binds widget descriptions to the existing renderer and
  carries out desktop service effects (opening apps, media/document/archive
  helpers). These adapters contain no file navigation or selection model.

`apps/files/index.jsx` registers the native app. `sdk/filepicker.jsx` preserves
callers' existing component API. Notes, Editor, Contacts, Settings, Mail and
Passwords use this shared chooser. Isolated apps retain their descriptor
broker: they receive the selected file descriptor, never the desktop's
filesystem API. `sdk/powerbox.jsx` only queues requests and returns results.

External GTK/Qt applications may still provide their own dialogs. This change
is not an `org.freedesktop.portal.FileChooser` backend, and does not replace
third-party application code.

## Everyday use

- Home, Downloads, Documents, Desktop, Pictures, Music and Videos follow
  `user-dirs.dirs`, including translated/custom Fedora folder names. Setting
  an XDG folder to HOME disables its shortcut; missing configured folders
  are not silently substituted.
- Independent tabs, back/forward/up, direct folder navigation, favorites,
  hidden files, natural sorting and per-folder view preferences.
- Icons, list, Miller columns and gallery views. Large directories materialize
  only visible rows plus an overscan buffer, rather than thousands of widgets.
- Search filters the current listing while typing; Enter searches recursively.
  The scope button switches between the current folder and Home. Traversals
  have depth, entry and time limits.
- Copy/cut/paste, duplication, rename, new folder and drag/drop. Drag normally
  moves; Alt copies. A destination that exists is never overwritten by a copy
  or move. Save dialogs separately ask before choosing an existing target.
- Trash/restore/permanent deletion, information/comments/color tags,
  bounded text/image previews, document/media previews and archive browsing,
  extraction and creation through the existing confined helpers.
- Network folders use Fedora's GVfs backends. Install the appropriate backend
  for the protocol, such as `gvfs-smb` or `gvfs-nfs`; `gvfs-fuse` exposes mounted
  folders to ordinary filesystem operations. Web/remote desktop addresses are
  handed to the existing desktop application integration.

Keyboard controls accept both Ctrl and Super/Command for common actions:
A/C/X/V, T/W/N, Shift+N (new folder), I (information), D (duplicate),
Shift+G (go to folder), K (connect), and period (hidden files). Alt+arrows
navigate history/up. Arrow/Page/Home/End keys navigate files; F2 renames,
F5 refreshes, Delete uses Trash, Space previews, Enter opens/chooses and
Escape closes the current dialog or cancels a chooser.

## Resource and data protection

The interface polls lightweight snapshots; unchanged windows return `null`.
Inotify refreshes local directory changes. Listing, recursive search, copy,
size calculation and thumbnails run on bounded workers. At most two image
thumbnail decoders run concurrently, with a 32 MiB allocation limit each.
The thumbnail disk cache is bounded to 128 MiB and memory metadata to 1,024
entries. Full-size images are not decoded by the UI while scrolling.

Copying writes a uniquely named staging entry and publishes it with
`renameat2(RENAME_NOREPLACE)`. Failed/cancelled copies clean up their staging
entry and preserve an existing destination. Recursive operations use open
directory handles and do not follow symlink entries. Moves never strip a
protected directory's contents by renaming an ancestor. Trash restore also
refuses an existing destination.

Preferences migrate from the legacy `files` settings into
`~/.config/axia/files.json` on first use. Tags and comments keep their existing
locations under `~/.local/share/axia/`. Preferences are written atomically;
replacing the implementation does not move the person's documents.

## Validation

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline
make -C server build/fsops-test
server/build/fsops-test
make check
python3 -m unittest discover -s tools/desktop/tests -v
```

The Rust tests use private temporary homes and cover XDG directories,
independent windows/tabs, open/save/folder modes, selection/filtering,
confirmation, folder watches, search, virtualization, protected paths,
non-UTF8 filenames in filesystem operations, copy conflicts, symlinks and
Trash. The existing C regression tests exercise the unchanged consumer ABI.
Headless tests do not validate physical input, DRM/KMS or remote servers.
