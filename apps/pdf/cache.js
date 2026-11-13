// Rust chooses cache keys, admission, generations, previews and LRU eviction.
// JS owns raster handles and pending helper promises and performs release effects.
import { releaseImage } from '@lordos/sdk';
import { createPdfModel } from './model.js';
export class PageCache {
  constructor(doc, onChange, budget = 48 * 1024 * 1024) {
    this.doc = doc;
    this.onChange = onChange;
    this.map = new Map();
    this.pending = new Map();
    this._model = createPdfModel();
    this.budget = budget;
  }
  _request(op, facts = {}) {
    const reply = this._model({ cmd: 'cache', op, ...facts });
    this._stats = reply.stats;
    for (const key of reply.release) {
      const entry = this.map.get(key);
      if (entry) { releaseImage(entry.image); this.map.delete(key); }
    }
    return reply.value;
  }
  get budget() { return this._budget; }
  set budget(value) { this._request('configure', { budget: value }); this._budget = value; }
  get bytes() { return this._stats.bytes; }
  get tick() { return this._stats.tick; }
  get firstAt() { return this._stats.firstAt; }
  get closed() { return this._stats.closed; }
  key(page, width, rot, inv) { return this._request('key', { page, width, rot, inv: !!inv }); }
  _entry(value) {
    if (!value) return null;
    const { key, ...metadata } = value;
    const entry = this.map.get(key);
    return entry ? Object.assign(entry, metadata) : null;
  }
  get(page, width, rot, inv) { return this._entry(this._request('get', { page, width, rot, inv: !!inv })); }
  best(page, rot, inv, width) { return this._entry(this._request('best', { page, width, rot, inv: !!inv })); }
  want(page, width, rot, inv, prio) {
    const ticket = this._request('begin', { page, width, rot, inv: !!inv });
    if (!ticket) return;
    const t0 = Date.now();
    let promise;
    try { promise = this.doc.render(page, ticket.width, { rot, invert: inv, prio }); }
    catch (error) { this._request('fail', { token: ticket.token }); throw error; }
    this.pending.set(ticket.token, promise);
    promise.then(result => {
      this.pending.delete(ticket.token);
      let value;
      try {
        value = this._request('complete', { token: ticket.token, w: result.w, h: result.h,
          ms: Date.now() - t0, now: Date.now() });
      } catch (error) { releaseImage(result.image); throw error; }
      if (!value) { releaseImage(result.image); return; }
      const { key, ...metadata } = value;
      this.map.set(key, { image: result.image, ...metadata });
      this.onChange();
    }).catch(() => {
      this.pending.delete(ticket.token);
      this._request('fail', { token: ticket.token });
    });
  }
  drop(keep) {
    for (const token of this._request('drop', { keep: [...keep] })) {
      const promise = this.pending.get(token);
      if (promise) this.doc.cancel(promise);
    }
  }
  evict(except) { this._request('evict', { except }); }
  invalidate(page) { this._request('invalidate', { page }); this.onChange(); }
  purgeStale(page) { this._request('purge', { page }); }
  clear() { this._request('clear'); }
  close() { this._request('close'); }
}
