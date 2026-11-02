// glyphs.jsx — the icons of Passwords, drawn with views (no color emoji, see docs/DESIGN.md):
// source-list glyphs in the accent color, white glyphs on the colored category tiles, and the
// eye / clip / clock / shield shapes used by the detail pane and the dialogs.
// A category stores the name of its glyph ("globe", "key"…); categories saved by older versions
// stored an emoji, which iconKind() maps to the closest glyph.
import { View, Text, useTheme } from '@lordos/sdk';

// The glyphs a user category can pick (Settings → New category), in picker order.
export const GLYPH_KINDS = ['folder', 'globe', 'mail', 'bank', 'card', 'network', 'wifi', 'terminal', 'database', 'apps', 'key', 'braces',
  'id', 'coin', 'shield', 'note', 'lock', 'star', 'person', 'briefcase', 'clock', 'drive'];

const LEGACY = {
  '🗂': 'folder', '📁': 'folder', '📂': 'folder', '🏠': 'folder', '🌐': 'globe', '✉': 'mail', '📧': 'mail', '🏦': 'bank', '💰': 'bank', '💳': 'card',
  '🖧': 'network', '📡': 'network', '📶': 'wifi', '🖥': 'terminal', '💻': 'terminal', '🗄': 'database', '📱': 'apps', '🎮': 'apps', '🔑': 'key',
  '🗝': 'key', '🧩': 'braces', '🪪': 'id', '🪙': 'coin', '🛡': 'shield', '📝': 'note', '📄': 'note', '🔒': 'lock', '🔐': 'lock', '⭐': 'star',
  '👤': 'person', '🐧': 'key', '💼': 'briefcase', '🎓': 'briefcase', '🛒': 'briefcase', '✈': 'briefcase', '🏥': 'shield', '⚙': 'apps',
  '📷': 'apps', '🎵': 'apps', '📦': 'drive', '💾': 'drive',
};

// The glyph of a stored icon value (a glyph name, or an emoji from an older version).
export function iconKind(icon, fallback = 'folder') {
  const s = String(icon || '');
  if (GLYPH_KINDS.includes(s)) return s;
  const bare = s.replace(/️/g, '');
  return LEGACY[bare] || fallback;
}

