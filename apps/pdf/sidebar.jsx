// Native sidebar transport: Rust builds every widget and the native host handles
// drawing, pointer input and outline scrolling. JS keeps helper/image ownership.
import { NativeView, NATIVE_TREE, treeOwners, useAppWindow } from '@lordos/sdk';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { SidebarModel } from './sidebar.js';

const MANIFEST = { ...NATIVE_TREE, shell: false };
const WIDTH = 184, THUMB_WIDTH = 116, HEADER_HEIGHT = 38;

export function Sidebar({ side, setSide, doc, sizes, page, onPage, cache, rot, invert, th }) {
  const { id } = useAppWindow();
  const owner = `pdf-sidebar:${id}`;
  const model = useMemo(() => new SidebarModel(), [doc]);
  const mount = useRef(null), current = useRef(null);
  const [, redraw] = useState(0);
  const [height, setHeight] = useState(500);
  const [outlineReady, setOutlineReady] = useState(0);
  useMemo(() => model.configure(sizes, rot), [model, sizes, rot]);
  const { frames } = model.viewport(height, page);
  const loading = outlineReady !== model.id;

  useEffect(() => {
    if (side !== 'outline' || !loading) return undefined;
    let alive = true;
    Promise.resolve().then(() => doc.outline()).then(reply => {
      if (alive) { model.outline(reply.items || []); setOutlineReady(model.id); }
    }).catch(() => { if (alive) { model.outline([]); setOutlineReady(model.id); } });
    return () => { alive = false; };
  }, [doc, model, side]);

  useEffect(() => {
    const measure = () => {
      if (!mount.current) return;
      const frame = lord.getLayout(mount.current.id);
      setHeight(Math.max(0, frame[3] - HEADER_HEIGHT));
    };
    measure();
    const timer = setInterval(measure, 300);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    if (side === 'thumbs' && cache)
      for (const frame of frames) cache.want(frame.page, THUMB_WIDTH, rot, invert, 0);
  });
  const images = {};
  if (side === 'thumbs' && cache) for (const frame of frames) {
    const entry = cache.get(frame.page, THUMB_WIDTH, rot, invert) || cache.best(frame.page, rot, invert, THUMB_WIDTH);
    if (entry) images[frame.page] = entry.image;
  }
  const theme = {};
  for (const key of ['text', 'textDim', 'accent', 'separator', 'mode', 'control', 'controlRadiusSmall', 'sidebarBg']) theme[key] = th[key];
  const tree = model.tree({ side, images, invert, loading, theme, document: model.id });
  current.current = ({ action, extra }) => {
    const [cmd, key] = action;
    if (cmd === 'side') setSide(key);
    else if (cmd === 'page') onPage(key);
    else if (cmd === 'pick') { const target = model.pick(key); if (target != null) onPage(target); }
    else if (cmd === 'toggle') { model.toggle(key); redraw(r => r + 1); }
    else if (cmd === 'scroll') { model.scroll(extra.deltaY); redraw(r => r + 1); }
  };
  useLayoutEffect(() => {
    treeOwners.set(owner, effect => current.current(effect));
    return () => treeOwners.delete(owner);
  }, [owner]);
  return <NativeView manifest={MANIFEST} args={{ owner, tree }} mountRef={mount} style={{ width: WIDTH, flex: 0, flexShrink: 0 }} />;
}
