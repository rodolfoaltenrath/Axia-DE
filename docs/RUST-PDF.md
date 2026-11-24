# PDF viewer migration

The main window remains React. The sidebar's widgets and input mount through
the native Rust host. `geometry.rs` owns page/view transforms,
rotated sizes, continuous layout and visible-page ranges. `cache_model.rs` owns
width quantization, render admission/deduplication, generations, progressive
preview selection, byte accounting and LRU eviction. `model.rs` dispatches them
through the existing native-app bridge as `pdf`.

`navigation.rs` retains each layout's viewport and computes visible page frames,
prefetch priorities, reading position, scroll limits, page jumps and hit testing.
`viewer_model.rs` owns fit/zoom/rotation rules, keyboard dispatch, wheel throttling,
search results and selection request generations. `viewer.js` carries helper
requests and publishes accepted results; `index.jsx` applies the returned actions.
Restarting an identical search, clearing selection, changing documents and closing
the window invalidate older results. Empty search parts never navigate to a page.

`edit_model.rs` owns ink/note/form drafts, sampling, annotation payload validation,
dirty state and the per-document queue for annotations, field writes, undo and
saving a copy. Only one mutation is sent to the helper at a time. Undo selects
its page when it reaches the queue head; save waits for preceding edits. Failed
requests retain the prior history and allow the queue to continue.

The editor emits ink deltas rather than full stroke arrays on every move. Text
and choice fields reuse pending/confirmed values until fresh page metadata arrives;
rapid checkbox/radio actions alternate against the latest native intent. Metadata
reads carry page revisions and document epochs, so an old read cannot replace
new field state. Highlights consume only the final native selection ticket.

`geometry.js` retains a native owner per layout in a WeakMap: normal scrolling
sends only its two viewport limits. `sidebar_model.rs` retains thumbnail sizes,
positions, scroll limits, page following and bookmark expansion. Thumbnail
scrolling uses binary search and returns only visible frames; sizes are supplied
once per page-size/rotation change. The model flattens visible bookmark rows and
preserves descendant expansion choices when a parent collapses. Its JS adapter
keeps helper I/O on the host, and the sidebar adapter ignores late outline replies
after a document change. Resizing clamps the thumbnail offset to the new extent.
`cache.js` retains helper promises, callbacks and
raster handles, and applies Rust's release effects. Image pixels, promises and
JS object references stay on the host. Models and their pending metadata are
independent per owner.

`sidebar_ui.rs` builds the complete sidebar tree: themed tabs, thumbnail frames,
page numbers, outline disclosure controls, titles and localized loading/empty
states. `sidebar.jsx` now only mounts it with the existing `shell:tree` native
view, transports events and performs helper/cache requests. Visible thumbnails
borrow numeric image IDs through `documentImage`; the cache retains ownership.
The native host owns outline scrolling, including its scrollbar. A new document
gets a fresh root key, clearing native scroll and pointer state. No SDK or C
changes are needed for this scene.

The cache preserves the previous 32-pixel quantization, 48 MiB default budget,
80% eviction target, exemption for the just-rendered image, stale previews until
a replacement arrives and rejection of results after invalidation/close. Closing
also prevents new render requests. Malformed image dimensions are rejected and
the adapter releases their handles so a later request can retry.

Tests can run without recompiling the compositor:

```sh
cargo test --manifest-path rust/Cargo.toml -p axia-shell --example pdf-model \
  --target-dir build/sdk-rust --locked --offline
node server/tests/pdf_model_test.mjs
cargo test --manifest-path rust/Cargo.toml -p axia-shell --lib \
  native_view::tests::pdf_sidebar --target-dir build/sdk-rust --locked --offline
```

The Node suite builds only its Rust test model if needed; `AXIA_PDF_MODEL_BIN`
can select an already-built example. Its subprocess adapter replays each owner's
history into the production source, retaining no alternative JS policy.

`rust/axia-shell/tests/fixtures/pdf_geometry.json` freezes 312 results from the
pre-migration JS at `eca7b122` (seed 1731). `pdf_cache_trace.json` freezes 13
snapshots of the previous cache under `server/tests/pdf_cache_scenario.mjs`,
including deduplication, LRU, cancellation, late results, invalidation and close.
They are reference artifacts; normal tests do not regenerate them. Additional
Rust/JS tests exercise ABI registration, model isolation, host failure cleanup,
entry identity, malformed requests and scalar-only scrolling.

