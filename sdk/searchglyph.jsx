// A legible search icon built from the renderer's existing vector primitives.
import { createElement as h } from 'react';
import { View } from '@lordos/runtime';
export function SearchGlyph({ size = 18, color = '#fff' }) {
  return h(View, { style: { width: size, height: size, pointerEvents: 'none' } },
    h(View, { style: { position: 'absolute', left: size * 0.06, top: size * 0.04,
      width: size * 0.65, height: size * 0.65, borderRadius: size * 0.325, border: `1.8px solid ${color}` } }),
    h(View, { style: { position: 'absolute', left: size * 0.59, top: size * 0.72,
      width: size * 0.4, height: 1.8, borderRadius: 0.9, backgroundColor: color, transform: 'rotate(45deg)' } }));
}
