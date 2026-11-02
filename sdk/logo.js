// logo.js — the LordOS logo (docs/brand/README.md).
//
//   <LordOSLogo size={64} />            the app icon: the crown on the violet rounded square (About,
//                                       Settings → General, Monitor, the theme menu in other themes)
//   <LordOSGlyph size={16} color={c} /> the flat one-colour crown, for a menu bar; it follows the
//                                       light or dark ink of `color` (the UI can't tint a PNG)
//   <LordOSWordmark size={28} />        "LordOS" as live text in the brand face (fontFamily 'brand':
//                                       Plus Jakarta Sans, "Lord" ExtraBold + "OS" Medium), with the
//                                       crown in front unless crown={false}. Navy on light, white on
//                                       dark (or `color`). Always left to right, also in RTL.
//
// The images are renders of assets/brand/*.svg in assets/icons/ (tools/gen-brand.py). The icon picks
// the smallest render at least as big as it is drawn (twice as big above 32); 16 and 32 come from the pixel-grid variant.
import { createElement as h } from 'react';
import { View, Text, Image } from '@lordos/runtime';
import { useTheme } from './theme.js';

const ICON_SIZES = [16, 32, 64, 128, 256, 512, 1024];

export function LordOSLogo({ size: s = 64 }) {
  const px = ICON_SIZES.find((z) => z >= (s > 32 ? s * 2 : s)) || 1024;   // big ones stay sharp on 2x displays
  return h(View, {
    style: {
      width: s, height: s, borderRadius: s * 0.22,
      boxShadow: s > 24 ? `0 ${s * 0.04}px ${s * 0.12}px rgba(0,0,0,0.3)` : 'none',
    },
  }, h(Image, { src: `asset:icons/lordos-icon-${px}.png`, style: { width: s, height: s } }));
}

// light ink (a dark bar) → the light glyph; anything else → the dark one
function isLight(color) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(color || '').trim());
  if (!m) return /white|rgba?\(\s*2[0-9]{2}/i.test(String(color));
  const n = parseInt(m[1], 16);
  return (0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)) > 140;
}

export function LordOSGlyph({ size: s = 16, color = '#1d1d1f' }) {
  const tone = isLight(color) ? 'light' : 'dark';
  return h(Image, { src: `asset:icons/lordos-glyph-${tone}-${s <= 16 ? 16 : 32}.png`, style: { width: s, height: s } });
}

// size = the font size. The crown is 1.6 times the cap height and sits 0.27 em before the text, as
// in assets/brand/logo-horizontal-*.svg; its PNG has 13 % of empty margin above and 7 % at the sides.
export function LordOSWordmark({ size: s = 28, color, crown = true }) {
  const t = useTheme();
  const ink = color || (t.mode === 'light' ? '#12122A' : '#ffffff');
  const box = Math.round(s * 1.49);
  // two Text nodes: each box carries ~1 px of rounding on its side, so "OS" moves back 2 px to keep
  // the d–O gap of the outlined SVG (measured on VM 118 at 17 and 26 px)
  const text = (str, weight, ml) => h(Text, { style: { fontFamily: 'brand', fontWeight: weight, fontSize: s, color: ink, whiteSpace: 'nowrap', marginLeft: ml } }, str);
  return h(View, { style: { flexDirection: 'row', alignItems: 'center', direction: 'ltr' } },
    crown ? h(Image, { src: `asset:icons/lordos-crown${box > 256 ? '-512' : ''}.png`,
      style: { width: box, height: box, marginLeft: -box * 0.07, marginRight: s * 0.27 - box * 0.07 } }) : null,
    text('Lord', 800, 0), text('OS', 500, -2));
}
