# Axia apps

`apps/about/`, `apps/characters/`, `apps/clock/`, `apps/forcequit/`, `apps/files/`, `apps/calculator/`,
`apps/monitor/`, `apps/store/`, `apps/help/`, `apps/logs/`, `apps/fonts/`, `apps/scanner/`, `apps/recorder/`, `apps/backup/` and `apps/appabout/` are compiled Rust applications. Their folders
contain only `.rs` files: `mod.rs` owns the application, `ui.rs` its widget trees,
`tests.rs` its behavior and `manifest.rs` its metadata (some small apps keep
their UI in `mod.rs`). There are no app JSX or
JSON manifest sources and no generated per-app JSX wrappers.

`rust/native_apps.rs` is the shared registry. `cargo xtask native-apps` exports
its metadata during the theme build; the common host in `sdk/nativeapps.js`
binds compiled apps to the desktop's current renderer. About, About App, Force Quit, Calculator, Emoji & Symbols, Clock, Help, Logs, Fonts, Scanner, Recorder, Backups, Monitor and Store
mount their widgets and icons directly into the Rust scene through
`native_view/`; their content and input do not pass through React. Calculator
keeps its existing widget tree, colors, arithmetic and localized display, and
has no polling timer. The shell still supplies the window frame and theme.
About preserves the branding, status badges, hardware/server rows, regional
formatting and System Report action. Its shared shell bridge supplies service
facts, with the original 5 s system and 2 s server sampling periods; the app
contains no React code. Emoji & Symbols owns its tabs, multilingual search,
Unicode blocks, skin tones, recents and clipboard actions in Rust. The shared
Unicode data in `assets/emoji.json` also supplies the shell picker. Clock mounts
all four tabs in Rust and shares its Rust session engine with the menu bar and
notifications, so alarms, the timer and stopwatch keep running with the window closed. Files shares its Rust
service with the file dialogs; `sdk/rustfiles.js` transports those shared widgets
and desktop effects. About App and Force Quit now mount directly into the Rust scene too. The
former keeps app metadata, provenance, versions and window titles; the latter
keeps grouping, selection, disabled actions and errors. Native windows only send
a group key for termination: the shared Rust controller rechecks current shell
facts, then the compositor applies its Wayland credential checks. File and
letter icon descriptors preserve package and Linux-window artwork. The generic
React widget host for compiled apps has been removed; Files still uses its
separate browser/dialog renderer.

Help mounts its sidebar, search field, reading column, Markdown and six-step
welcome tour directly in Rust. `apps/help/content.rs` owns the translated
catalogs, English fallback, ranking, snippets and internal links; `markdown.rs`
keeps the article formatting and clickable navigation. Its remaining app-icon
artwork uses the shared Rust icon library too. `sdk/help.js` remains the shell's shared content API and
window-effects adapter; the Help window does not use it or the React Markdown
renderer. There are no Help JSX or JSON manifest sources.

Logs owns its entire window, filters, selection, virtual list, scrollbar,
detail view, local-log parser, query generations and export content in Rust.
`runtime/nativeapps.js` leases asynchronous SDK service jobs to each window;
the SDK still performs journal reads, admin-helper access, inotify following and
confined filesystem writes. Closing a window removes its subscriptions and
rejects publication of late results. Large responses cross the native JSON
boundary in bounded fragments. The Logs folder contains only Rust source.

Fonts now owns its entire native window, icon, family/style collections, search,
sample sizing, character blocks/chunks, glyph picking, metadata and install/remove
sequencing in Rust. The document helper still renders samples and grids. Window
leases retain those image handles until Rust releases them or the window closes;
late or cancelled images are disposed once. Installation waits for the file job's
success before reporting completion and rescanning. The app folder contains only
Rust files; the former JSX, JSON manifest and JS block table were removed.

Scanner owns device discovery/selection, capabilities, preview/crop geometry,
scan settings, localized file naming, conversion and save sequencing in Rust.
Existing SDK services perform eSCL requests, confined image/PDF decoding,
filesystem jobs and the system print dialog. The shared window lease retains
preview images and disposes late results. A direct save waits for the copy job
to finish before announcing success; conversion keeps the settings captured
when scanning started. The app folder contains only Rust source.