export function Glyph({ kind, size = 16, color }) {
  const th = useTheme();
  const c = color || th.accent;
  const s = size;
  const w = Math.max(1.5, Math.round(s / 11 * 2) / 2);    // stroke
  const box = (children, extra) => (
    <View style={{ width: s, height: s, alignItems: 'center', justifyContent: 'center', ...extra }}>{children}</View>
  );
  const line = (style) => <View style={{ position: 'absolute', backgroundColor: c, ...style }} />;
  switch (kind) {
    case 'lock':
      return box(<>
        <View style={{ width: s * 0.5, height: s * 0.42, marginBottom: -w, borderTopLeftRadius: s * 0.25, borderTopRightRadius: s * 0.25,
          borderTopWidth: w, borderLeftWidth: w, borderRightWidth: w, borderColor: c }} />
        <View style={{ width: s * 0.74, height: s * 0.48, borderRadius: s * 0.1, backgroundColor: c }} />
      </>);
    case 'star':
      return box(<Text style={{ fontSize: s * 0.92, lineHeight: s, color: c }}>★</Text>);
    case 'archive':
      return box(<>
        <View style={{ width: s * 0.9, height: s * 0.2, borderRadius: 1.5, backgroundColor: c, marginBottom: 1 }} />
        <View style={{ width: s * 0.76, height: s * 0.48, borderBottomLeftRadius: s * 0.1, borderBottomRightRadius: s * 0.1, border: `${w}px solid ${c}`, alignItems: 'center' }}>
          <View style={{ marginTop: s * 0.1, width: s * 0.28, height: w, backgroundColor: c }} />
        </View>
      </>);
    case 'trash':
      return box(<>
        <View style={{ width: s * 0.3, height: s * 0.1, borderTopLeftRadius: 1.5, borderTopRightRadius: 1.5, backgroundColor: c }} />
        <View style={{ width: s * 0.8, height: w, backgroundColor: c, marginBottom: 1 }} />
        <View style={{ width: s * 0.6, height: s * 0.54, borderBottomLeftRadius: s * 0.1, borderBottomRightRadius: s * 0.1, border: `${w}px solid ${c}` }} />
      </>);
    case 'folder':
      return box(
        <View style={{ width: s * 0.86, height: s * 0.66 }}>
          <View style={{ width: s * 0.36, height: s * 0.16, borderTopLeftRadius: s * 0.08, borderTopRightRadius: s * 0.08, backgroundColor: c }} />
          <View style={{ flex: 1, borderRadius: s * 0.1, borderTopLeftRadius: 0, border: `${w}px solid ${c}` }} />
        </View>,
      );
    case 'clock':
      return box(
        <View style={{ width: s * 0.84, height: s * 0.84, borderRadius: s * 0.42, border: `${w}px solid ${c}` }}>
          {line({ left: s * 0.42 - w * 1.5, top: s * 0.16, width: w, height: s * 0.28 })}
          {line({ left: s * 0.42 - w * 1.5, top: s * 0.42 - w * 1.5, width: s * 0.22, height: w })}
        </View>,
      );
    case 'wifi':
      return box(
        <View style={{ flexDirection: 'row', alignItems: 'flex-end', gap: s * 0.1, height: s * 0.76, direction: 'ltr' }}>
          {[0.32, 0.54, 0.76].map((h) => <View key={h} style={{ width: s * 0.2, height: s * h, borderRadius: 1.5, backgroundColor: c }} />)}
        </View>,
      );
    case 'note':
      return box(
        <View style={{ width: s * 0.7, height: s * 0.86, borderRadius: s * 0.1, border: `${w}px solid ${c}`, paddingTop: s * 0.14, paddingHorizontal: s * 0.1, gap: s * 0.1 }}>
          {[1, 0.8, 0.55].map((f) => <View key={f} style={{ width: `${f * 100}%`, height: w, backgroundColor: c }} />)}
        </View>,
      );
    case 'card':
      return box(
        <View style={{ width: s * 0.9, height: s * 0.64, borderRadius: s * 0.1, border: `${w}px solid ${c}`, overflow: 'hidden' }}>
          <View style={{ marginTop: s * 0.1, height: s * 0.14, backgroundColor: c }} />
        </View>,
      );
    case 'shield':
      return box(
        <View style={{ width: s * 0.7, height: s * 0.84, borderTopLeftRadius: s * 0.08, borderTopRightRadius: s * 0.08,
          borderBottomLeftRadius: s * 0.35, borderBottomRightRadius: s * 0.35, border: `${w}px solid ${c}` }} />,
      );
    case 'person':
      return box(<>
        <View style={{ width: s * 0.4, height: s * 0.4, borderRadius: s * 0.2, backgroundColor: c, marginBottom: s * 0.06 }} />
        <View style={{ width: s * 0.76, height: s * 0.34, borderTopLeftRadius: s * 0.38, borderTopRightRadius: s * 0.38, backgroundColor: c }} />
      </>);
    case 'globe':
      return box(
        <View style={{ width: s * 0.86, height: s * 0.86, borderRadius: s * 0.43, border: `${w}px solid ${c}`, alignItems: 'center', justifyContent: 'center' }}>
          <View style={{ width: s * 0.36, height: s * 0.86 - w * 2, borderRadius: s * 0.18, border: `${w}px solid ${c}` }} />
          {line({ left: 0, right: 0, top: s * 0.43 - w * 1.5, height: w })}
        </View>,
      );
    case 'mail':
      return box(
        <View style={{ width: s * 0.9, height: s * 0.64, borderRadius: s * 0.08, border: `${w}px solid ${c}`, overflow: 'hidden', alignItems: 'center' }}>
          <View style={{ position: 'absolute', top: -s * 0.34, width: s * 0.6, height: s * 0.6, borderRadius: s * 0.06, border: `${w}px solid ${c}`, transform: 'rotate(45deg)' }} />
        </View>,
      );
    case 'bank':
      return box(<>
        <View style={{ width: s * 0.84, height: s * 0.16, borderTopLeftRadius: s * 0.3, borderTopRightRadius: s * 0.3, backgroundColor: c }} />
        <View style={{ flexDirection: 'row', gap: s * 0.12, height: s * 0.4, marginVertical: s * 0.04 }}>
          {[0, 1, 2].map((i) => <View key={i} style={{ width: s * 0.12, backgroundColor: c }} />)}
        </View>
        <View style={{ width: s * 0.84, height: s * 0.12, backgroundColor: c }} />
      </>);
    case 'network':
      return box(<>
        <View style={{ width: s * 0.36, height: s * 0.26, borderRadius: s * 0.05, backgroundColor: c }} />
        <View style={{ width: w, height: s * 0.12, backgroundColor: c }} />
        <View style={{ width: s * 0.66, height: w, backgroundColor: c }} />
        <View style={{ flexDirection: 'row', width: s * 0.9, justifyContent: 'space-between' }}>
          {[0, 1].map((i) => <View key={i} style={{ alignItems: 'center' }}>
            <View style={{ width: w, height: s * 0.1, backgroundColor: c }} />
            <View style={{ width: s * 0.32, height: s * 0.22, borderRadius: s * 0.05, backgroundColor: c }} />
          </View>)}
        </View>
      </>);
    case 'terminal':
      return box(
        <View style={{ width: s * 0.9, height: s * 0.72, borderRadius: s * 0.1, border: `${w}px solid ${c}`, justifyContent: 'center', paddingInlineStart: s * 0.08, direction: 'ltr' }}>
          <Text style={{ fontSize: s * 0.42, lineHeight: s * 0.5, fontWeight: 700, color: c }}>{'>_'}</Text>
        </View>,
      );
    case 'database':
      return box(
        <View style={{ gap: s * 0.04 }}>
          {[0, 1, 2].map((i) => <View key={i} style={{ width: s * 0.72, height: s * 0.24, borderRadius: s * 0.12, border: `${w}px solid ${c}` }} />)}
        </View>,
      );
    case 'apps':
      return box(
        <View style={{ width: s * 0.78, flexDirection: 'row', flexWrap: 'wrap', gap: s * 0.1 }}>
          {[0, 1, 2, 3].map((i) => <View key={i} style={{ width: s * 0.34, height: s * 0.34, borderRadius: s * 0.08, backgroundColor: c }} />)}
        </View>,
      );
    case 'key':
      return box(
        <View style={{ flexDirection: 'row', alignItems: 'center', direction: 'ltr' }}>
          <View style={{ width: s * 0.4, height: s * 0.4, borderRadius: s * 0.2, border: `${w * 1.3}px solid ${c}` }} />
          <View style={{ width: s * 0.46, height: w * 1.3, backgroundColor: c }}>
            <View style={{ position: 'absolute', right: 0, top: 0, width: w * 1.3, height: s * 0.2, backgroundColor: c }} />
            <View style={{ position: 'absolute', right: s * 0.12, top: 0, width: w * 1.3, height: s * 0.14, backgroundColor: c }} />
          </View>
        </View>,
      );
    case 'braces':
      return box(<Text style={{ fontSize: s * 0.62, lineHeight: s * 0.8, fontWeight: 700, color: c, direction: 'ltr' }}>{'{ }'}</Text>);
    case 'id':
      return box(
        <View style={{ width: s * 0.9, height: s * 0.64, borderRadius: s * 0.1, border: `${w}px solid ${c}`, flexDirection: 'row', alignItems: 'center',
          paddingHorizontal: s * 0.08, gap: s * 0.08, direction: 'ltr' }}>
          <View style={{ width: s * 0.22, height: s * 0.22, borderRadius: s * 0.11, backgroundColor: c }} />
          <View style={{ flex: 1, gap: s * 0.08 }}>
            <View style={{ height: w, backgroundColor: c }} />
            <View style={{ width: '70%', height: w, backgroundColor: c }} />
          </View>
        </View>,
      );
    case 'coin':
      return box(
        <View style={{ width: s * 0.84, height: s * 0.84, borderRadius: s * 0.42, border: `${w}px solid ${c}`, alignItems: 'center', justifyContent: 'center' }}>
          <View style={{ width: s * 0.44, height: s * 0.44, borderRadius: s * 0.22, border: `${w}px solid ${c}` }} />
        </View>,
      );
    case 'briefcase':
      return box(<>
        <View style={{ width: s * 0.34, height: s * 0.16, borderTopLeftRadius: s * 0.06, borderTopRightRadius: s * 0.06, borderTopWidth: w, borderLeftWidth: w, borderRightWidth: w, borderColor: c }} />
        <View style={{ width: s * 0.86, height: s * 0.56, borderRadius: s * 0.1, backgroundColor: c }} />
      </>);
    case 'drive':
      return box(<>
        <View style={{ width: s * 0.3, height: s * 0.2, borderTopLeftRadius: s * 0.04, borderTopRightRadius: s * 0.04, border: `${w}px solid ${c}`, marginBottom: -w }} />
        <View style={{ width: s * 0.5, height: s * 0.62, borderRadius: s * 0.1, backgroundColor: c }} />
      </>);
    case 'eye':
    case 'eyeOff':
      return box(<>
        <View style={{ width: s * 0.9, height: s * 0.56, borderRadius: s * 0.28, border: `${w}px solid ${c}`, alignItems: 'center', justifyContent: 'center' }}>
          <View style={{ width: s * 0.26, height: s * 0.26, borderRadius: s * 0.13, backgroundColor: c }} />
        </View>
        {kind === 'eyeOff' ? <View style={{ position: 'absolute', width: s * 1.0, height: w, backgroundColor: c, transform: 'rotate(-40deg)' }} /> : null}
      </>);
    case 'clip':
      return box(
        <View style={{ width: s * 0.34, height: s * 0.86, borderRadius: s * 0.17, border: `${w}px solid ${c}`, transform: 'rotate(40deg)' }} />,
      );
    case 'image':
      return box(
        <View style={{ width: s * 0.9, height: s * 0.72, borderRadius: s * 0.1, border: `${w}px solid ${c}`, overflow: 'hidden', direction: 'ltr' }}>
          <View style={{ position: 'absolute', left: s * 0.14, top: s * 0.12, width: s * 0.18, height: s * 0.18, borderRadius: s * 0.09, backgroundColor: c }} />
          <View style={{ position: 'absolute', left: s * 0.2, top: s * 0.36, width: s * 0.5, height: s * 0.5, backgroundColor: c, transform: 'rotate(45deg)' }} />
        </View>,
      );
    default:
      return box(null);
  }
}

// A category's tile: its color as a soft vertical gradient and its glyph in white (as the
// badges of Settings).
export function Tile({ kind, color = '#8e8e93', size = 20, radius }) {
  return (
    <View style={{ width: size, height: size, borderRadius: radius ?? size * 0.26, alignItems: 'center', justifyContent: 'center',
      background: `linear-gradient(180deg, ${tint(color, 0.18)}, ${tint(color, -0.12)})` }}>
      <Glyph kind={kind} size={Math.round(size * 0.62)} color="#ffffff" />
    </View>
  );
}

export function tint(hex, amt) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(hex || ''));
  if (!m) return hex;
  const n = parseInt(m[1], 16);
  const ch = (v) => Math.max(0, Math.min(255, Math.round(v + (amt < 0 ? v * amt : (255 - v) * amt))));
  const r = ch(n >> 16), g = ch((n >> 8) & 255), b = ch(n & 255);
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, '0')}`;
}
