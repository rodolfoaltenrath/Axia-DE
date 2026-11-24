// Helper transport for the compiled PDF viewer. No UI and no policy: Rust chooses every
// request (what to open, render, search, select, edit) and this only runs the SDK call,
// keeps the document sessions and raster handles in the window's lease, and releases
// them when Rust asks, when a result is discarded or when the window closes.
// Search results, menu clicks and a crashed reader reach Rust as lease events (`pdf.events`).
// See docs/RUST-PDF.md.
export function pdfServices({ pdf, releaseImage, fs, print, events }) {
  // owner id → { docs: Map(id → { doc, images:Set }), seq }
  const sessions = new Map();
  // raster handle → the document entry that holds it
  const images = new Map();

  function session(owner) {
    let state = sessions.get(owner.id);
    if (!state) {
      state = { docs: new Map(), seq: 0 };
      sessions.set(owner.id, state);
      owner.cleanup(() => { for (const id of [...state.docs.keys()]) closeDoc(state, id); sessions.delete(owner.id); });
    }
    return state;
  }
  function entry(owner, id) {
    const found = session(owner).docs.get(id);
    if (!found) throw new Error('pdf.closed');
    return found;
  }
  function dropImage(image) {
    const holder = images.get(image);
    if (!holder) return;
    images.delete(image);
    holder.images.delete(image);
    try { releaseImage(image); } catch (e) { /* already gone */ }
  }
  function closeDoc(state, id) {
    const found = state.docs.get(id);
    if (!found) return;
    state.docs.delete(id);
    for (const image of [...found.images]) dropImage(image);
    try { found.doc.close(); } catch (e) { /* already closed */ }
  }
  const failure = error => new Error(error?.code === 'password' ? 'password' : error?.code === 'invalid' ? 'invalid' : String(error?.message || 'invalid'));
  const edit = (run) => ({ async run([id, ...rest], owner) { await run(entry(owner, id).doc, ...rest); return null; } });

  return {
    'pdf.recent': { run() {
      try {
        return fs.search(fs.home(), '.pdf', { limit: 40, depth: 6 })
          .filter(e => /\.pdf$/i.test(e.name)).map(e => ({ name: e.name, path: e.path }));
      } catch (e) { return []; }
    } },
    'pdf.open': {
      async run([path, password], owner) {
        const state = session(owner);
        let doc;
        try { doc = await pdf.open(String(path), { quick: true, password: String(password || '') }); }
        catch (error) { throw failure(error); }
        if (!owner.alive()) { doc.close(); return null; }
        const id = ++state.seq;
        state.docs.set(id, { doc, images: new Set() });
        doc.onCrash(() => owner.emit({ kind: 'crash', doc: id }));
        return { doc: id, pages: doc.info.pages, title: doc.info.title || '', sizes: doc.info.sizes || [] };
      },
      dispose(result, owner) {
        const state = sessions.get(owner.id);
        if (state && result?.doc) closeDoc(state, result.doc);
      },
    },
    'pdf.sizes': { async run([id], owner) {
      const all = await entry(owner, id).doc.sizes();
      return { sizes: all?.sizes || [], forms: all?.forms ?? null };
    } },
    'pdf.render': {
      async run([id, page, width, rot, invert, prio], owner) {
        const held = entry(owner, id);
        const result = await held.doc.render(page, width, { rot, invert: !!invert, prio });
        if (!owner.alive() || !session(owner).docs.has(id)) { releaseImage(result.image); throw new Error('pdf.closed'); }
        held.images.add(result.image);
        images.set(result.image, held);
        return { image: result.image, w: result.w, h: result.h };
      },
      // a render nobody will read (cancelled, or its window closed) frees its raster at once
      dispose(result) { if (result?.image) dropImage(result.image); },
    },
    'pdf.release': { run([image]) { dropImage(image); return null; } },
    'pdf.close': { run([id], owner) { closeDoc(session(owner), id); return null; } },
    'pdf.outline': { run([id], owner) { return entry(owner, id).doc.outline(); } },
    'pdf.pageInfo': { run([id, page], owner) { return entry(owner, id).doc.pageInfo(page); } },
    'pdf.select': { run([id, page, x0, y0, x1, y1, style], owner) { return entry(owner, id).doc.select(page, x0, y0, x1, y1, style); } },
    'pdf.find': { run([id, query, token], owner) {
      const finish = () => owner.emit({ kind: 'findDone', token });
      entry(owner, id).doc.find(String(query), {}, (page, rects) => owner.emit({ kind: 'find', token, page, rects })).then(finish, finish);
      return null;
    } },
    'pdf.events': { run: (args, owner) => events(owner.id) },
    'pdf.annot': edit((doc, annotation) => doc.annot(annotation)),
    'pdf.setField': edit((doc, field, value) => doc.setField(field, value)),
    'pdf.undo': edit((doc, page) => doc.undo(page)),
    'pdf.save': edit((doc, path) => doc.save(String(path))),
    'pdf.print': { run([id, title], owner) { print.open({ title: String(title), doc: entry(owner, id).doc }); return null; } },
  };
}
