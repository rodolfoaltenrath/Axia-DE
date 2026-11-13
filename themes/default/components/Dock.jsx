import { dockGlass, useDockPreviews } from '@lordos/sdk';
// Dock with magnification by mouse position, tooltip and open-app dot. It can go at the bottom,
// on the left or on the right (Settings → Desktop & Dock), auto-hide and show recent
// apps. Left and right are physical sides of the screen, also in Arabic or Hebrew (as
// on popular desktop systems): the Dock is laid out with direction: 'ltr' and only the bottom row reverses its order in RTL.
//
// Customizing (docs/DOCK.md): an icon is dragged to another place (its neighbours slide aside in
// 150 ms), dragged out and held to remove it (a «poof»), or dropped from the app launcher at a
// precise position. The logic is the SDK's (sdk/dock.js: beginDockDrag, registerDockTarget, dock.*);
// this file only draws it and says where things are. The Trash is always the last item.
import {
  NativeView, View, Text, screen, appName, useLocale, addListener, t as tr, useStore, settings, dockDrag, dockPoof, beginDockDrag,
  registerDockTarget, dockDragJustEnded, refreshTrash, Layer, nativeAnim, whenAnimDone,
} from '@lordos/sdk';
import { memo, useEffect, useMemo, useRef, useState } from 'react';

const GAP = 8, PAD = 9, MARGIN = 6;
const REORDER_MS = 150;       // neighbours sliding aside while dragging
const NEAR = 60;              // dragged farther than this from the Dock: the remove cue (after a hold)
const TRASH = '__trash';

// What the Dock takes up from its screen edge (for the work area).
export const dockThickness = (size) => size + PAD * 2 + MARGIN + 4;

const launcherIcon = Object.freeze({ id: 'shell:dock-launcher' });
const trashIcon = Object.freeze({ id: 'shell:dock-trash' });
// One icon of the bar. The Dock renders on every pointer move over it (magnification) and on every
// hover change; memoized, an icon whose component and size didn't change isn't rendered again: with
// magnification only the few icons near the pointer change size.
const BarIcon = memo(function BarIcon({ Icon, size }) { return <Icon size={size} />; });
const BarTrash = memo(function BarTrash({ size, full, dark }) { return <TrashIcon size={size} full={full} dark={dark} />; });

export function LaunchpadIcon({ size }) {
  return <NativeView manifest={launcherIcon} icon={size || 32} />;
}
export function TrashIcon({ size, full, dark }) {
  return <NativeView manifest={trashIcon} icon={size || 32} args={{ full, dark }} />;
}

// Auto-hide: shows when the pointer touches the edge (after a short delay) and hides
// when it leaves the Dock. State only changes when crossing those boundaries: moving the mouse around the rest of the
// screen doesn't redraw anything. Rust owns interpolation in the native animator.
// force: shown right away (an app being dragged from the app launcher).
function useAutohide(enabled, position, rectRef, keepOpen, areaRef, force) {
  const [shown, setShown] = useState(!enabled);
  const want = useRef(!enabled);
  const keep = useRef(keepOpen);
  keep.current = keepOpen;
  const show = useRef(null);
  useEffect(() => {
    if (!enabled) { want.current = true; setShown(true); return undefined; }
    want.current = false;
    setShown(false);
    let timer = 0;
    const set = (on) => {
      if (want.current === on) return;
      want.current = on;
      setShown(on);
    };
    show.current = () => set(true);
    const off = addListener('pointermove', (e) => {
      // the edge of the display the Dock is on (the main one: docs/DISPLAYS.md)
      const A = areaRef.current, inX = e.x >= A.x && e.x < A.x + A.w, inY = e.y >= A.y && e.y < A.y + A.h;
      const edge = position === 'bottom' ? inX && e.y >= A.y + A.h - 2 : position === 'left' ? inY && e.x <= A.x + 1 : inY && e.x >= A.x + A.w - 2;
      const r = rectRef.current;
      const over = r && e.x >= r.x0 - 8 && e.x < r.x1 + 8 && e.y >= r.y0 - 8 && e.y < r.y1 + 8;
      if (!want.current) {
        if (edge && !timer) timer = setTimeout(() => { timer = 0; set(true); }, 120);
        else if (!edge && timer) { clearTimeout(timer); timer = 0; }
      } else if (!over && !keep.current) {
        if (!timer) timer = setTimeout(() => { timer = 0; if (!keep.current) set(false); }, 500);
      } else if (timer) { clearTimeout(timer); timer = 0; }
    });
    return () => { off(); clearTimeout(timer); show.current = null; };
  }, [enabled, position]);
  useEffect(() => { if (force && enabled) show.current?.(); }, [force, enabled]);
  return shown;
}

