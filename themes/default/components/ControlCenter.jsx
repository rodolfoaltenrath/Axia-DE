// Control Center: appearance, sound and media. Network has its own menu.
import {
  View, Text, Toggle, Slider, ScrollView, screen, useSettings, wm, session, addListener, t as tr, dockGlass,
  useSound, defaultSink, sound, NowPlayingTile,
} from '@lordos/sdk';
import { useEffect, useRef } from 'react';

function Tile({ t, children, style }) {
  return <View style={[{ padding: 12, borderRadius: 12, backgroundColor: t.hover, border: `1px solid ${t.separator}` }, style]}>{children}</View>;
}

// Volume of the default output (PipeWire). Click on the speaker: mute.
function SoundTile({ t }) {
  const st = useSound();
  const out = defaultSink(st);
  if (!st.connected || !out) return null;
  const pct = Math.round(out.level * 100);
  return (
    <Tile t={t} style={{ gap: 8 }}>
      <View style={{ flexDirection: 'row', justifyContent: 'space-between' }}>
        <Text style={{ fontSize: 12, fontWeight: 600, color: t.text }}>{tr('shell.cc.sound')}</Text>
        <Text style={{ fontSize: 11, color: t.textDim, whiteSpace: 'nowrap', maxWidth: 200, overflow: 'hidden' }}>{out.description}</Text>
      </View>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8, direction: 'ltr' }}>
        <View onClick={() => sound.setMute('sink', out.index, !out.mute)} style={{ width: 26, alignItems: 'center' }}>
          <Text style={{ fontSize: 15 }}>{out.mute || pct === 0 ? '🔇' : pct < 40 ? '🔈' : pct < 75 ? '🔉' : '🔊'}</Text>
        </View>
        <Slider value={Math.min(1, out.level)} width={196} onChange={(v) => sound.setVolume('sink', out.index, v)} />
        <Text style={{ fontSize: 11, color: t.textDim, width: 34, textAlign: 'end' }}>{out.mute ? '—' : `${pct} %`}</Text>
      </View>
    </Tile>
  );
}

const inside = (inst, ref) => { for (let n = inst; n; n = n.parent) if (n === ref.current) return true; return false; };

// area: the display of the menu bar it opened from (global coordinates)
export function ControlCenter({ t, top, onClose, area = null }) {
  const ax = area ? area.x + area.w : null, ay = area ? area.y : 0;
  const [s, set] = useSettings();
  const ref = useRef(null);
  useEffect(() => {
    const pointer = addListener('pointerdown', (e) => { if (!inside(e.target, ref) && e.y > ay + top) onClose(); });
    const keys = addListener('keydown', (e) => { if (e.key === 'Escape') onClose(); });
    return () => { pointer(); keys(); };
  }, [onClose, ay, top]);

  return (
    <View ref={ref} style={{
      position: 'absolute', ...(ax == null ? { right: 10 } : { left: ax - 330 }), top: ay + top + 6, width: 320, padding: 12, maxHeight: Math.min(690, (area?.h || screen.height) - top - 24), borderRadius: 16,
      ...dockGlass(t),
      boxShadow: '0 16px 40px rgba(0,0,0,0.4)',
    }}>
      <ScrollView style={{ flexGrow: 0, flexShrink: 1, flexBasis: 'auto' }} contentStyle={{ gap: 10 }}>
        <View style={{ flexDirection: 'row', gap: 10 }}>
          <Tile t={t} style={{ flex: 1, gap: 8 }}>
            <Text style={{ fontSize: 12, fontWeight: 600, color: t.text }}>{tr('shell.cc.dark')}</Text>
            <Toggle on={s.mode === 'dark'} onChange={(on) => set({ mode: on ? 'dark' : 'light' })} />
          </Tile>
          <Tile t={t} style={{ flex: 1, gap: 8 }}>
            <Text style={{ fontSize: 12, fontWeight: 600, color: t.text }}>{tr('shell.cc.effects')}</Text>
            <Toggle on={s.effects} onChange={(effects) => set({ effects })} />
          </Tile>
        </View>
        <SoundTile t={t} />
        <NowPlayingTile />
        <Tile t={t} style={{ gap: 10 }}>
          <Text style={{ fontSize: 12, fontWeight: 600, color: t.text }}>{tr('shell.cc.accent')}</Text>
          <View style={{ flexDirection: 'row', gap: 10 }}>
            {Object.entries(t.accents).map(([name, c]) => (
              <View key={name} onClick={() => set({ accent: name })} style={{ width: 22, height: 22, borderRadius: 11,
                backgroundColor: c, border: s.accent === name ? `2px solid ${t.text}` : '0 solid transparent' }} />
            ))}
          </View>
        </Tile>
        <View style={{ flexDirection: 'row', gap: 10 }}>
          <Tile t={t} style={{ flex: 1, alignItems: 'center' }}>
            <View onClick={() => { onClose(); wm.open('settings'); }}><Text style={{ fontSize: 12, fontWeight: 600, color: t.text }}>{tr('shell.menu.settings')}</Text></View>
          </Tile>
          <Tile t={t} style={{ flex: 1, alignItems: 'center' }}>
            <View onClick={() => session.quit()}><Text style={{ fontSize: 12, fontWeight: 600, color: '#ff5f57' }}>{tr('shell.menu.logout')}</Text></View>
          </Tile>
        </View>
      </ScrollView>
    </View>
  );
}
