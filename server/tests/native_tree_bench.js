// Component benchmark for axia-server --headless 800x600 --theme <this-file>.
// Alternate one root style while keeping 100–2000 keyed siblings intact.
// Measures nativeViews.facts (JSON parsing, model conversion and reconciliation),
// excluding JS stringify, layout, paint and physical presentation. No threshold:
// compare the same fixture/build profile on the same otherwise idle hardware.
const root = lord.createNode('view');
lord.setRoot(root);
setTimeout(() => {
  for (const count of [100, 500, 1000, 2000]) {
    const node = lord.createNode('view');
    lord.appendChild(root, node);
    const tree = { kind: 'view', key: 'root', style: { width: 800, height: 600 },
      children: Array.from({ length: count }, (_, i) => ({ kind: 'view', key: 'item-' + i, style: { width: 1, height: 1 } })) };
    const facts = { args: { owner: 'audit', tree }, context: { theme: {} } };
    const handle = lord.nativeViews.mount(node, 'shell:tree', JSON.stringify(facts));
    const samples = [];
    for (let i = 0; i < 7; i++) {
      tree.style.opacity = i % 2 ? 0.99 : 1;
      const encoded = JSON.stringify(facts), start = lord.now();
      lord.nativeViews.facts(handle, encoded);
      samples.push(lord.now() - start);
    }
    samples.sort((a, b) => a - b);
    console.log('tree-audit ' + JSON.stringify({ count, medianUpdateMs: samples[3], maxMs: samples[6], ...lord.stats() }));
    lord.nativeViews.unmount(handle);
    lord.removeChild(root, node);
    lord.destroy(node);
  }
}, 200);
