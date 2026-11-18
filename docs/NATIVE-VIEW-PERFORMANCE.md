# Native view updates

The keyed sibling matcher has an ordered fast path and builds a lazy index only
when matching departs from the old order. Matching is linear in the number of
siblings (expected hash lookup cost for rearrangements), preserves scene IDs,
and keeps the first remaining match for duplicate keys of the same widget kind.
Removal still clears focus, pointer and hover references before destroying nodes.
This does not make every other tree/model/layout operation constant time.

## Retained regions

`native_view::retained::RetainedTree` owns one full widget specification and its
semantic signature. It invokes the builder only when the signature or region key
changes, returning a small `{kind, key, revision, retained:true}` marker.
`NativeApp::retained_tree(key)` supplies the full specification to the host on
first mount, remount and invalidation. `clear()` discards the tree/signature while
keeping revision numbers monotonic. No Markdown/persistence schema changes.

The host skips an existing region only when both its revision and its rendering
epoch match. Facts, focus transitions and viewport size changes invalidate that
epoch. Producers must include their own content/presentation dependencies in the
signature; a revision is a correctness contract, not an arbitrary counter added
to otherwise changing specifications. Invalid/missing markers cannot overwrite
an existing widget with an incomplete tree. Ordinary trees need no revisions.

Notes applies this to its virtual sidebar and Markdown reader. Sidebar signatures
include session revision, query, selection, offset and viewport; reader signatures
include note identity, body revision, width, widget mode and outline visibility.
Both include presentation changes (context, language and date region). Editing
clears the reader region. The complete reading scroll node keeps its original key
inside a flex wrapper, preserving scroll and outline navigation.

## Shell bridge

Native windows observe settings, catalog, window facts, clock and screen changes
only when their manifest requests them. Shared derived catalog/window snapshots
subscribe lazily, coalesce synchronous source notifications and retain identity
when the serializable output is unchanged. Window metadata preparation excludes
geometry/focus-only changes. A memo boundary carries only the window facts the
bridge actually consumes, so fresh equivalent provider objects do not prepare
another snapshot. Service polling retains the old snapshot on equal results.

Theme, settings, catalog, registry, arguments, focus, services and context scalars
have independent encoder revisions. Unchanged blocks reuse their JSON; immutable
object snapshots can share their encoding across mounts through a WeakMap. The
complete compatible facts snapshot is sent only when its JSON changes. This
preserves the current Rust/QuickJS wire protocol. Cache blocks have a fixed set
of names per mount and weak shared caches do not keep dead source objects alive.
Store/context inputs must be immutable snapshots, as with useSyncExternalStore.

## Verification and measurement

`server/tests/native_view_facts_test.mjs` bundles the production bridge with the
real React renderer. It covers narrow/lazy subscriptions, coalescing even when a
render reads ahead of notification, independent encodings, unrelated geometry,
opening another window, own focus/args/mode, settings, catalogs, service polling,
language/theme/screen changes, teardown and remount. It runs in the server test
suite. Rust coverage exercises 2000 stable/reversed/changed siblings, duplicate
keys, retained-builder reuse, remount, focus, viewport and presentation changes,
as well as Notes content/query/mode/selection invalidation.

`server/tests/native_tree_bench.js` is a headless component benchmark. Use a
release server, an isolated temporary HOME/XDG_RUNTIME_DIR and a copy of that
fixture outside the signed asset root in a development build. Run with
`--headless 800x600 --timeout 5 --theme /absolute/path/to/probe.js` and the normal
font/shell paths. Read `tree-audit` in HOME/.cache/axia/session.log. Seven forced
updates alternate root opacity while retaining keyed siblings. Their median
includes JSON parsing, conversion and reconciliation; it excludes JS stringify,
layout, painting and physical click-to-display latency. Do not treat these as
whole-desktop latency or a memory-saving guarantee.

### Before/after component results

On the same Xeon E5-2667 v4, release Rust from commit `9fc3f46c` and this change
were linked against the same compositor C objects. Three runs alternated build
order, with fresh temporary homes. The table is the median of each run's median
(seven samples for trees). Other machine activity caused occasional outliers;
small-list differences are within that variation, rather than demonstrated gains.

| Keyed siblings | Before | After |
| --- | ---: | ---: |
| 100 | 0.475 ms | 0.476 ms |
| 500 | 2.550 ms | 2.585 ms |
| 1000 | 5.312 ms | 4.738 ms |
| 2000 | 12.226 ms | 10.079 ms |

Thus the 2000-sibling update used about 18% less time in this fixture. This is
still a complete changed-tree update, including parsing and widget work; the
linear matcher alone is only one part of it.

A separate fixture copied the same 200 Markdown notes into each temporary home,
waited for the real native tree, warmed up twice, then forced 31 app syncs via
`nativeViews.result(handle, null)` without changing facts. Taking the median of
three run medians gave **1.119 ms before and 0.136 ms after** (about 88% less).
The new reader flex wrapper adds one scene node; the retained sidebar and reader
builders stay unused during unchanged syncs. This measures refresh work,
not initial disk loading or physical presentation. Idle RSS reduction was not
established by this benchmark.

In the real-React bridge fixture, equivalent provider/argument objects and an
unrelated geometry update produced zero additional fact-block preparations or
IPC sends. Opening another window updated only the window-list consumer;
settings and service changes updated only their declared consumers. Changing
one window's focus updated only that window's bridge. These are deterministic
work counts rather than a timing estimate for a full desktop.
