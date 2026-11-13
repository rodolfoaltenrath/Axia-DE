// File drag transport: native hit testing and descriptor I/O. No motion listener,
// React state, filesystem scan, or animation timer belongs to this adapter.
import { useEffect, useRef } from 'react';
const targets = new Map();
let sequence = 0, sourceNode = 0;
const api = () => globalThis.lord?.wayland;
export function startFileDrag(paths, origin, internalMove = false) {
  if (!origin || !api()?.dragFiles || !Array.isArray(paths) || !paths.length) return false;
  const ok = api().dragFiles(JSON.stringify(paths), origin, internalMove);
  if (ok) sourceNode = origin;
  return !!ok;
}
export function dispatchFileDrag(type, data) {
  if (type === 'dragend') { sourceNode = 0; return; }
  const handler = targets.get(data.token);
  if (handler && Array.isArray(data.paths)) handler({ paths: data.paths, internalMove: !!data.native });
}
// opt-in: files are never delivered to an arbitrary window just because it has focus.
export function useFileDrop(ref, onDrop, nativeMoveOnly = false) {
  const latest = useRef(onDrop);
  latest.current = onDrop;
  useEffect(() => {
    const id = ref.current?.id;
    if (!onDrop || !id || !api()?.dropTarget) return;
    const token = sequence = sequence % 0x7fffffff + 1;
    if (!api().dropTarget(id, nativeMoveOnly ? token | 0x80000000 : token)) return;
    targets.set(token, (data) => latest.current?.(data));
    return () => {
      targets.delete(token);
      api()?.dropTarget(id, 0);
      if (sourceNode === id) { sourceNode = 0; api()?.cancelDrag?.(); }
    };
  }, [ref, !!onDrop, nativeMoveOnly]);
}