// Positions that glide to their new value in `ms` (ease-out) while `active`; otherwise they jump.
// Values are numbers by key; a key seen for the first time starts where `origin(key)` says, or at
// its target.
function useGlide(targets, active, ms, origin) {
  const cur = useRef({});
  const anim = useRef(null);                 // { from, to, t0, sig }
  const running = useRef(false);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!running.current) return undefined;
    const raf = requestAnimationFrame(() => tick((n) => n + 1));
    return () => cancelAnimationFrame(raf);
  });
  const sig = Object.keys(targets).map((k) => `${k}:${Math.round(targets[k])}`).join('|');
  const now = performance.now();
  const valueAt = (a, k) => {
    const p = Math.min(1, (now - a.t0) / ms), e = 1 - (1 - p) ** 3;
    const f = a.from[k] ?? a.to[k];
    return f + (a.to[k] - f) * e;
  };
  if (!active || ms <= 0) { anim.current = null; running.current = false; cur.current = { ...targets }; return targets; }
  if (!anim.current || anim.current.sig !== sig) {
    const from = {};
    for (const k of Object.keys(targets)) {
      const was = anim.current && k in anim.current.to ? valueAt(anim.current, k) : cur.current[k];
      from[k] = was ?? origin?.(k) ?? targets[k];
    }
    anim.current = { from, to: { ...targets }, t0: now, sig };
  }
  const out = {};
  for (const k of Object.keys(targets)) out[k] = valueAt(anim.current, k);
  cur.current = out;
  running.current = now - anim.current.t0 < ms;
  return out;
}

// Static React nodes; Rust animates the cached rasters until one completion notice.
function PoofPart({ part, size, Icon, onDone }) {
  const node = useRef(null);
  useEffect(() => {
    const anim = nativeAnim(), n = node.current?.id;
    if (!anim?.dockPoof || !n) { onDone?.(); return undefined; }
    const id = anim.dockPoof(n, size, part);
    if (!id) { onDone?.(); return undefined; }
    const cancel = onDone ? whenAnimDone(id, onDone) : () => {};
    return () => { cancel(); if (node.current?.id === n) anim.clear(n); };
  }, [part, size]);
  return <View ref={node} style={{ position: 'absolute', left: size / 2, top: size / 2,
    width: size, height: size, ...(part ? { borderRadius: size / 2, backgroundColor: 'rgba(235,238,245,0.9)' } : null) }}>
    {Icon ? <Icon size={size} /> : null}
  </View>;
}
function PoofEffect({ poof, size, reduce }) {
  const [done, setDone] = useState(!!reduce);
  useEffect(() => { if (reduce) setDone(true); }, [reduce]);
  const finish = useRef(() => setDone(true));
  if (done || reduce) return null;
  return <View style={{ position: 'absolute', left: poof.x - size, top: poof.y - size,
    width: size * 2, height: size * 2, pointerEvents: 'none' }}>
    <PoofPart part={0} size={size} Icon={poof.Icon} onDone={finish.current} />
    {[1, 2, 3, 4, 5].map((part) => <PoofPart key={part} part={part} size={size} />)}
  </View>;
}
function Poof({ poof, size, reduce }) {
  return poof ? <PoofEffect key={poof.t0} poof={poof} size={size} reduce={reduce} /> : null;
}