Voice Recorder now owns its native window, recording/playback state, microphone
selection, waveform raster, crop handles, rename and export policy in Rust.
The SDK still requests microphone permission and talks to PipeWire/axia-media;
the runtime transports raw events and retains sessions in the window lease.
Closing or discarding a late start releases the microphone/player and listener.
Rust also handles session completion, trimmed playback and delayed seek; active
media can keep its control cadence after focus loss. The original waveform is
covered by pixel fixtures, alongside a native-window export test and service
lifecycle tests. The Recorder folder contains only Rust source.

Backups owns its entire native window: status/progress, destination volumes and
folders, scheduling, exclusions, encryption/unlock forms, timeline, breadcrumbs,
snapshot contents and restore/delete confirmations. The session backup engine
and SDK notifications keep running independently of the window. Window leases
own status subscriptions; generation tags reject obsolete directory results.
Confirmations capture their snapshot and replacement policy. Native secure fields
mask Unicode text and use the shared Rust editor's password clipboard rules.
The folder contains only Rust source; frozen header cases, native-host flows and
SDK service ownership tests cover the migration.

## Remaining apps awaiting migration

The complete remaining scope is tracked in `docs/NATIVE-APP-MIGRATION.md`.

PDF's transforms, viewport navigation, zoom/input rules, search/selection lifecycle
annotation/form drafts and mutation ordering, raster-cache policy, thumbnail
layout/scrolling and bookmark expansion now live in
`apps/pdf/{model,geometry,navigation,viewer_model,edit_model,cache_model,sidebar_model}.rs`.
`sidebar_ui.rs` builds the sidebar's native widgets, mounted through the existing
`shell:tree` host; its JSX adapter only supplies the mount and helper/cache transport.
The main PDF window remains React. Its JS adapters keep helper promises and image handles on the
host side; each cache has a separate Rust model through `lord.nativeApps.create('pdf')`.
The cache preserves progressive previews, generations, LRU and release effects.
See `apps/pdf/README.md` for the frozen-reference tests and current migration boundary.

Each remaining app is a folder `apps/<id>/` with two files. Apps don't depend on the theme: any theme shows
them with its own colors (via `useTheme()`).

```
apps/quicknotes/
├── manifest.json
└── index.jsx
```

**manifest.json**

```json
{
  "id": "quicknotes",
  "version": "1.0.0",
  "name": "Quick Notes",
  "description": "Quick notes",
  "size": [500, 400],
  "dock": true,
  "permissions": []
}
```

`version` is required: the app's own version, in semver (`major.minor.patch`, optionally with a
`-beta.1` or `~rc1` suffix). The build refuses an app without one (`tools/build-theme.mjs`), and the
Store shows it next to the app (Installed: "LordOS app · 0.8.5"). The native apps started at `0.8.5`,
the LordOS version they first shipped in; from then on an app's version changes when the app changes,
apart from LordOS's, so the apps can later live in repositories of their own. An app added later starts
at the release it first ships in (`remote-desk`, 0.8.7).

Optional fields: `"transparent": true` (the theme paints no background under the content; the
app draws its own, e.g. the Terminal with transparency) and `"minSize": [width, height]`.

**index.jsx**

```jsx
import { View, Text, useTheme, TextInput } from '@lordos/sdk';
import { useState } from 'react';

export function Icon({ size }) {          // icon for the dock, app launcher, taskbar
  return <View style={{ width: size, height: size, borderRadius: size * 0.22, backgroundColor: '#ffd60a' }} />;
}

export default function QuickNotes() {    // the window's content
  const t = useTheme();
  const [text, setText] = useState('');
  return (
    <View style={{ flex: 1, padding: 16, gap: 10 }}>
      <Text style={{ fontSize: 18, fontWeight: 700, color: t.text }}>Quick Notes</Text>
      <TextInput value={text} onChange={setText} placeholder="Write…" />
    </View>
  );
}
```

