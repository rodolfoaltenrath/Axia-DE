// Parts of the Dock drawn by the Rust native view host (the "dockNativeView" setting): the name
// tooltip and the ghost of an icon being dragged, which React redrew on every hover change and
// every pointer move of a drag. The bar, the reorder, the previews and the poof stay with the Dock.
import { createElement as h } from 'react';
import { NativeView } from './nativeview.js';
import { NATIVE_TREE_APPS } from './nativetree.js';

const text = (label, style) => ({ kind: 'text', label, style });
const view = (key, style, children) => ({ kind: 'view', key, style, children: children.filter(Boolean) });

// The name tooltip: `style` is where the Dock puts it (absolute, in the tooltip layer).
export function NativeDockTip({ t, label, style }) {
  const tree = view('tip', { paddingHorizontal: 10, paddingVertical: 4, borderRadius: 7, backgroundColor: t.panelBg,
    backdropFilter: `blur(${t.blur}px)`, border: `1px solid ${t.windowBorder}` },
  [text(label, { fontSize: 12, fontWeight: 500, color: t.text, whiteSpace: 'nowrap' })]);
  return h(NativeView, { manifest: NATIVE_TREE_APPS, args: { tree, owner: 'dock-tip' }, style: { ...style, pointerEvents: 'none' } });
}

// The icon under the pointer while it is dragged (and the cue that releasing it removes it).
export function NativeDockGhost({ t, drag, base, removeLabel }) {
  const tree = view('ghost', { width: base, height: base, opacity: drag.removing ? 0.55 : 0.92 }, [
    drag.id ? { kind: 'appIcon', app: drag.id, size: base } : null,
    drag.removing ? view('cue', { position: 'absolute', left: base / 2 - 50, width: 100, top: base + 6, alignItems: 'center' }, [
      view('cuebox', { paddingHorizontal: 8, paddingVertical: 3, borderRadius: 6, backgroundColor: t.panelBg, border: `1px solid ${t.windowBorder}` },
        [text(removeLabel, { fontSize: 11, fontWeight: 600, color: t.text, whiteSpace: 'nowrap' })])]) : null,
    drag.removing ? view('x', { position: 'absolute', right: -6, top: -6, width: 18, height: 18, borderRadius: 9, backgroundColor: 'rgba(235,238,245,0.95)',
      alignItems: 'center', justifyContent: 'center', boxShadow: '0 1px 4px rgba(0,0,0,0.35)' },
    [text('×', { fontSize: 12, fontWeight: 700, color: '#4a4f5c' })]) : null,
  ]);
  return h(NativeView, { manifest: NATIVE_TREE_APPS, args: { tree, owner: 'dock-ghost' },
    style: { position: 'absolute', left: drag.x - base / 2, top: drag.y - base / 2, width: base, height: base, pointerEvents: 'none' } });
}