// The Dock stays mounted when hidden. Its native transform moves the hit box with the image.
function useDockVisibility(shown, position, distance, reduce) {
  const node = useRef(null), first = useRef(true);
  useEffect(() => {
    const anim = nativeAnim(), n = node.current?.id;
    if (!anim?.dockVisibility || !n) return undefined;
    anim.dockVisibility(n, { bottom: 0, left: 1, right: 2 }[position], distance, shown, first.current || reduce);
    first.current = false;
  }, [shown, position, distance, reduce]);
  useEffect(() => () => { const n = node.current?.id; if (n) nativeAnim()?.clear(n); }, []);
  return node;
}

export function Dock({ t, items: list, running, onOpen, onContext, onTrash, onTrashContext, trashFull = false, position = 'bottom', size = 54,
  magnify = false, maxSize = 86, autohide = false, top = 0, keepOpen = false, area = { x: 0, y: 0, w: screen.width, h: screen.height } }) {
  // area: the display it is on (global coordinates); everything below is placed inside it
  const vert = position !== 'bottom';
  const [m, setM] = useState(null);             // mouse position along the Dock's axis
  const [pressed, setPressed] = useState(null);
  const feedback = useMemo(() => lord.appearance.request(JSON.stringify({ mode: t.mode })).dockFeedback, [t.mode]);
  const { dir } = useLocale();
  const drag = useStore(dockDrag);
  const poof = useStore(dockPoof);
  const reduce = !!useStore(settings).reduceMotion;
  const dragging = !!drag;
  const own = drag?.source === 'dock' ? drag.id : null;
  // Rust owns ordering, magnification and geometry. Callable icon components
  // remain here until their apps have native views.
  const model = lord.appearance.request(JSON.stringify({ op: 'dockLayout',
    items: list.map(({ id, pinned, recent }) => ({ id, pinned: !!pinned, recent: !!recent })),
    own, dragging, gapAt: drag?.over?.index ?? null, rtl: dir === 'rtl',
    position, size, maxSize, magnify, area, top, pointer: m }));
  if (!model?.tokens) throw new Error('Dock geometry unavailable');
  const trashItem = { id: TRASH, trash: true };
  const vis = model.tokens.map(token => token.item === undefined ? token
    : { item: token.item < 0 ? trashItem : list[token.item] });
  const items = vis.filter(token => token.item).map(token => token.item);
  const n = items.length;
  const { baseSize: BASE, maxSize: MAX, centers, sizes, hovered, len, start: startT, bar, flip } = model;
  const AX = area.x, AY = area.y, AR = area.x + area.w, AB = area.y + area.h;
  const movePointer = (e) => {
    const coordinate = vert ? e.y : e.x;
    if (!magnify && !dragging && m != null) {
      const next = centers.findIndex((c) => Math.abs(coordinate - c) <= (BASE + GAP) / 2);
      if (next === hovered) return;  // Fixed icons only redraw when the hovered item changes.
    }
    setM(coordinate);
  };
  const real = { c: model.offsets };

  // glide: every icon's place on screen, the bar's start and length
  const settle = useRef(0);
  const wasDragging = useRef(false);
  const lastGhost = useRef(null);
  if (wasDragging.current && !dragging) settle.current = Date.now() + REORDER_MS + 40;
  wasDragging.current = dragging;
  if (drag) lastGhost.current = { id: drag.id, at: vert ? drag.y : drag.x };
  const targets = { __start: startT, __len: len };
  items.forEach((it, i) => { targets[it.id] = startT + real.c[i]; });
  const glide = useGlide(targets, dragging || Date.now() < settle.current, reduce ? 0 : REORDER_MS,
    (k) => (lastGhost.current && lastGhost.current.id === k ? lastGhost.current.at : undefined));
  const start = glide.__start, glen = glide.__len;
  const iconC = items.map((it) => glide[it.id] - start);

  // where the bar is on screen (for auto-hide, drops) and how far it moves when hiding
  const rect = useRef(null);
  rect.current = model.rect;
  const areaRef = useRef(area);
  areaRef.current = area;
  const external = !!drag && drag.source !== 'dock';
  const anchor = (i) => (position === 'bottom' ? { x: start + iconC[i], y: AB - MARGIN - Math.max(bar, sizes[i] + PAD) - 10 }
    : position === 'left' ? { x: AX + MARGIN + Math.max(bar, sizes[i] + PAD) + 10, y: start + iconC[i] }
      : { x: AR - MARGIN - Math.max(bar, sizes[i] + PAD) - 10, y: start + iconC[i] });
  const hit = hovered >= 0 ? items[hovered] : null;
  const previewAnchor = useRef(null);
  if (hovered >= 0) previewAnchor.current = anchor(hovered);
  const preview = useDockPreviews({ hit, anchor: previewAnchor.current, area, position, disabled: keepOpen || dragging, theme: t });
  const shown = useAutohide(autohide, position, rect, keepOpen || preview.open || hovered >= 0 || dragging, areaRef, external);
  const dockNode = useDockVisibility(shown, position, bar + MARGIN + 2, reduce);

  // Drop target (sdk/dock.js): the index among the shown pinned items, or the Trash. Computed from
  // the layout without the dragged icon and without the gap, so it doesn't move under the pointer.
  const geo = useRef(null);
  geo.current = { ...model.drop, flip, vert, rect: model.rect, BASE };
  useEffect(() => registerDockTarget({
    hit(x, y) {
      const g = geo.current, r = g.rect, slack = 24;
      if (!(x >= r.x0 - slack && x < r.x1 + slack && y >= r.y0 - slack && y < r.y1 + slack)) return null;
      const a = g.vert ? y : x;
      if (g.trashC != null && Math.abs(a - g.trashC) <= g.BASE / 2 + 2) return { trash: true };
      const before = g.rest.filter((c) => c < a).length;
      return { index: g.flip ? g.rest.length - before : before };
    },
    near(x, y) {
      const r = geo.current.rect;
      const dx = x < r.x0 ? r.x0 - x : x > r.x1 ? x - r.x1 : 0, dy = y < r.y0 ? r.y0 - y : y > r.y1 ? y - r.y1 : 0;
      return Math.hypot(dx, dy) <= NEAR;
    },
  }), []);

  // a Files drag (or any held button) over the Trash lights it up
  const [trashHover, setTrashHover] = useState(false);
  const buttonDown = useRef(false);
  useEffect(() => {
    const a = addListener('pointerdown', () => { buttonDown.current = true; });
    const b = addListener('pointerup', () => { buttonDown.current = false; setPressed(null); });
    return () => { a(); b(); };
  }, []);

  // The hit box covers only the bar until the pointer is on it; then it grows to fit the magnified
  // icons and the tooltip. A full-height box at rest would swallow clicks on windows just above the Dock.
  const BH = m != null ? MAX + PAD * 2 + 34 : bar, BW = m != null ? MAX + PAD * 2 : bar;
  const box = position === 'bottom' ? { left: start, top: AB - MARGIN - BH, width: glen, height: BH }
    : position === 'left' ? { left: AX + MARGIN, top: start, width: BW, height: glen }
      : { left: AR - MARGIN - BW, top: start, width: BW, height: glen };
  const back = position === 'bottom' ? { left: 0, right: 0, bottom: 0, height: bar }
    : position === 'left' ? { left: 0, top: 0, bottom: 0, width: bar } : { right: 0, top: 0, bottom: 0, width: bar };
  const place = (i) => (position === 'bottom' ? { left: iconC[i] - sizes[i] / 2, bottom: PAD }
    : position === 'left' ? { top: iconC[i] - sizes[i] / 2, left: PAD } : { top: iconC[i] - sizes[i] / 2, right: PAD });
  const dot = (s) => (position === 'bottom' ? { left: s / 2 - 2, bottom: -7 } : position === 'left' ? { top: s / 2 - 2, left: -7 } : { top: s / 2 - 2, right: -7 });
  const dotColor = t.mode === 'dark' ? 'rgba(255,255,255,0.85)' : 'rgba(0,0,0,0.7)';
  // where to open the right-click menu and the tooltip: beside the icon, on the inner side
  const showTip = shown && hovered >= 0 && !keepOpen && !preview.open;
  const label = !hit ? '' : hit.trash ? tr('shell.trash.name') : hit.launcher ? tr('shell.menu.apps') : appName(hit);
  const tip = (
    <View style={{ paddingHorizontal: 10, paddingVertical: 4, borderRadius: 7, backgroundColor: t.panelBg,
      backdropFilter: `blur(${t.blur}px)`, border: `1px solid ${t.windowBorder}` }}>
      <Text style={{ fontSize: 12, fontWeight: 500, color: t.text, whiteSpace: 'nowrap' }}>{label}</Text>
    </View>
  );
  // separators: halfway between the icons around them
  const sepLines = [];
  {
    let k = 0;
    for (let j = 0; j < vis.length; j++) {
      if (vis[j].item) { k++; continue; }
      if (!vis[j].sep || k === 0 || k >= n) continue;
      const mid = (iconC[k - 1] + sizes[k - 1] / 2 + iconC[k] - sizes[k] / 2) / 2;
      sepLines.push(mid);
    }
  }
  const trashLit = (drag && drag.over && drag.over.trash) || (trashHover && buttonDown.current && !drag);
  // Rust supplies the state colors; the host only binds pointer transitions.
  // Feedback remains visible with magnification and decorative effects off.
  const isPressed = (i) => hovered === i && pressed === items[i].id && !dragging && !keepOpen;
  const hoverPlate = (i) => hovered === i && !keepOpen && !dragging ? (
    <View style={{ position: 'absolute', left: -3, top: -3, right: -3, bottom: -3,
      borderRadius: sizes[i] * 0.225 + 3,
      backgroundColor: feedback[isPressed(i) ? 'pressed' : 'hover'].backgroundColor,
      pointerEvents: 'none' }} />
  ) : null;
  const ghost = drag ? (
    <View style={{ position: 'absolute', left: drag.x - BASE / 2, top: drag.y - BASE / 2, width: BASE, height: BASE, pointerEvents: 'none',
      opacity: drag.removing ? 0.55 : 0.92 }}>
      {drag.Icon ? <drag.Icon size={BASE} /> : null}
      {drag.removing && (
        <View style={{ position: 'absolute', left: BASE / 2 - 50, width: 100, top: BASE + 6, alignItems: 'center' }}>
          <View style={{ paddingHorizontal: 8, paddingVertical: 3, borderRadius: 6, backgroundColor: t.panelBg, border: `1px solid ${t.windowBorder}` }}>
            <Text style={{ fontSize: 11, fontWeight: 600, color: t.text, whiteSpace: 'nowrap' }}>{tr('shell.dock.removeCue')}</Text>
          </View>
        </View>
      )}
      {drag.removing && (
        <View style={{ position: 'absolute', right: -6, top: -6, width: 18, height: 18, borderRadius: 9, backgroundColor: 'rgba(235,238,245,0.95)',
          alignItems: 'center', justifyContent: 'center', boxShadow: '0 1px 4px rgba(0,0,0,0.35)' }}>
          <Text style={{ fontSize: 12, fontWeight: 700, color: '#4a4f5c' }}>×</Text>
        </View>
      )}
    </View>
  ) : null;

  return (
    <>
      <View style={{ position: 'absolute', left: AX, top: AY, width: area.w, height: area.h,
        overflow: 'hidden', pointerEvents: 'box-none' }}>
      <View ref={dockNode} onMouseMove={movePointer} onMouseLeave={() => { setM(null); setPressed(null); }}
        style={{ position: 'absolute', ...box, left: box.left - AX, top: box.top - AY, direction: 'ltr' }}>
        <View style={{ position: 'absolute', ...back, borderRadius: 20,
          ...dockGlass(t) }} />
        {sepLines.map((c, i) => (
          <View key={`sep${i}`} style={vert ? { position: 'absolute', top: c, height: 1, width: BASE * 0.7, [position]: PAD + BASE * 0.15, backgroundColor: t.dockBorder }
            : { position: 'absolute', left: c, width: 1, height: BASE * 0.7, bottom: PAD + BASE * 0.15, backgroundColor: t.dockBorder }} />
        ))}
        {items.map((app, i) => (app.trash ? (
          <View key={TRASH} dropTrash onClick={() => { if (!dockDragJustEnded()) onTrash?.(); }}
            onMouseDown={(e) => { if (!e.button) setPressed(TRASH); }}
            onContextMenu={() => onTrashContext?.(anchor(i))}
            onMouseEnter={() => setTrashHover(true)} onMouseLeave={() => setTrashHover(false)}
            onMouseUp={() => setTimeout(refreshTrash, 60)}
            style={{ position: 'absolute', ...place(i), width: sizes[i], height: sizes[i], borderRadius: sizes[i] * 0.22,
              opacity: isPressed(i) ? feedback.pressed.opacity : 1,
              backgroundColor: trashLit ? 'rgba(255,255,255,0.22)' : 'transparent' }}>
            {hoverPlate(i)}
            <BarTrash size={sizes[i]} full={trashFull} dark={t.mode === 'dark'} />
          </View>
        ) : (
          <View key={app.id} onClick={() => { if (!dockDragJustEnded()) onOpen(app.id); }} onContextMenu={() => onContext?.(app.id, anchor(i))}
            onMouseDown={(e) => { if (!e.button) { setPressed(app.id); beginDockDrag({ id: app.id, Icon: app.Icon, x: e.x, y: e.y, source: 'dock' }); } }}
            style={{ position: 'absolute', ...place(i), width: sizes[i], height: sizes[i], opacity: isPressed(i) ? feedback.pressed.opacity : 1 }}>
            {hoverPlate(i)}
            <BarIcon Icon={app.Icon} size={sizes[i]} />
            {running.includes(app.id) && (
              <View style={{ position: 'absolute', ...dot(sizes[i]), width: 4, height: 4, borderRadius: 2, backgroundColor: dotColor }} />
            )}
          </View>
        )))}
      </View>
      </View>
      {preview.panel}
      {/* the name tooltip (not while the icon's menu is open: the menu takes that place) */}
      <Layer name="tooltip">
        {showTip && position === 'bottom' && (
          <View style={{ position: 'absolute', left: start + iconC[hovered] - 90, width: 180, bottom: screen.height - (AB - MARGIN) + sizes[hovered] + PAD + 12,
            alignItems: 'center', pointerEvents: 'none', direction: 'ltr' }}>{tip}</View>
        )}
        {showTip && vert && (
          <View style={{ position: 'absolute', top: start + iconC[hovered] - 13, pointerEvents: 'none', direction: 'ltr',
            ...(position === 'left' ? { left: AX + MARGIN + PAD + sizes[hovered] + 14 } : { right: screen.width - AR + MARGIN + PAD + sizes[hovered] + 14 }) }}>{tip}</View>
        )}
      </Layer>
      <Layer name="drag">
        {ghost}
        <Poof poof={poof} size={BASE} reduce={reduce} />
      </Layer>
    </>
  );
}