(Real apps don't hard-code visible text like this: see "Texts and languages" below.)

`npm run build` rebundles every theme with every app. To distribute an app without touching the
themes, it is packaged as an `.lpk` with `node tools/lpk.mjs <folder>` and installed from the
Store (see `docs/NATIVE-APPS.md`).

## Available API (`@lordos/sdk`)

| Area | API |
|---|---|
| UI | `View`, `Text` (with `fontFamily: 'monospace'` for JetBrains Mono), `Image`, `ScrollView`, `TextInput` (with `onKeyDown`), `Button`, `Toggle`, `Segmented`, `Stepper`, `Slider`, `SidebarItem`, `SectionTitle`, `Terminal` |
| Theme | `useTheme()` → `text`, `textDim`, `accent`, `windowBg`, `sidebarBg`, `separator`, `hover`, `control`, `inputBg`, `mode` |
| Windows | `wm.open(app, { new, instance, frame, args })`, `wm.close(id)`, `wm.focus(id)`, `wm.setTitle(id, t)`, `wm.setMenus(id, menus)`, `wm.setCloseGuard(id, fn)`, `useAppWindow()` → `{ id, focused, args }` |
| Terminal | `term.create/close/configure/key/paste/search/...` (see `docs/TERMINAL.md`), `clipboard.get/set`, `clipboardHistory` |
| Keyboard | `useKeyboard(fn)` (only with the window in front), `TextInput`; `keyboard.list/configured/active/next/set` (XKB layouts) |
| Language | `t('namespace.key', vars)`, `useLocale()`, `appName(app)`, `formatDate`, `formatTime`, `formatNumber`, `languages()`, `setLanguage(id)`, `direction()`, `textDirection(text)` — see `docs/LANGUAGES.md` |
| Input methods | `TextInput` already uses them; for a custom field, `ime.attach(token, caret)` / `ime.detach(token)` and `useIme()` (the on-screen keyboard also learns about the field and its caret from these) |
| Administration | `admin.run('locale-gen' \| 'system-locale' \| 'timezone' \| 'langpack', arg, onLog)` → promise; `admin.busy` (progress) |
| Files | `useDirectory(path)`, `fs.list`, `fs.read(path, { binary })` (max. 8 MB), `fs.write(path, text \| ArrayBuffer)` (atomic, max. 32 MB), `fs.mkdir`, `fs.rename`, `fs.copy`/`fs.move` (in the background: `fs.jobs()`), `fs.trash`/`trashList`/`restore`/`removeFromTrash`/`emptyTrash` (freedesktop Trash), `fs.stat`, `fs.du`, `fs.free`, `fs.search`, `fs.recent`, `fs.volumes`, `fs.join`, `fs.parent`, `fs.basename` — only in the home folder and on the user's mounted disks (`/media/$USER`, `/run/media/$USER`); writing to `~/.local/share/applications`, `~/.config/autostart`, `~/.local/share/axia/apps` or the settings is not allowed, and deleting is only possible inside the Trash (see `docs/FILES.md`) |
| Fonts | `lord.fonts.list({ rescan })` → `{ family, style, ps, weight, italic, mono, color }` of the installed fonts |
| Touchpad | `onWheel` with `e.precise`, `e.deltaX/deltaY` in pixels and `e.phase` (`'momentum'` = inertia); `onPinch({ scale, rotation, x, y, phase })`; `onSwipe({ dx, dy, fingers, phase, system })`; `useTrackpads()`, `trackpad.config()/set(changes)`, `registerGestureActions({ up, down, left, right })` (themes) — see `docs/TRACKPAD.md` |
| System | `useSystemInfo()`, `useCpuUsage()`, `useServerStats()`, `formatBytes`, `formatDuration` |
| Settings | `useSettings()` → `[settings, update]`, stored in `~/.config/axia/settings.json` |
| Session | `session.quit()`, `themes.list()`, `themes.use(id)` |
| Apps | `useLauncherApps()` (LordOS, packages and Linux apps), `launchApp(id)`, `packages.*`, `windowIcon(w)` — see `docs/NATIVE-APPS.md` |
| Clipboard | `clipboard.get/set(text, { secure })/use/pin/remove/clear`, `clipboardHistory` and `clipboardKind` (saved history; ❖V and ❖⇧V panels) — see `docs/SPOTLIGHT.md` |
| Search | `spotlight.open/close/toggle` (❖Space); `spotlightCalc(text)` and `spotlightConvert(text)` |
| Sound | `useSound()`, `sound.setVolume/setMute/setDefault/setPort/move/meter/play`, `defaultSink(s)`, `microphones(s)` — see `docs/SOUND.md` |
| WebAssembly | the standard `WebAssembly` global (`docs/WASM.md`); in installed apps, `loadWasm(id, path)` |
| Canvas | `<Image pixels={{ width, height, data }} />`: RGBA pixels (for example, computed in WASM) |
| Media | `media.play/image/thumb/probe/poster/edit/transcode/screenshot/annotate/saveImage/copyImage/screenRecord/audioRecord/camera/photo/cameraRecord`, `usePlayer(id)`, `<MediaView media={id} />`, `nowPlaying`, `NowPlayingTile`, `useThumb(path)`, `userDirs()` — decoded by `axia-media`, sandboxed (see `docs/MEDIA.md`) |
| Windows (media) | `wm.setProtected(id, on)` (black in screenshots and recordings), `wm.setFullscreen(id, on)`, `wm.setPip(id, frame)`, `useWindowState(key, initial)`, `useAppWindow().mode` (`'window'`, `'fullscreen'` or `'pip'`) |
| Markdown | `<Markdown source base width onLink onToggleTask />`, `markdownHeadings(text)`, `toggleTask` — see `docs/NOTES.md` |
| Desktop | `registerWidgetType(type, Component)`, `setWidgets(type, [{ id, x, y, w, h, pinned }])`: widgets between the wallpaper and the windows |
| Notifications (M9) | `notify({ app, title, body, actions: [{ id, label, primary, run }], sound, sticky, timeout, onClose })` → id, `dismissNotification(id)`, `useNotifications()`, `notifyHistory`; `registerStatusItem(id, Component)` (status icons in the theme's bar) — see `docs/SYSTEM.md` |
| Clock and Help (M9) | `clockAlarms`, `clockTimer`/`timerStart…`, `stopwatch`/`swStart…` (keep running with the app closed); `lord.time.zoneOffset(zone, t)`; `help.list/read/search/open/startTour`; `registerSpotlightSource(id, fn)`; `weekdayName(d)` |
| Documents (M9) | `pdf.open(path \| { blob })` (page, search, outline, links, forms, annotations, save a copy), `<DocImage image>`, `archive.list/extract/create`, `fontInfo/fontSample/fontGrid`, `pdfThumb`, `makeTextPdf`/`makeImagePdf`: all in the sandboxed helper `axia-doc` — see `docs/DOCUMENTS.md` |
| Printing (M9) | `print.open({ title, path \| doc \| text \| image })` opens the system dialog; `print.printers/jobs/cancel/add/remove/setDefault/discover/scanCaps/scan` — see `docs/PRINTING.md` |
| Emoji (M9) | `emoji.open()` (❖.), `emoji.search/name/withTone/insert`, `EmojiGrid`, `EmojiToneBar` |
| Custom text focus | `takeTextFocus(token, onKey, onChange)` / `releaseTextFocus(token)`: a custom field receives keys like a TextInput |

## Texts and languages

Visible texts aren't written in the code: they go through `t('myapp.key')` with their
translations in `assets/locales/{es,en,pt}/myapp.json` (`{ "key": "Text" }`). The app's name is
translated in `apps.json` (`"myapp": "My app"`). Lists containing texts are built at render
time, not as module constants, because the UI is remounted when the language changes.
`node tools/i18n-check.mjs` warns if any key is missing.

## Permissions

For an app built into the theme, `permissions` in the manifest is **declarative**: it documents what
the app uses, and the app runs in the same JS context as the theme. Apps with `"process": "isolated"`
and installed `.lpk` apps run as their own process, with permissions checked by the server
(`docs/NATIVE-APPS.md`, `docs/PRIVACY.md`). For all of them the server runs without root and there is
no API to run commands (the terminal can only open the user's shell).

`fs` only writes inside the home folder and the user's mounts and never to the folders the system uses to launch or
install things (`.desktop`, autostart, installed apps), so an app can't turn a write into
running a command.
