// sidebar.jsx — PDF Viewer sidebar: thumbnails (only the visible ones are drawn, at
// low priority) or the document outline (PDF bookmarks, collapsible).
import { View, Text, Segmented, DocImage, t as tr } from '@lordos/sdk';
import { useEffect, useMemo, useRef, useState } from 'react';
import { SidebarModel } from './sidebar.js';

const W = 184, TW = 116;

function Thumbs({ doc, sizes, page, onPage, cache, rot, invert, th }) {
  const model = useMemo(() => new SidebarModel(), [doc]);
  const [, redraw] = useState(0);
  const [h, setH] = useState(500);
  const box = useRef(null);
  useMemo(() => model.configure(sizes, rot), [model, sizes, rot]);
  const { frames } = model.viewport(h, page);
  useEffect(() => { const id = setInterval(() => { if (box.current) { const l = lord.getLayout(box.current.id); if (l[3] !== h) setH(l[3]); } }, 300); return () => clearInterval(id); }, [h]);
  useEffect(() => {
    if (!cache) return;
    for (const frame of frames) cache.want(frame.page, TW, rot, invert, 0);
  });
  return (
    <View ref={box} onWheel={(e) => { model.scroll(e.deltaY); redraw(r => r + 1); }} style={{ flex: 1, overflow: 'hidden' }}>
      {frames.map(({ page: i, top, height: th2, selected }) => {
        const e = cache?.get(i, TW, rot, invert) || cache?.best(i, rot, invert, TW);
        return (
          <View key={i} onClick={() => onPage(i)} style={{ position: 'absolute', left: 0, right: 0, top, alignItems: 'center', gap: 4 }}>
            <View style={{ width: TW + 8, height: th2 + 8, borderRadius: 5, alignItems: 'center', justifyContent: 'center', border: `3px solid ${selected ? th.accent : 'transparent'}` }}>
              <View style={{ width: TW, height: th2, backgroundColor: invert ? '#1b1b1f' : '#ffffff', boxShadow: '0 1px 4px rgba(0,0,0,0.35)' }}>
                {e ? <DocImage image={e.image} style={{ width: TW, height: th2, objectFit: 'fill' }} /> : null}
              </View>
            </View>
            <Text style={{ fontSize: 11, fontWeight: selected ? 600 : 400, color: selected ? th.text : th.textDim }}>{String(i + 1)}</Text>
          </View>
        );
      })}
    </View>
  );
}

function Outline({ doc, onPage, th }) {
  const model = useMemo(() => new SidebarModel(), [doc]);
  const [rows, setRows] = useState(null);
  const [offset, setOffset] = useState(0);
  useEffect(() => {
    let alive = true;
    setRows(null); setOffset(0);
    doc.outline().then(r => {
      if (alive) setRows(model.outline(r.items || []).rows);
    }).catch(() => { if (alive) setRows([]); });
    return () => { alive = false; };
  }, [doc, model]);
  if (rows == null) return <Text style={{ margin: 12, fontSize: 12, color: th.textDim }}>{tr('pdf.loading')}</Text>;
  if (!rows.length) return <Text style={{ margin: 12, fontSize: 12, color: th.textDim }}>{tr('pdf.noOutline')}</Text>;
  return (
    <View onWheel={(e) => setOffset((o) => Math.max(0, o + e.deltaY))} style={{ flex: 1, overflow: 'hidden' }}>
      <View style={{ translateY: -offset, paddingHorizontal: 8, paddingVertical: 4 }}>
        {rows.map(row => (
          <View key={row.key} onClick={() => { const target = model.pick(row.key); if (target != null) onPage(target); }}
            style={{ flexDirection: 'row', alignItems: 'center', minHeight: 26, paddingRight: 8, paddingLeft: 4 + row.depth * 12, borderRadius: 6 }}>
            <View onClick={(e) => { e.stopPropagation(); setRows(model.toggle(row.key).rows); }} style={{ width: 14, alignItems: 'center' }}>
              {row.hasKids ? <Text style={{ fontSize: 8, color: th.textDim }}>{row.open ? '▼' : '▶'}</Text> : null}
            </View>
            <Text style={{ flex: 1, fontSize: 12, fontWeight: row.depth === 0 ? 500 : 400, color: th.text, lineHeight: 16 }}>{row.title}</Text>
            {row.page >= 0 ? <Text style={{ fontSize: 10, color: th.textDim }}>{String(row.page + 1)}</Text> : null}
          </View>
        ))}
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
