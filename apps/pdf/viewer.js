// Viewer effects adapter. Rust owns navigation/input policy and async result
// generations; this class only calls the PDF helper and publishes accepted replies.
import { createPdfModel } from './model.js';
export class ViewerModel {
  constructor() { this.request = createPdfModel(); this.document = null; }
  call(op, facts = {}) { return this.request({ cmd: 'viewer', op, ...facts }); }
  reset() { this.document = null; this.call('reset'); this._applyEdit(this.request({ cmd: 'edit', op: 'state' })); }
  bind(doc) {
    if (doc !== this.document) { this.reset(); this.document = doc; this.edit('open', { pages: doc?.info?.pages }); }
  }
  edit(op, facts = {}, doc = this.document) {
    if (doc !== this.document) return null;
    return this._applyEdit(this.request({ cmd: 'edit', op, ...facts }));
  }
  _applyEdit(packet) {
    if (!packet) return null;
    try { this.onEdit?.(packet); }
    finally { if (packet.effect && this.document) this._runEdit(packet.effect, this.document); }
    return packet;
  }
  _runEdit(effect, doc) {
    let promise;
    try {
      const p = effect.payload;
      switch (effect.kind) {
        case 'annot': promise = doc.annot(p); break;
        case 'field': promise = doc.setField(p.id, p.value); break;
        case 'undo': promise = doc.undo(effect.page); break;
        case 'save': promise = doc.save(p.path); break;
        default: throw new Error('pdf.invalidEditRequest');
      }
    } catch (error) { promise = Promise.reject(error); }
    const finish = (ok, error) => {
      if (doc !== this.document) return;
      this.edit('complete', { ticket: effect.ticket, ok, message: error?.message || '' }, doc);
    };
    Promise.resolve(promise).then(() => finish(true), error => finish(false, error));
  }
  pageInfo(doc, page, publish) {
    if (!doc || doc !== this.document) return Promise.resolve();
    const stamp = this.request({ cmd: 'edit', op: 'infoStart', page });
    if (!stamp) return Promise.resolve();
    return doc.pageInfo(page).then(info => {
      if (doc !== this.document) return;
      const value = this.request({ cmd: 'edit', op: 'infoReply', page, stamp, info });
      if (value) publish(value);
    }).catch(() => {});
  }
  key(event, editing, hasDoc) {
    const { key, base, char } = event;
    return this.call('key', { editing, hasDoc, event: { key, base, char,
      meta: !!event.meta, ctrl: !!event.ctrl, alt: !!event.alt, shift: !!event.shift } });
  }
  wheel(event, single) {
    return this.call('wheel', { single, now: Date.now(), event: { deltaY: event.deltaY,
      ctrl: !!event.ctrl, meta: !!event.meta, shift: !!event.shift, precise: !!event.precise, notches: !!event.notches } });
  }
  find(doc, query, onHits, onGo) {
    // A delayed event from an old render must not start a new request against
    // a closed helper, even after the native request generations were reset.
    if (doc && doc !== this.document) return Promise.resolve();
    const ticket = this.call('findStart', { query: doc ? query : '' });
    onHits(ticket.hits);
    if (!ticket.query) return Promise.resolve();
    return doc.find(ticket.query, {}, (page, rects) => {
      const reply = this.call('findAppend', { token: ticket.token, page, rects });
      if (!reply) return;
      onHits(reply.hits);
      if (reply.go) onGo(reply.go.page, reply.go.rect);
    }).then(() => {
      const hits = this.call('findFinish', { token: ticket.token });
      if (hits) onHits(hits);
    }).catch(() => {});
  }
  clearSelection() { return this.call('selectionClear'); }
  select(doc, drag, style, onSelection, highlightColor) {
    if (!doc || doc !== this.document) return Promise.resolve();
    const token = this.call('selectionStart');
    return doc.select(drag.page, drag.x0, drag.y0, drag.x1, drag.y1, style).then(result => {
      if (highlightColor) {
        if (doc === this.document) this.edit('highlightReply', { token, page: drag.page, result, color: highlightColor }, doc);
        return;
      }
      const value = this.call('selectionReply', { token, page: drag.page, result });
      if (value) onSelection(value);
    }).catch(() => {});
  }
}
