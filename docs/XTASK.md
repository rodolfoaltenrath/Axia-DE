# Development tools: `cargo xtask`

The scripts that used to live in `tools/` (Node, Python and shell) are subcommands of the `xtask`
crate (`rust/xtask`, the commands in `rust/xtask/src/devtools/`). They run on the development or
build machine only and are never installed with the desktop. Run them from the repository root;
`cargo xtask` is an alias for `cargo run --quiet -p xtask --locked --offline --`.

Programs the tools drive stay external and are only called: `esbuild` (`npm ci`), `ffmpeg` and
`cjpeg`, `rsvg-convert`, `pyftsubset` (fontTools), `copr-cli`, `nm`, `make`, `curl`.

## Checks

| Command | What it does |
|---|---|
| `i18n-check [LANG\|pack.json]` | language catalogs against English, the `t('…')` keys the code uses; with no argument also `err-check` and `brand-check` |
| `err-check` | the server reports errors and statuses as codes, never as prose |
| `brand-check` | third-party brand names stay out of the public text |
| `desktop-check [--require-gpu]` | stages the installation and runs the private headless sessions of the integration fixtures (`make check`, the RPM `%check`) |
| `clipboard-x11-check` | clipboard transfers through XWayland, with an independent Xlib peer |
| `dock-bench` | cost of mouse moves over the Dock in a private session |
| `network-tray-check` | the NetworkManager, brightness and tray contracts against fake services on a private session bus |

## Build and packaging

| Command | What it does |
|---|---|
| `build-theme [theme\|app-host\|sysapps …]` | bundles the themes, the app host, the isolated system apps and the test bundles with esbuild, then writes `code-manifest.json` (`npm run build` calls it) |
| `code-manifest [--root DIR] [--check]` | the size and SHA-256 of every file the desktop runs as its own code |
| `sign-code [ROOT [KEY]]`, `sign-release DIR [KEY]` | Ed25519 signatures with the release key (no openssl needed) |
| `lpk FOLDER`, `lpk keygen\|sign\|verify\|catalog` | packages and signs `.lpk` apps and the app catalog |
| `copr-publish [--build-id N]` | submits the checked SRPM to Copr and watches the build, tolerating short outages |
| `install`, `dist`, `srpm`, `deploy`, … | the packaging commands that were already here |

## Generated data (committed; regenerate by hand)

| Command | Output |
|---|---|
| `gen-wallpaper --palette P --output F` | the four gradient wallpapers |
| `gen-wallpapers SOURCE-DIR` | the JPEG collection and `assets/wallpapers/index.json` |
| `gen-brand [--svg-only]` | the logo SVGs and their PNG renders |
| `gen-controls-font` | the window-controls COLR font of the VS Code icon theme |
| `gen-emoji FOLDER` | `assets/emoji.json`, `sdk/emojidata.js`, the per-language keyword files |
| `gen-unicode UCD-FOLDER` | `server/src/ui/unidata.h` |
| `gen-monitor-data GEOJSON CSV` | the Monitor app's outlines, GeoIP database, labels and country names |
| `gen-lbfonts [--fonts DIR] [--ucd DIR]` | the LordBench font subsets |

## Test machines

| Command | What it does |
|---|---|
| `collect-results MACHINE …` | the evidence of one machine and one version (`results/<version>/<machine>/`, docs/RESULTS.md) |
| `qmp-input`, `vm-type`, `vm-record` | mouse, keyboard and screen recording of a Proxmox VM, from its host |
| `keylag [SECONDS]`, `keylag-inject`, `keylag-client` | where the typing delay comes from, and the uinput harness (docs/TROUBLESHOOTING.md) |

## Not migrated yet

Still in `tools/`: `fedora-live/` (the live ISO remix and the installer apps that run inside the ISO) and
`hwreport/` (it reads the JavaScript helpers of the System Information app, which are still JavaScript).
