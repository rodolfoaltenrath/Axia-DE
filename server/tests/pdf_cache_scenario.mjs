// A deterministic helper lifecycle recorded against the JS cache before removal.
// Fake promises model out-of-order results, cancellation, invalidation and close.
export async function cacheScenario(PageCache) {
  const released = [], cancelled = [], calls = [], trace = [];
  let changed = 0, now = 1000;
  const originalNow = Date.now;
  Date.now = () => now;
  globalThis.__pdfRelease = image => released.push(image);
  const pending = [];
  const doc = {
    render(page, width, options) {
      const id = pending.length;
      let resolve, reject;
      const promise = new Promise((a, b) => { resolve = a; reject = b; });
      pending.push({ resolve, reject, promise });
      calls.push({ id, page, width, ...options });
      return promise;
    },
    cancel(promise) { cancelled.push(pending.findIndex(p => p.promise === promise)); },
  };
  const cache = new PageCache(doc, () => changed++, 1200);
  const snapshot = (label, extra = {}) => trace.push({ label, ...extra, bytes: cache.bytes, tick: cache.tick,
    firstAt: cache.firstAt, closed: !!cache.closed, pending: cache.pending.size, changed,
    entries: [...cache.map].map(([key, e]) => ({ key, image: e.image, w: e.w, h: e.h, page: e.page,
      width: e.width, rot: e.rot, inv: e.inv, gen: e.gen, bytes: e.bytes, used: e.used, ms: e.ms })),
    released: [...released], cancelled: [...cancelled], calls: [...calls] });
  const want = (page, width) => cache.want(page, width, 0, false, 4);
  const settle = async (id, fail = false) => {
    now += 100;
    if (fail) pending[id].reject(new Error('helper cancelled'));
    else pending[id].resolve({ image: `image:${id}`, w: 10, h: 10 });
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
  };
  try {
    want(0, 47); want(0, 46); want(1, 64); snapshot('deduplicate');
    await settle(0); await settle(1); snapshot('ready');
    want(2, 96); await settle(2); cache.get(0, 32, 0, false);
    want(3, 32); await settle(3); snapshot('lru');
    want(4, 32); want(5, 32); cache.drop(new Set([0, 4])); snapshot('drop');
    await settle(5, true); await settle(4); snapshot('retryable rejection');
    want(0, 64); cache.invalidate(0);
    snapshot('old preview', { best: cache.best(0, 0, false, 32)?.image, exact: cache.get(0, 32, 0, false) });
    want(0, 32); await settle(6); snapshot('stale reply');
    await settle(7); snapshot('replacement');
    cache.clear(); snapshot('clear');
    want(6, 32); cache.clear(); await settle(8); snapshot('clear preserves pending');
    want(7, 32); cache.close(); snapshot('close');
    await settle(9); snapshot('late reply after close');
    cache.close(); snapshot('idempotent close');
    return trace;
  } finally { Date.now = originalNow; }
}
