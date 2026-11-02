// sidebar.jsx — PDF Viewer sidebar: thumbnails (only the visible ones are drawn, at
// low priority) or the document outline (PDF bookmarks, collapsible).
import { View, Text, Segmented, DocImage, t as tr } from '@lordos/sdk';
import { useEffect, useRef, useState } from 'react';
import { shown } from './geometry.js';

const W = 184, TW = 116;

function Thumbs({ doc, sizes, page, onPage, cache, rot, invert, th }) {
  const [offset, setOffset] = useState(0);
  const [h, setH] = useState(500);
  const box = useRef(null);
  const n = sizes.length;
  // height of each thumbnail (with its number)
  const heights = sizes.map((s) => { const [w, hh] = shown(s, rot); return Math.round(TW * hh / w) + 28; });
  const tops = [];
  let y = 8;
  for (const hh of heights) { tops.push(y); y += hh; }
  const total = y + 8;
  useEffect(() => { const id = setInterval(() => { if (box.current) { const l = lord.getLayout(box.current.id); if (l[3] !== h) setH(l[3]); } }, 300); return () => clearInterval(id); }, [h]);
  // follow the current page
  useEffect(() => {
    if (tops[page] == null) return;
    if (tops[page] < offset) setOffset(Math.max(0, tops[page] - 8));
    else if (tops[page] + heights[page] > offset + h) setOffset(Math.min(Math.max(0, total - h), tops[page] + heights[page] - h + 8));
  }, [page]);
  const vis = [];
  for (let i = 0; i < n; i++) if (tops[i] + heights[i] >= offset && tops[i] <= offset + h) vis.push(i);
  useEffect(() => {
    if (!cache) return;
    for (const i of vis) cache.want(i, TW, rot, invert, 0);
  });
  return (
    <View ref={box} onWheel={(e) => setOffset((o) => Math.max(0, Math.min(Math.max(0, total - h), o + e.deltaY)))} style={{ flex: 1, overflow: 'hidden' }}>
      {vis.map((i) => {
        const [w, hh] = shown(sizes[i], rot);
        const th2 = Math.round(TW * hh / w);
        const e = cache?.get(i, TW, rot, invert) || cache?.best(i, rot, invert, TW);
        return (
          <View key={i} onClick={() => onPage(i)} style={{ position: 'absolute', left: 0, right: 0, top: tops[i] - offset, alignItems: 'center', gap: 4 }}>
            <View style={{ width: TW + 8, height: th2 + 8, borderRadius: 5, alignItems: 'center', justifyContent: 'center', border: `3px solid ${i === page ? th.accent : 'transparent'}` }}>
              <View style={{ width: TW, height: th2, backgroundColor: invert ? '#1b1b1f' : '#ffffff', boxShadow: '0 1px 4px rgba(0,0,0,0.35)' }}>
                {e ? <DocImage image={e.image} style={{ width: TW, height: th2, objectFit: 'fill' }} /> : null}
              </View>
            </View>
            <Text style={{ fontSize: 11, fontWeight: i === page ? 600 : 400, color: i === page ? th.text : th.textDim }}>{String(i + 1)}</Text>
          </View>
        );
      })}
    </View>
  );
}

function OutlineItems({ items, depth, onPage, page, th, open, toggle, path }) {
  return items.map((it, i) => {
    const key = `${path}${i}`;
    const isOpen = open.has(key) || (it.open && !open.has(`!${key}`));
    return (
      <View key={key}>
        <View onClick={() => it.page >= 0 && onPage(it.page)} style={{ flexDirection: 'row', alignItems: 'center', minHeight: 26, paddingRight: 8, paddingLeft: 4 + depth * 12, borderRadius: 6 }}>
          <View onClick={(e) => { e.stopPropagation(); toggle(key, isOpen); }} style={{ width: 14, alignItems: 'center' }}>
            {it.kids ? <Text style={{ fontSize: 8, color: th.textDim }}>{isOpen ? '▼' : '▶'}</Text> : null}
          </View>
          <Text style={{ flex: 1, fontSize: 12, fontWeight: depth === 0 ? 500 : 400, color: th.text, lineHeight: 16 }}>{it.title}</Text>
          {it.page >= 0 ? <Text style={{ fontSize: 10, color: th.textDim }}>{String(it.page + 1)}</Text> : null}
        </View>
        {it.kids && isOpen ? <OutlineItems items={it.kids} depth={depth + 1} onPage={onPage} page={page} th={th} open={open} toggle={toggle} path={`${key}.`} /> : null}
      </View>
    );
  });
}

function Outline({ doc, onPage, page, th }) {
  const [items, setItems] = useState(null);
  const [open, setOpen] = useState(() => new Set());
  const [offset, setOffset] = useState(0);
  useEffect(() => { doc.outline().then((r) => setItems(r.items || [])).catch(() => setItems([])); }, [doc]);
  const toggle = (k, isOpen) => setOpen((s) => { const n = new Set(s); n.delete(k); n.delete(`!${k}`); n.add(isOpen ? `!${k}` : k); return n; });
  if (items == null) return <Text style={{ margin: 12, fontSize: 12, color: th.textDim }}>{tr('pdf.loading')}</Text>;
  if (!items.length) return <Text style={{ margin: 12, fontSize: 12, color: th.textDim }}>{tr('pdf.noOutline')}</Text>;
  return (
    <View onWheel={(e) => setOffset((o) => Math.max(0, o + e.deltaY))} style={{ flex: 1, overflow: 'hidden' }}>
      <View style={{ translateY: -offset, paddingHorizontal: 8, paddingVertical: 4 }}>
        <OutlineItems items={items} depth={0} onPage={onPage} page={page} th={th} open={open} toggle={toggle} path="" />
      </View>
    </View>
  );
}

export function Sidebar({ side, setSide, doc, sizes, page, onPage, cache, rot, invert, th }) {
  return (
    <View style={{ width: W, backgroundColor: th.sidebarBg }}>
      <View style={{ paddingHorizontal: 12, paddingTop: 10, paddingBottom: 6 }}>
        <Segmented appearance="neutral" small fill value={side} onChange={setSide} options={[['thumbs', tr('pdf.side.thumbsShort')], ['outline', tr('pdf.side.outlineShort')]]} />
      </View>
      {side === 'thumbs'
        ? <Thumbs doc={doc} sizes={sizes} page={page} onPage={onPage} cache={cache} rot={rot} invert={invert} th={th} />
        : <Outline doc={doc} onPage={onPage} page={page} th={th} />}
      <View style={{ position: 'absolute', right: 0, top: 0, bottom: 0, width: 1, backgroundColor: th.separator }} />
    </View>
  );
}
