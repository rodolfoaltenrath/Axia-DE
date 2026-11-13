// PDF Viewer — continuous or single-page scrolling,
// thumbnails and outline, search with highlighting, zoom and fit, rotate, select and copy text,
// go to a page, links, forms, annotations (highlight, note and freehand) saved as a
// copy, dark mode with the page inverted and printing with the system dialog.
// Poppler runs in the confined axia-doc helper (see docs/DOCUMENTS.md); this app only asks.
import {
  View, Text, TextInput, Button, useTheme, useAppWindow, useKeyboard, addListener, wm, fs, clipboard, pdf, print, DocImage, releaseImage,
  formatNumber, direction, t as tr, Toolbar, ToolbarButton, ToolbarGroup, ToolbarSpacer, SearchField, EmptyState,
} from '@lordos/sdk';
import { useEffect, useMemo, useRef, useState } from 'react';
import { PageCache } from './cache.js';
import { shown, rectToView, layoutPages, viewport, navigate, frameFor, hitPoint } from './geometry.js';
import { ViewerModel } from './viewer.js';
import { Sidebar } from './sidebar.jsx';

export function Icon({ size: s }) {
  return (
    <View style={{ width: s, height: s, borderRadius: s * 0.225, background: 'linear-gradient(160deg, #ff7a59, #c8102e)',
      boxShadow: `0 ${s * 0.04}px ${s * 0.12}px rgba(0,0,0,0.35)`, overflow: 'hidden' }}>
      {/* two sheets, the front one with ruled lines and one highlighted */}
      <View style={{ position: 'absolute', left: s * 0.3, top: s * 0.12, width: s * 0.5, height: s * 0.64, borderRadius: s * 0.04, backgroundColor: 'rgba(255,255,255,0.55)' }} />
      <View style={{ position: 'absolute', left: s * 0.2, top: s * 0.2, width: s * 0.52, height: s * 0.66, borderRadius: s * 0.045, backgroundColor: '#ffffff',
        boxShadow: `0 ${s * 0.02}px ${s * 0.06}px rgba(0,0,0,0.3)` }} />
      {[0.32, 0.42, 0.52, 0.62, 0.72].map((y, i) => (
        <View key={y} style={{ position: 'absolute', left: s * 0.28, top: s * y, width: s * (i === 4 ? 0.22 : 0.36), height: Math.max(1, s * 0.04), borderRadius: s * 0.02,
          backgroundColor: i === 2 ? '#ffd60a' : '#d4d4dc' }} />
      ))}
      <Text style={{ position: 'absolute', left: s * 0.48, top: s * 0.66, fontSize: s * 0.13, fontWeight: 700, color: '#c8102e' }}>PDF</Text>
    </View>
  );
}

const GAP = 14;
const TOOL_COLORS = ['#ffd60a', '#30d158', '#64d2ff', '#ff6482', '#bf5af2'];
const winStates = new Map();

function argPath(args) {
  if (!args?.path) return null;
  const p = String(args.path);
  return p.startsWith('/') ? p : `${fs.home()}/${p}`;
}

function ToolBtn({ label, onClick, active, disabled, th, title }) {
  return (
    <View onClick={() => !disabled && onClick()} style={{ minWidth: 28, height: 26, paddingHorizontal: 7, borderRadius: 6, alignItems: 'center', justifyContent: 'center',
      backgroundColor: active ? th.accent : 'transparent', opacity: disabled ? 0.35 : 1 }}> 
      <Text style={{ fontSize: 13, fontWeight: 600, color: active ? '#fff' : th.text }}>{label}</Text>
    </View>
  );
}

// List of PDFs in the home folder (no-document screen)
function Welcome({ onOpen, th }) {
  const [list, setList] = useState(null);
  useEffect(() => { const id = setTimeout(() => { try { setList(fs.search(fs.home(), '.pdf', { limit: 40, depth: 6 }).filter((e) => /\.pdf$/i.test(e.name))); } catch (e) { setList([]); } }, 30); return () => clearTimeout(id); }, []);
  const box = th.mode === 'dark' ? 'rgba(255,255,255,0.04)' : 'rgba(255,255,255,0.7)';
  return (
    <View style={{ position: 'absolute', left: 0, top: 0, right: 0, bottom: 0, direction: direction() }}>
      <EmptyState icon={<Icon size={64} />} title={tr('pdf.welcome')} hint={tr('pdf.welcomeHint')} action={
        <View style={{ width: 420, maxHeight: 300, gap: 6 }}>
          {list == null ? <Text style={{ fontSize: 12, color: th.textDim, textAlign: 'center' }}>{tr('pdf.searching')}</Text>
            : list.length ? (
              <>
                <Text style={{ fontSize: 11, fontWeight: 600, color: th.textDim, marginLeft: 8 }}>{tr('pdf.recent')}</Text>
                <View style={{ borderRadius: 10, backgroundColor: box, border: `1px solid ${th.separator}`, overflow: 'hidden' }}>
                  {list.slice(0, 8).map((e, i) => <RecentRow key={e.path} e={e} first={i === 0} onOpen={onOpen} th={th} />)}
                </View>
              </>
            ) : <Text style={{ fontSize: 12, color: th.textDim, textAlign: 'center' }}>{tr('pdf.noneFound')}</Text>}
        </View>
      } />
    </View>
  );
}

