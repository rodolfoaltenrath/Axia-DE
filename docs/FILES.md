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
- `udisks.rs` implements bounded, asynchronous mount/unmount/safe removal over
  the system UDisks2 D-Bus service, with polkit authorization. It unmounts all
  mounted partitions before powering off a removable drive, never forces a busy
  unmount and never powers off an internal drive. No LordOS root helper or shell
  command is used. `volumes.rs` maps native device facts into the sidebar.
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

## Window design

The browser keeps the theme's existing window frame, rounded corners and traffic
light controls. Rust supplies the content through the same Axia toolbar, controls,
fonts and theme colors: a compact toolbar, neutral sidebar selection, folder path
bar and a tab strip shown only with multiple tabs. Narrow windows use one view
switcher; all four modes remain available in the View menu. Long tab strips keep
the active tab visible and provide buttons to reach the remaining tabs.

File choosers use this same content inside a modal sheet; they keep their own
Open/Save and Cancel actions. Information, previews and confirmations stay within
the owning window, with scrollable content and actions accessible after resizing.
Rust response effects are queued independently of React rendering, so batched
snapshots cannot drop an open, preview, mount or resource-release operation.
Visible file rows, keyboard navigation and scrollbar movement share one Rust
layout calculation, including gallery previews, search, errors and tab height.
Ancestor columns are virtualized too, so large folders remain fully reachable.

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
- The sidebar follows the native udev/mountinfo notifications. USB drives and
  secondary disks appear without reopening Files. Click a recognized unmounted
  file system to mount it through Fedora's UDisks2 service in a Rust worker;
  the existing Fedora polkit agent handles authorization. Eject safely with
  the sidebar control. Unformatted/locked/unsupported devices open Disks for
  management. Formatting is never inferred from clicking a device. An internal
  disk can require administrator authorization. System disks are excluded.
- Mounted secondary disks outside the usual `/media` roots become accessible
  through validated native disk facts; roots are revoked after removal. All tabs
  and windows leave removed volumes and prune those entries from history.
- PDF icons show their first page using the existing confined document helper.
  Rust queues at most two jobs per window and retains 32 small 128×176 images
  (at most 2.75 MiB of pixels); errors fall back to the document icon. Eviction,
  changed files, closing previews and closing a window release document handles.
- Folder addresses accept absolute paths, `~`, paths relative to the current
  folder and local `file://` URLs. Network URLs use the connection flow.
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
size calculation and thumbnails run on bounded workers. One image
thumbnail decoder runs at a time, with a 64 MiB decoded-pixel limit and a
32 MiB encoded-file limit. Common 12-megapixel RGB photographs fit this budget.
PNG, JPEG, WebP, GIF and BMP thumbnails preserve orientation and never enlarge
a small source. Other formats, oversized sources and damaged images retain an
ordinary file icon. A FIFO or special file cannot stall the decoder.
The thumbnail disk cache is bounded to 128 MiB and memory metadata to 1,024
entries. Full-size images are not decoded by the UI while scrolling.

Each browser owns the results of its own copy/move jobs. Another window cannot
consume those results or forget jobs owned by a C consumer such as backup.
Closing a window lets copies finish and then reclaims their job records.

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
The headless Files fixture exercises actual PDF rendering/binding/release and
hotplug/mount/eject across React, QuickJS and Rust. Disk facts and privileged
volume transport replies are simulated: it never mounts, ejects or formats a host device.
Rust regression tests cover resize cycles, mouse/keyboard selection, folder
creation/rename conflicts, links, address/history navigation and removal in
multiple tabs/windows. Headless tests do not validate physical hotplug/input,
DRM/KMS or remote servers; those still require hardware/session testing.

To verify the typed UDisks2 connection against an existing device without changing
it, run the ignored `files::udisks::integration::read_only_system_bus_probe` test
with `AXIA_UDISKS_PROBE` set to a kernel device name. That test only reads the
Block.Drive property and never mounts, unmounts, powers off or formats a device.

A manual Fedora 44 hardware check on 2026-11-03 exercised a USB drive containing
an ISO9660 installation image with the newly built Files UI in a private headless
session. Native device notifications, UDisks2 mount, Rust directory listing,
safe whole-drive removal and return to Home all passed, without writing to the
device. Physical FAT/exFAT/NTFS disks and an additional internal disk still need
separate hardware checks; simulated facts cover those sidebar/control paths.
Real pointer/keyboard probes also exercised PDF tiles, rename collision feedback,
maximizing/restoring and manual resizing between 640×400 and larger windows.