Validated on 2026-11-13: 1,146 Node adapter/reference checks, 41 Rust model/ABI
tests and two native scene/input tests passed. The scene tests mount the model's
tree in the real host, exercise localized tabs, page clicks, wheel events and
bookmark disclosures, and check document replacement and unmount cleanup.
The PDF application bundled successfully in memory with the real
SDK/runtime aliases. No compositor rebuild or graphical session test was run.

The compositor must eventually be rebuilt to expose the `pdf` model. This stage
does not migrate PDF decoding/rendering, the main React window or
the full application to a native Rust view.

Further frozen references in `pdf_viewer.json` (216 zoom/fit cases) and
`pdf_navigation.json` (72 viewport/jump/hit scenarios, seed 5801) come from the
original viewer policies at `eca7b122`. Normal pointer/scroll navigation sends
scalar facts to the layout's owner, not all page sizes/positions. Thumbnail and
page image lifetimes remain in the previously tested host cache.

`pdf_sidebar.json` freezes 40 thumbnail scenarios (initial page following,
manual scroll and a subsequent page jump), generated by executing the layout,
follow and visibility statements from `eca7b122:apps/pdf/sidebar.jsx`, seed 7123.
Additional tests cover collapsed descendants, malformed updates preserving the
last valid state, independent owners, resize clamping and 100,000-page documents.

The remaining React code still owns presentation, text-drag gesture capture,
app menus and dialogs; the host executes the Rust editor's helper
effects and mirrors its ink/draft deltas. The complete PDF
application is therefore not yet a native Rust view.

The host binds each viewer controller to the currently open helper object.
Delayed events from an older React render cannot start another search/selection
against a closed document. Rust request generations separately reject late
replies, including repeated searches with exactly the same text. Failed viewport
updates preserve the last valid frame state; nonfinite derived geometry is rejected.

## Native window plan

The remaining React code is `apps/pdf/index.jsx` (the window), `sidebar.jsx` and the
JS adapters `viewer.js`, `cache.js`, `geometry.js`, `sidebar.js`, `model.js`. The
policy is already Rust (`Model::request`); the window must become a compiled
`NativeApp` (`id: "pdf"`, isolated helper I/O through `runtime/native_pdf_services.js`)
that calls the same `Model` commands the adapters send today. The compiled-app
policy test requires every file under `apps/pdf/` to be Rust, so JSX, JS and the
JSON manifest go in the last stage, in one commit with the registry change.

Stages (each committed with tests; the JSX window stays the live app until stage 4):

1. **Core window.** `native.rs` / `native_ui.rs`: welcome list, opening, error and
   password states, toolbar, status bar, sidebar (reusing `sidebar_ui.rs`), page
   frames with `documentImage` rasters from `Model` viewport/cache commands,
   wheel/keyboard/zoom/rotate/mode/page navigation and window state. Document I/O
   through `Jobs` (`pdf.open`, `pdf.sizes`, `pdf.render`, `pdf.release`,
   `pdf.close`, `pdf.outline`). Tested with the in-process `Host` and fake services.
2. **Page overlays and pointer input.** Page info (links, fields, notes), link
   hits, text-drag selection and copy, search field with hit highlighting, go-to-page.
3. **Editing.** Highlight/ink/note tools, text/choice form editors, undo, save copy,
   print, menus (`windows.menus`) and dirty state, using `edit_model.rs` effects.
4. **Registration.** `manifest.rs`, `rust/native_apps.rs`, `native_view::app`,
   `runtime/native_pdf_services.js` (doc registry per window lease: open/render/
   find/select/annot/field/undo/save/print/outline, crash events), removal of
   `index.jsx`, `sidebar.jsx`, the JS adapters and `manifest.json`, runtime test
   with the real `axia-doc` helper and a generated PDF.

Contract notes for the window: frames come from `{"cmd":"view","op":"viewport"}`
(`first`/`last`/`currentPage`/`frames`/`renders`); cache admission from
`{"cmd":"cache","op":"begin"|"complete"|"fail"|"drop"|"invalidate"|"best"}` whose
`release` list names images the transport must release; keys and wheel from
`{"cmd":"viewer","op":"key"|"wheel"}`; sidebar from `{"cmd":"sidebar",...}`.