function RecentRow({ e, first, onOpen, th }) {
  const [hover, setHover] = useState(false);
  return (
    <View onClick={() => onOpen(e.path)} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{ height: 32, flexDirection: 'row', gap: 10, paddingHorizontal: 12, alignItems: 'center', backgroundColor: hover ? th.hover : 'transparent' }}>
      {!first ? <View style={{ position: 'absolute', left: 12, right: 0, top: 0, height: 1, backgroundColor: th.separator }} /> : null}
      <Text style={{ fontSize: 10, color: '#ff453a', fontWeight: 700 }}>PDF</Text>
      <Text style={{ flex: 1, fontSize: 13, color: th.text, whiteSpace: 'nowrap', overflow: 'hidden' }}>{e.name}</Text>
    </View>
  );
}

export default function PdfViewer() {
  const th = useTheme();
  const { id: wid, args } = useAppWindow();
  const [controller] = useState(() => new ViewerModel());
  const saved = winStates.get(wid);
  const [path, setPath] = useState(() => saved?.path || argPath(args));
  const [doc, setDoc] = useState(null);
  const [error, setError] = useState(null);
  const [needPass, setNeedPass] = useState(false);
  const [password, setPassword] = useState('');
  const [rev, setRev] = useState(0);                  // something arrived from axia-doc: redraw
  const [zoom, setZoom] = useState(saved?.zoom || 'width');   // number | 'width' | 'page'
  const [rot, setRot] = useState(0);
  const [mode, setMode] = useState(saved?.mode || 'continuous');   // 'continuous' | 'single'
  const [invert, setInvert] = useState(() => th.mode === 'dark' && !!saved?.invert);
  const [side, setSide] = useState(saved?.side ?? 'thumbs');         // null | 'thumbs' | 'outline'
  const [offset, setOffset] = useState(0);
  const [offsetX, setOffsetX] = useState(0);
  const [page, setPage] = useState(0);
  const [tool, setTool] = useState('select');          // select | highlight | note | ink
  const [color, setColor] = useState(TOOL_COLORS[0]);
  const [selection, setSelection] = useState(null);    // { page, rects, text }
  const [query, setQuery] = useState('');
  const [hits, setHits] = useState(null);              // { list: [{ page, rect }], cur, done }
  const [goto, setGoto] = useState(null);              // text of the «go to page» field
  const [info, setInfo] = useState(() => new Map());   // page → { links, fields, notes }
  const [ink, setInk] = useState(null);                // { page, strokes: [[x,y…]] } in points
  const [editState, setEditState] = useState(() => controller.edit('state').state);
  const noteAt = editState.note, editField = editState.field, annotated = editState.history;
  const [msg, setMsg] = useState(null);
  const [view, setView] = useState({ w: 800, h: 600 });
  const area = useRef(null);
  const cache = useRef(null), thumbs = useRef(null);
  const drag = useRef(null);
  const edit = (op, facts = {}) => controller.edit(op, facts, doc);
  controller.onEdit = packet => {
    setEditState(previous => JSON.stringify(previous) === JSON.stringify(packet.state) ? previous : packet.state);
    if (packet.ink) setInk(previous => {
      const delta = packet.ink;
      if (delta.kind === 'replace') return delta.value;
      if (!previous) return previous;
      if (delta.kind === 'stroke') return { page: delta.page, strokes: [...previous.strokes, delta.point] };
      if (delta.kind === 'point') return { ...previous, strokes: previous.strokes.slice(0, -1).concat([[...previous.strokes[previous.strokes.length - 1], ...delta.point]]) };
      return previous;
    });
    if (packet.clearSelection) setSelection(null);
    if (packet.invalidate) {
      const p = packet.invalidate.page;
      cache.current?.invalidate(p); thumbs.current?.invalidate(p);
      if (packet.invalidate.refresh) refreshInfo(p);
    }
    if (packet.notice) setMsg(tr(packet.notice.key, { error: packet.notice.code ? tr(`docs.err.${packet.notice.code}`) : packet.notice.message }));
    if (packet.saved) setMsg(tr('pdf.saved', { name: packet.saved }));
  };

  useEffect(() => { winStates.set(wid, { path, zoom, mode, side, invert }); }, [path, zoom, mode, side, invert]);
  useEffect(() => () => { if (!wm.get(wid)) winStates.delete(wid); }, []);
  useEffect(() => { const p = argPath(args); if (p && args?.nonce) setPath(p); }, [args?.nonce]);
  useEffect(() => { if (!msg) return undefined; const id = setTimeout(() => setMsg(null), 3500); return () => clearTimeout(id); }, [msg]);

  // ---- open ----
  useEffect(() => {
    if (!path) return undefined;
    let alive = true, d = null;
    controller.reset();
    drag.current = null;
    setError(null); setDoc(null); setHits(null); setSelection(null); setInfo(new Map()); setOffset(0); setPage(0);
    const t0 = Date.now();
    pdf.open(path, { quick: true, password }).then(async (x) => {
      if (!alive) { x.close(); return; }
      d = x;
      controller.bind(x);
      cache.current = new PageCache(x, () => setRev((r) => r + 1));
      thumbs.current = new PageCache(x, () => setRev((r) => r + 1), 8 * 1024 * 1024);
      cache.current.openedAt = t0;
      x.onCrash(() => { if (alive) setError(tr('pdf.crashed')); });
      setNeedPass(false);
      setDoc(x);
      // the sizes of all the pages arrive later, without holding up the first one
      if (x.info.pages > 1) {
        const all = await x.sizes().catch(() => null);
        if (alive && all?.sizes) { x.info.sizes = all.sizes; x.info.forms = all.forms; setRev((r) => r + 1); }
      }
    }).catch((e) => {
      if (!alive) return;
      if (e.code === 'password') { setNeedPass(true); setError(password ? tr('pdf.wrongPassword') : null); }
      else setError(e.code === 'invalid' ? tr('pdf.invalid') : tr('pdf.openError', { error: e.message }));
    });
    wm.setTitle(wid, fs.basename(path));
    return () => { alive = false; drag.current = null; controller.reset(); cache.current?.close(); thumbs.current?.close(); d?.close(); };
  }, [path, password]);

  const n = doc?.info.pages || 0;
  // sizes: until they all arrive, all are assumed to be like the first one
  const sizes = useMemo(() => {
    if (!doc) return [];
    const s = doc.info.sizes || [];
    if (s.length >= n) return s;
    return Array.from({ length: n }, (_, i) => s[i] || s[0] || [595, 842]);
  }, [doc, rev, n]);

  // ---- area size and scale ----
  useEffect(() => {
    const measure = () => { if (area.current) { const l = lord.getLayout(area.current.id); if (l[2] !== view.w || l[3] !== view.h) setView({ w: l[2], h: l[3] }); } };
    measure();
    const id = setInterval(measure, 250);
    return () => clearInterval(id);
  }, [view.w, view.h, side]);
  const ref0 = shown(sizes[page] || [595, 842], rot);
  const { scale, pct } = controller.call('scale', { zoom, size: ref0, view });
  const lay = useMemo(() => layoutPages(sizes, rot, scale, GAP), [sizes, rot, scale]);
  const viewState = viewport(lay, { mode, page, offset, offsetX, view });
  const { maxOffset, maxX, first, last } = viewState;

  // ---- what is visible ----
  useEffect(() => {
    if (mode !== 'continuous' || !lay.tops.length) return;
    // the «current» page: the one in the center
    const p = viewState.currentPage;
    if (p !== page) setPage(p);
  }, [offset, first, last]);
  useEffect(() => {
    if (!doc || !cache.current) return undefined;
    const id = setTimeout(() => {
      const keep = new Set();
      for (const job of viewState.renders) {
        keep.add(job.page);
        cache.current.want(job.page, job.width, rot, invert, job.priority);
      }
      cache.current.drop(keep);
      // links and fields of what is visible
      for (let p = first; p <= last; p++) {
        if (info.has(p)) continue;
        info.set(p, null);
        controller.pageInfo(doc, p, r => setInfo(m => new Map(m).set(p, r)));
      }
    }, zoomAnim.current ? 120 : 0);
    zoomAnim.current = false;
    return () => clearTimeout(id);
  }, [doc, first, last, scale, rot, invert, rev, sizes]);   // rev: something arrived or a page changed (annotation, form)
  const zoomAnim = useRef(false);

  // first draw: it is measured (docs/DOCUMENTS.md asks for it)
  useEffect(() => {
    const c = cache.current;
    if (c && c.firstAt && !c.reported) { c.reported = true; console.log(`pdf: ${fs.basename(path)} first page in ${c.firstAt - c.openedAt} ms (${n} pages)`); }
  }, [rev]);

  // ---- navigation ----
  const goPage = (p, rect) => {
    const next = navigate(lay, p, rect);
    if (!next) return;
    setPage(next.page);
    if (next.offset != null) setOffset(next.offset);
  };
  const setZoomTo = (z) => {
    // keep in view what is at the top
    const top = page;
    zoomAnim.current = true;
    setZoom(z);
    setTimeout(() => { if (mode === 'continuous') setOffset((o) => o); goPageLater.current = top; }, 0);
  };
  const goPageLater = useRef(null);
  useEffect(() => { if (goPageLater.current != null) { const p = goPageLater.current; goPageLater.current = null; goPage(p); } }, [scale]);
  const zoomStep = (d) => {
    setZoomTo(controller.call('zoom', { scale, step: d }));
  };
  const onWheel = (e) => {
    const action = controller.wheel(e, mode === 'single');
    if (!action) return;
    if (action.kind === 'zoom') zoomStep(action.step);
    else if (action.kind === 'page') goPage(page + action.delta);
    else if (action.kind === 'horizontal') setOffsetX(x => controller.call('scroll', { value: x, delta: action.delta, max: maxX }));
    else if (action.kind === 'vertical') setOffset(o => controller.call('scroll', { value: o, delta: action.delta, max: maxOffset }));
  };

  // ---- search ----
  const runFind = (q) => {
    controller.find(doc, q, setHits, goPage);
  };
  const nextHit = (d) => {
    const result = controller.call('findNext', { delta: d });
    if (!result) return;
    setHits(result.hits);
    goPage(result.go.page, result.go.rect);
  };

  // ---- coordinates: event → page and point ----
  const pageLeft = (p) => {
    return frameFor(lay, p)?.left || 0;
  };
  const pageTop = (p) => frameFor(lay, p)?.top || 0;
  const hitPage = (ev) => {
    if (!area.current) return null;
    const [ax, ay] = lord.getLayout(area.current.id);
    return hitPoint(lay, ev.x - ax, ev.y - ay);
  };
  const linkAt = (hp) => {
    const inf = info.get(hp.page);
    if (!inf) return null;
    return inf.links[controller.call('hitItem', { items: inf.links, x: hp.x, y: hp.y })] || null;
  };
  const fieldAt = (hp) => {
    const inf = info.get(hp.page);
    if (!inf) return null;
    return inf.fields[controller.call('hitItem', { items: inf.fields, x: hp.x, y: hp.y })] || null;
  };

  // ---- mouse over the pages ----
  const select = (d, style) => {
    controller.select(doc, d, style, setSelection);
  };
  const clearSelection = () => setSelection(controller.clearSelection());
  const onDown = (ev) => {
    if (ev.button === 2) return;
    edit('fieldCancel');
    const hp = hitPage(ev);
    if (!hp) { clearSelection(); return; }
    if (tool === 'select') {
      const f = fieldAt(hp);
      if (f && !f.f.ro) { onField(hp.page, f); return; }
      const link = linkAt(hp);
      if (link) {
        if (link.page != null && link.page >= 0) goPage(link.page);
        else if (link.uri) { clipboard.set(link.uri); setMsg(tr('pdf.linkCopied', { uri: link.uri.slice(0, 80) })); }
        return;
      }
    }
    if (tool === 'note') { edit('noteOpen', { point: hp }); return; }
    if (tool === 'ink') {
      drag.current = { kind: 'ink', page: hp.page };
      edit('inkStart', { point: hp });
      return;
    }
    drag.current = { kind: 'text', page: hp.page, x0: hp.x, y0: hp.y, x1: hp.x, y1: hp.y, moved: false, at: Date.now() };
    clearSelection();
  };
  useEffect(() => {
    const mv = addListener('pointermove', (ev) => {
      const d = drag.current;
      if (!d) return;
      const hp = hitPage(ev);
      if (!hp || hp.page !== d.page) return;
      if (d.kind === 'ink') {
        edit('inkAppend', { point: hp, scale });
        return;
      }
      d.x1 = hp.x; d.y1 = hp.y; d.moved = true;
      const now = Date.now();
      if (now - (d.last || 0) > 50) { d.last = now; select(d, 0); }
    });
    const up = () => {
      const d = drag.current;
      drag.current = null;
      if (!d || d.kind !== 'text') return;
      if (!d.moved) { if (Date.now() - (lastClick.current || 0) < 400) select({ ...d, x1: d.x0 + 0.5, y1: d.y0 + 0.5 }, 1); lastClick.current = Date.now(); return; }
      controller.select(doc, d, 0, setSelection, tool === 'highlight' ? color : undefined);
    };
    const a = addListener('pointermove', () => {});
    const b = addListener('pointerup', up);
    return () => { mv(); a(); b(); };
  }, [first, last, scale, rot, offset, offsetX, view.w, sizes, tool, color, doc]);
  const lastClick = useRef(0);
  const finishInk = () => edit('inkFinish', { color });
  useEffect(() => { if (tool !== 'ink' && ink) finishInk(); }, [tool]);
  const undo = () => edit('undo');
  const onField = (page, field) => edit('fieldOpen', { page, field });
  const commitField = value => edit('fieldCommit', { value });
  const refreshInfo = p => {
    const current = doc;
    if (!current || current !== controller.document) return Promise.resolve();
    return controller.pageInfo(current, p, r => setInfo(m => new Map(m).set(p, r)));
  };

  // save a copy (never overwrites the original)
  const saveCopy = () => {
    if (!doc) return;
    const dir = fs.parent(path), base = fs.basename(path).replace(/\.pdf$/i, '');
    let name = tr('pdf.copyName', { name: base });
    for (let i = 2; i < 200; i++) { try { fs.stat(`${dir}/${name}.pdf`); name = `${tr('pdf.copyName', { name: base })} ${i}`; } catch (e) { break; } }
    const out = `${dir}/${name}.pdf`;
    edit('save', { path: out, name: `${name}.pdf` });
  };
  const doPrint = () => { if (doc) print.open({ title: fs.basename(path), doc }); };
  const copySel = () => { if (selection?.text) { clipboard.set(selection.text); setMsg(tr('pdf.copied')); } };
  const rotate = (d) => setRot(r => controller.call('rotate', { rot: r, delta: d }));

  // ---- keyboard ----
  useKeyboard((e) => {
    const action = controller.key(e, goto != null || !!noteAt || !!editField, !!doc);
    switch (action) {
      case 'find': findRef.current = true; setRev(r => r + 1); break;
      case 'copy': copySel(); break;
      case 'selectAll': { const [w, h] = sizes[page]; select({ page, x0: 0, y0: 0, x1: w, y1: h }, 0); break; }
      case 'print': doPrint(); break;
      case 'save': saveCopy(); break;
      case 'undo': undo(); break;
      case 'rotateLeft': rotate(-1); break;
      case 'rotateRight': rotate(1); break;
      case 'zoomIn': zoomStep(1); break;
      case 'zoomOut': zoomStep(-1); break;
      case 'zoomActual': setZoomTo(1); break;
      case 'zoomWidth': setZoomTo('width'); break;
      case 'goto': setGoto(''); break;
      case 'previousHit': nextHit(-1); break;
      case 'nextHit': nextHit(1); break;
      case 'escape': clearSelection(); setTool('select'); controller.call('findStart', { query: '' }); setHits(null); break;
      case 'pageDown': case 'down':
        if (mode === 'single') goPage(page + 1);
        else setOffset(value => controller.call('keyScroll', { action, value, max: maxOffset, height: view.h }));
        break;
      case 'pageUp': case 'up':
        if (mode === 'single') goPage(page - 1);
        else setOffset(value => controller.call('keyScroll', { action, value, max: maxOffset, height: view.h }));
        break;
      case 'nextPage': goPage(page + 1); break;
      case 'previousPage': goPage(page - 1); break;
      case 'firstPage': goPage(0); break;
      case 'lastPage': goPage(n - 1); break;
    }
  });
  const findRef = useRef(false);

  // ---- app menus ----
  useEffect(() => {
    wm.setMenus(wid, [
      { label: tr('pdf.menu.file'), items: [
        { label: tr('pdf.menu.saveCopy'), hint: '❖S', onClick: saveCopy, disabled: !doc },
        { label: tr('pdf.menu.print'), hint: '❖P', onClick: doPrint, disabled: !doc },
        { label: tr('pdf.menu.showInFiles'), onClick: () => wm.open('files', { new: true, args: { path: fs.parent(path) } }), disabled: !path },
      ] },
      { label: tr('pdf.menu.edit'), items: [
        { label: tr('pdf.menu.undo'), hint: '❖Z', onClick: undo, disabled: !annotated.length },
        { label: tr('pdf.menu.copy'), hint: '❖C', onClick: copySel, disabled: !selection?.text },
        { label: tr('pdf.menu.find'), hint: '❖F', onClick: () => { findRef.current = true; setRev((r) => r + 1); } },
      ] },
      { label: tr('pdf.menu.view'), items: [
        { label: tr('pdf.view.continuous'), checked: mode === 'continuous', onClick: () => setMode('continuous') },
        { label: tr('pdf.view.single'), checked: mode === 'single', onClick: () => setMode('single') }, '-',
        { label: tr('pdf.side.thumbs'), checked: side === 'thumbs', onClick: () => setSide((s) => (s === 'thumbs' ? null : 'thumbs')) },
        { label: tr('pdf.side.outline'), checked: side === 'outline', onClick: () => setSide((s) => (s === 'outline' ? null : 'outline')) }, '-',
        { label: tr('pdf.zoom.in'), hint: '❖+', onClick: () => zoomStep(1) },
        { label: tr('pdf.zoom.out'), hint: '❖−', onClick: () => zoomStep(-1) },
        { label: tr('pdf.zoom.actual'), hint: '❖0', onClick: () => setZoomTo(1) },
        { label: tr('pdf.zoom.width'), hint: '❖9', checked: zoom === 'width', onClick: () => setZoomTo('width') },
        { label: tr('pdf.zoom.page'), checked: zoom === 'page', onClick: () => setZoomTo('page') }, '-',
        { label: tr('pdf.rotateRight'), hint: '❖R', onClick: () => rotate(1) },
        { label: tr('pdf.rotateLeft'), hint: '❖⇧R', onClick: () => rotate(-1) },
        { label: tr('pdf.invert'), checked: invert, onClick: () => setInvert((v) => !v) },
      ] },
      { label: tr('pdf.menu.go'), items: [
        { label: tr('pdf.go.page'), hint: '❖⌥G', onClick: () => setGoto('') },
        { label: tr('pdf.go.first'), hint: 'Inicio', onClick: () => goPage(0) },
        { label: tr('pdf.go.last'), hint: 'Fin', onClick: () => goPage(n - 1) },
      ] },
    ]);
  }, [doc, mode, side, zoom, invert, annotated.length, selection?.text, path, page, n]);

  // ---- drawing ----
  const dark = th.mode === 'dark';
  const pagesShown = [];
  if (doc) {
    for (const frame of viewState.frames) {
      const { page: p, width: W, height: H } = frame;
      const e = cache.current?.get(p, W, rot, invert) || cache.current?.best(p, rot, invert, W);
      const inf = info.get(p);
      const pageHits = hits?.list.map((x, i) => ({ ...x, i })).filter((x) => x.page === p) || [];
      pagesShown.push(
        <View key={p} style={{ position: 'absolute', left: frame.left, top: frame.top, width: W, height: H, backgroundColor: invert ? '#1b1b1f' : '#ffffff',
          boxShadow: '0 2px 10px rgba(0,0,0,0.35)', overflow: 'hidden' }}>
          {e ? <DocImage image={e.image} style={{ position: 'absolute', left: 0, top: 0, width: W, height: H, objectFit: 'fill' }} /> : null}
          {/* search results */}
          {pageHits.map((x) => { const r = rectToView(x.rect, sizes[p], rot, scale); return <View key={`h${x.i}`} style={{ position: 'absolute', ...r, borderRadius: 2, backgroundColor: x.i === hits.cur ? 'rgba(255,149,0,0.55)' : 'rgba(255,214,10,0.42)', pointerEvents: 'none' }} />; })}
          {/* text selection */}
          {selection?.page === p && selection.rects.map((r, i) => { const v = rectToView(r, sizes[p], rot, scale); return <View key={`s${i}`} style={{ position: 'absolute', ...v, backgroundColor: tool === 'highlight' ? `${color}88` : 'rgba(10,132,255,0.32)', pointerEvents: 'none' }} />; })}
          {/* form fields */}
          {inf?.fields?.map((fm, i) => {
            const v = rectToView(fm.r, sizes[p], rot, scale);
            if (fm.f.type === 'other' || fm.f.type === 'push') return null;
            return <View key={`f${i}`} style={{ position: 'absolute', ...v, backgroundColor: fm.f.ro ? 'transparent' : 'rgba(10,132,255,0.10)', border: fm.f.ro ? '0 solid transparent' : '1px solid rgba(10,132,255,0.45)', pointerEvents: 'none' }} />;
          })}
          {/* existing notes */}
          {inf?.notes?.map((nt, i) => { const v = rectToView(nt.r, sizes[p], rot, scale); return <View key={`n${i}`} style={{ position: 'absolute', ...v }} />; })}
          {/* stroke in progress */}
          {ink?.page === p && ink.strokes.map((s, si) => {
            const dots = [];
            for (let k = 0; k + 1 < s.length; k += 2) {
              const r = rectToView([s[k] - 1.1, s[k + 1] - 1.1, 2.2, 2.2], sizes[p], rot, scale);
              dots.push(<View key={`${si}-${k}`} style={{ position: 'absolute', ...r, borderRadius: r.width, backgroundColor: color, pointerEvents: 'none' }} />);
            }
            return dots;
          })}
        </View>,
      );
    }
  }
  // the editor for a text field, on top of the page
  let fieldEditor = null;
  if (editField && doc) {
    const v = rectToView(editField.r, sizes[editField.page], rot, scale);
    const l = pageLeft(editField.page) + v.left, tp = pageTop(editField.page) + v.top;
    if (editField.f.type === 'text') {
      fieldEditor = (
        <View style={{ position: 'absolute', left: l, top: tp, width: Math.max(60, v.width) }}>
          <TextInput autoFocus value={editField.value} onChange={value => edit('fieldChange', { value })} onSubmit={() => commitField()}
            onCancel={() => edit('fieldCancel')} size={Math.max(10, Math.min(18, v.height * 0.6))} style={{ height: Math.max(22, v.height) }} />
        </View>
      );
    } else {
      fieldEditor = (
        <View style={{ position: 'absolute', left: l, top: tp + v.height, width: Math.max(140, v.width), maxHeight: 200, borderRadius: 6, backgroundColor: th.windowBg,
          border: `1px solid ${th.separator}`, boxShadow: '0 8px 22px rgba(0,0,0,0.35)', overflow: 'hidden' }}>
          {editField.f.items.slice(0, 20).map((it, i) => (
            <View key={i} onClick={() => commitField(i)} style={{ height: 24, paddingHorizontal: 8, justifyContent: 'center', backgroundColor: editField.f.value === i ? th.accent : 'transparent' }}>
              <Text style={{ fontSize: 12, color: editField.f.value === i ? '#fff' : th.text }}>{it}</Text>
            </View>
          ))}
        </View>
      );
    }
  }
  let noteEditor = null;
  if (noteAt && doc) {
    const v = rectToView([noteAt.x, noteAt.y, 1, 1], sizes[noteAt.page], rot, scale);
    noteEditor = (
      <View style={{ position: 'absolute', left: Math.min(view.w - 250, pageLeft(noteAt.page) + v.left), top: pageTop(noteAt.page) + v.top, width: 240, padding: 8, gap: 6, borderRadius: 8,
        backgroundColor: '#fff6b0', boxShadow: '0 8px 22px rgba(0,0,0,0.35)' }}>
        <TextInput autoFocus value={noteAt.text} onChange={value => edit('noteChange', { value })} placeholder={tr('pdf.notePlaceholder')}
          onSubmit={() => edit('noteCommit')}
          onCancel={() => edit('noteCancel')} style={{ height: 26 }} />
        <Text style={{ fontSize: 10, color: '#6b5d00' }}>{tr('pdf.noteHint')}</Text>
      </View>
    );
  }

  const barBg = dark ? 'rgba(0,0,0,0.12)' : 'rgba(0,0,0,0.03)';
  const winW = view.w + (side && doc ? 184 : 0);
  const pageLabel = n ? tr('pdf.pageOf', { page: page + 1, count: n }) : '';
  return (
    <View style={{ flex: 1 }}>
      {/* toolbar: icon-only items with tooltips, as in Preview; the narrow window drops what is also in the menus */}
      <Toolbar style={{ gap: 6 }}>
        <ToolbarButton icon="◫" title={tr('pdf.tool.sidebar')} active={!!side} onClick={() => setSide((s) => (s ? null : 'thumbs'))} />
        {goto != null ? (
          <TextInput autoFocus value={goto} onChange={setGoto} placeholder={tr('pdf.goPlaceholder')} onCancel={() => setGoto(null)}
            onSubmit={() => { const v = Number(goto); setGoto(null); if (v >= 1) goPage(v - 1); }} style={{ width: 96 }} />
        ) : (
          <View onClick={() => doc && setGoto('')} style={{ paddingHorizontal: 4, overflow: 'hidden', flexShrink: 1 }}>
            <Text style={{ fontSize: 12, color: th.textDim, whiteSpace: 'nowrap' }}>{winW < 640 ? (n ? `${page + 1} / ${n}` : '') : pageLabel}</Text>
          </View>
        )}
        <ToolbarSpacer />
        <ToolbarGroup style={{ direction: 'ltr' }}>
          <ToolbarButton icon="−" small title={tr('pdf.zoom.out')} onClick={() => zoomStep(-1)} disabled={!doc} />
          <View onClick={() => setZoomTo(zoom === 'width' ? 'page' : 'width')} style={{ minWidth: 48, alignItems: 'center', justifyContent: 'center' }}>
            <Text style={{ fontSize: 11, fontWeight: 500, color: th.text }}>{doc ? `${formatNumber(pct)} %` : '—'}</Text>
          </View>
          <ToolbarButton icon="+" small title={tr('pdf.zoom.in')} onClick={() => zoomStep(1)} disabled={!doc} />
        </ToolbarGroup>
        {winW >= 760 && <ToolbarButton icon="↻" title={tr('pdf.rotateRight')} onClick={() => rotate(1)} disabled={!doc} />}
        <ToolbarGroup>
          <ToolbarButton icon="I" small iconSize={13} title={tr('pdf.tool.select')} active={tool === 'select'} onClick={() => setTool('select')} disabled={!doc} />
          <ToolbarButton icon="▬" small iconSize={11} title={tr('pdf.tool.highlight')} active={tool === 'highlight'} onClick={() => setTool('highlight')} disabled={!doc} />
          <ToolbarButton icon="✎" small title={tr('pdf.tool.ink')} active={tool === 'ink'} onClick={() => setTool((x) => (x === 'ink' ? 'select' : 'ink'))} disabled={!doc} />
          <ToolbarButton icon="▤" small title={tr('pdf.tool.note')} active={tool === 'note'} onClick={() => setTool('note')} disabled={!doc} />
        </ToolbarGroup>
        {(tool === 'highlight' || tool === 'ink') && (
          <View style={{ flexDirection: 'row', gap: 4 }}>
            {TOOL_COLORS.map((c) => <View key={c} onClick={() => setColor(c)} style={{ width: 14, height: 14, borderRadius: 7, backgroundColor: c, border: `2px solid ${color === c ? th.text : 'transparent'}` }} />)}
          </View>
        )}
        {winW >= 760 && <ToolbarButton icon="◐" title={tr('pdf.invert')} active={invert} onClick={() => setInvert((v) => !v)} disabled={!doc} />}
        {winW >= 760 && <ToolbarButton icon="⎙" title={tr('pdf.menu.print')} onClick={doPrint} disabled={!doc} />}
        <FindField th={th} query={query} setQuery={setQuery} hits={hits} onFind={runFind} onNext={nextHit} focusReq={findRef} disabled={!doc} width={winW < 760 ? 130 : 180} />
      </Toolbar>
      <View style={{ flex: 1, flexDirection: 'row' }}>
        {side && doc && (
          <Sidebar side={side} setSide={setSide} doc={doc} sizes={sizes} page={page} onPage={goPage} cache={thumbs.current} rot={rot} invert={invert} rev={rev} th={th} />
        )}
        <View ref={area} onWheel={onWheel} onMouseDown={onDown} onContextMenu={() => {}}
          style={{ flex: 1, overflow: 'hidden', backgroundColor: dark ? '#2a2a2e' : '#d9d9de', direction: 'ltr' }}>
          {pagesShown}
          {fieldEditor}
          {noteEditor}
          {!path && <Welcome th={th} onOpen={setPath} />}
          {path && !doc && !error && !needPass && <Text style={{ margin: 40, fontSize: 13, color: th.textDim, textAlign: 'center' }}>{tr('pdf.opening')}</Text>}
          {needPass && (
            <View style={{ position: 'absolute', left: view.w / 2 - 160, top: view.h / 2 - 70, width: 320, padding: 20, gap: 12, borderRadius: 12, backgroundColor: th.windowBg, boxShadow: '0 12px 30px rgba(0,0,0,0.4)' }}>
              <Text style={{ fontSize: 15, fontWeight: 600, color: th.text }}>{tr('pdf.passwordTitle')}</Text>
              <PassField onSubmit={(p) => setPassword(p)} />
              {error ? <Text style={{ fontSize: 11, color: '#ff453a' }}>{error}</Text> : null}
            </View>
          )}
          {error && !needPass && (
            <View style={{ position: 'absolute', left: 0, right: 0, top: view.h / 2 - 40, alignItems: 'center', gap: 10 }}>
              <Text style={{ fontSize: 13, color: '#ff453a', textAlign: 'center' }}>{error}</Text>
              <Button label={tr('pdf.reopen')} onClick={() => { const p = path; setError(null); setPath(null); setTimeout(() => setPath(p), 0); }} />
            </View>
          )}
          {/* scrollbar */}
          {doc && mode === 'continuous' && maxOffset > 0 && (
            <View style={{ position: 'absolute', right: 3, width: 6, borderRadius: 3, top: (view.h - Math.max(30, view.h * view.h / lay.height)) * (offset / maxOffset),
              height: Math.max(30, view.h * view.h / lay.height), backgroundColor: dark ? 'rgba(255,255,255,0.3)' : 'rgba(0,0,0,0.3)' }} />
          )}
        </View>
      </View>
      {/* status bar */}
      <View style={{ height: 26, flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 12, backgroundColor: barBg }}>
        <View style={{ position: 'absolute', left: 0, right: 0, top: 0, height: 1, backgroundColor: th.separator }} />
        <Text style={{ flex: 1, fontSize: 11, color: msg ? th.accent : th.textDim, whiteSpace: 'nowrap', overflow: 'hidden' }}>
          {msg || (hits ? (hits.list.length ? tr('pdf.matches', { cur: hits.cur + 1, count: hits.list.length }) + (hits.done ? '' : '…') : hits.done ? tr('pdf.noMatches') : tr('pdf.searchingText'))
            : selection?.text ? tr('pdf.selected', { count: selection.text.length }) : tool === 'ink' ? tr('pdf.inkHint') : tool === 'note' ? tr('pdf.noteTool') : tool === 'highlight' ? tr('pdf.highlightHint') : doc?.info.title || '')}
        </Text>
        {editState.dirty ? <Text style={{ fontSize: 11, color: '#ff9f0a' }}>{tr('pdf.unsaved')}</Text> : null}
        <Text style={{ fontSize: 11, color: th.textDim }}>{mode === 'single' ? tr('pdf.view.single') : tr('pdf.view.continuous')}</Text>
      </View>
    </View>
  );
}

function PassField({ onSubmit }) {
  const [v, setV] = useState('');
  return <TextInput autoFocus secure value={v} onChange={setV} onSubmit={() => onSubmit(v)} placeholder={tr('pdf.password')} />;
}

function FindField({ th, query, setQuery, hits, onFind, onNext, focusReq, disabled, width = 180 }) {
  const [key, setKey] = useState(0);
  useEffect(() => { if (focusReq.current) { focusReq.current = false; setKey((k) => k + 1); } });
  useEffect(() => { const id = setTimeout(() => onFind(query), 350); return () => clearTimeout(id); }, [query]);
  if (disabled) return <View style={{ width }} />;
  return (
    <SearchField key={key} autoFocus={key > 0} value={query} onChange={setQuery} placeholder={tr('pdf.find')}
      onSubmit={() => (hits?.list.length ? onNext(1) : onFind(query))} onKeyDown={(e) => { if (e.key === 'Enter' && e.shift) { onNext(-1); return true; } return false; }}
      width={width} />
  );
}
