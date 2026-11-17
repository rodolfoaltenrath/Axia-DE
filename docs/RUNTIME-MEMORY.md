# Runtime memory and Notes I/O

The image, shadow, glyph and transform caches have independent byte budgets.
These are reusable-resource limits, not limits on the entire compositor RSS.

| Cache | Reusable storage | Retirement |
| --- | --- | --- |
| Path images | 16 MiB, including source/scaled vector capacity and entry metadata; 2048 entries | Least-recently-used unreferenced images |
| Shadow masks | 16 MiB; at most 32 masks | Least-recently-used masks; 10 seconds without use |
| Glyphs | 8 MiB, including the hash table and grayscale/color rasters; 16384 entries | Cold generations; 10 seconds without use |
| Transformed subtrees | 32 MiB across 16 slots | Least-recently-used rasters between paints; 10 seconds per unused slot |

Live scene image handles are never invalidated to meet a cache budget. Their
necessary storage can exceed 16 MiB; unreferenced entries then retire first.
A shadow or glyph larger than its cache budget remains temporary storage for its
current synchronous draw, rather than a permanently cached exception.

`paint_cache_tick()` runs between synchronous paints in both the compositor and
isolated clients. Its deadline participates in polling, so transform expiration
also runs when no additional frames are drawn. Shadow row workers have joined
before a mask can retire. Text's existing pointer lifetime (until the next glyph
lookup) also covers hash-table pruning. Pixel sampling, shaping and compositing
arithmetic are unchanged.

`lord.stats()` exposes `imageCacheKB`, `shadowCacheKB`, `glyphCacheKB` and
`transformCacheKB`. `managedImageKB` retains its generated-image meaning.

## Notes

The initial Markdown scan and all note-content writes/fsyncs run on one ordered
storage worker per session. The UI receives load results and save acknowledgements
without waiting for disk I/O. The queue holds at most 32 pending note snapshots,
coalesces pending revisions of the same slug, and leaves work dirty when full.
Only session teardown waits for the queue to drain; closing an individual window
queues its final edits. Atomic replacement, the existing Markdown/YAML format,
legacy `Notas` migration and home confinement remain in place. Errors leave the
latest revision dirty for retry; older acknowledgements cannot discard newer
edits. Existing small directory/attachment metadata operations retain their paths.

Literal regular expressions compile once. Each note's preview changes with its
body; sidebar summaries are shared for an unchanged session revision/query and
never contain the Markdown body. The sidebar mounts only visible fixed-height
rows plus overscan, with wheel scrolling and a draggable scrollbar. Editor line
wrapping is reused until text or width/font/padding changes.

Regression coverage includes shared live image handles under cache pressure,
scaled capacity, raster pixel/metric equality after glyph eviction, cache idle
expiration, original Notes serialization, concurrent edits/acknowledgements,
write failures/retries, queue pressure, deletion during pending saves, teardown
with more than 32 dirty notes, and sidebar virtualization. Existing renderer
pixel comparisons and real compositor integration remain additional gates.

## Controlled headless measurements

On an Intel Xeon E5-2667 v4, using the same synthetic fixtures as the initial
investigation:

- Loading and releasing twelve distinct 1024x1024 PNGs left about 53 MiB of
  additional RSS before the change and 21 MiB afterward. The new path cache
  reported 12 MiB retained. RSS also includes allocator/runtime storage, so it
  is not identical to the cache counter.
- With 200 small Markdown notes, the initial storage call previously occupied
  the UI thread for 1.14 seconds. The asynchronous call returned in 0.18 ms;
  the native tree was ready after about 28 ms, including the probe's 20 ms
  polling interval. That tree contained 134 scene nodes. Seven forced refreshes
  with unchanged data had a median of 1.16 ms, excluding layout and painting.

These are component measurements, not physical click-to-display latency or
predictions for other hardware.
