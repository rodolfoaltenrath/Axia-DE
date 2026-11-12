# Axia Store

The Axia Store (`apps/store/*.rs`, app id `store`) is where people discover, open
and manage apps. It replaces the former React Store (`apps/store/index.jsx`), whose
Debian package pages had stopped working once the distribution's package engine
was removed (see [PACKAGES.md](PACKAGES.md) for that history). Its name is "Axia
Store", not "App Store", which is a trademark of Apple.

## Language boundary

The store is a **native view** ([RUST.md](RUST.md), "Visões nativas"): Rust builds
its widget trees and the native view host mounts them into the scene; React draws
no part of it. The window keeps the window manager's frame and buttons, like every
other app. The shell runs only the effects that belong to it, from
`sdk/nativeview.js`:

- package calls on `lord.apps` (`packages`, `inspectPackage`, `installPackage`,
  `removePackage`) and signature calls on `lord.sign` (`status`, `restore`,
  `userKey`, `share`), allowed one by one in the manifest (`reads`, `calls`);
- `launchApp` to open an app, `refreshPackages` after a package changes, and the
  window title.

Answers come back in order (`Tag` in `mod.rs`): a package list asks for each
package's signature status; installing, removing or restoring refreshes the shell's
list and reads the packages again.

## Layout

- **Sidebar**: translucent (`backdropFilter: blur(30px)` over a see-through fill),
  with the search field and Discover, Create, Work, Play, Develop, Categories,
  Updates (a count badge when there are updates) and Installed. Icons are
  antialiased line glyphs drawn in Rust in the accent colour (`art.rs`).
- **Content**: a large title; two editorial cards per page (a blue uppercase tag,
  title, subtitle and artwork); sections with a title and "See All"; app cards in
  three columns (two or one in narrower windows): icon, name, summary and a pill
  button.
- **Pages**: Discover (Axia essentials, then a section per group), Create / Work /
  Play / Develop (by freedesktop categories), Categories (coloured tiles with
  counts) and a category's apps, Updates, Installed (Axia packages with their
  signature, "modified by you", restore and share; `.lpk` files found in the home
  folder, Downloads and the Desktop; all apps), an app page, search results, and
  the confirmation before installing a package file (opened from Files with
  `args.install`).
- Light and dark follow the system theme. No accounts or prices: every app is
  free, so the pill says **Open** for an installed app (and **Get** is reserved for
  the catalog).

## Content until the catalog exists

The Axia Store's own catalog (descriptions, screenshots, artwork, updates) comes
later. Until then it lists the apps on this computer: the shell's launcher apps,
with the categories and PNG icons of their `.desktop` files
(`applications::desktop_facts`). Native views draw their own icons (Monitor, the
store); other apps without a PNG show their initial on a colour from the name, as
`LetterIcon` does. The editorial cards' artwork is generated from the story's name
(`art::hero`): it stays the same between frames and is drawn once.

## Tests

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline --lib store::
make -C server
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust \
  --locked --offline --test native_apps_runtime -- --ignored --nocapture
```

The unit tests cover the catalog (categories, icons, sources, order), pages and
history, search, launching, the package and signature flows with their answers in
order, installing a file (only the one on screen; failures keep the page with the
shell's error), the glyphs and artwork, and rasters drawn once. The integration
opens the store in a real session and checks the sidebar, cards, title and
Categories page.
