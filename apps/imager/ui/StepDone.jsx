// Done — Lumo celebrates; what to do next with the card, and how to find the board on the network.
import { View, Text, Button, ScrollView, Lumo, useTheme, useLocale, typeScale, spacing } from '@lordos/sdk';
import { useEffect, useRef } from 'react';
import { platform } from '../platform.js';
import { app, restart } from '../state.js';
import { deviceById } from '../core/catalog.js';
import { Group, Badge } from './kit.jsx';
import { formatSize } from './format.js';

function StepRow({ n, title, text, mono }) {
  const t = useTheme();
  return (
    <View style={{ flexDirection: 'row', gap: spacing.m, paddingVertical: 10 }}>
      <View style={{ width: 22, height: 22, borderRadius: 11, backgroundColor: t.accent, alignItems: 'center', justifyContent: 'center' }}>
        <Text style={{ fontSize: 11, fontWeight: 700, color: '#fff' }}>{String(n)}</Text>
      </View>
      <View style={{ flex: 1, gap: 3 }}>
        <Text style={{ fontSize: typeScale.body, fontWeight: 600, color: t.text }}>{title}</Text>
        {text ? <Text style={{ fontSize: typeScale.callout, color: t.textDim, lineHeight: 1.45, fontFamily: mono ? 'monospace' : undefined }}>{text}</Text> : null}
      </View>
    </View>
  );
}

export function DoneStep({ s }) {
  const t = useTheme();
  const { t: tr } = useLocale();
  const dev = deviceById(s.device);
  const r = s.job?.result || {};
  const host = `${s.cfg.hostname || 'lordos-pi'}.local`;
  const announced = useRef(false);
  useEffect(() => {
    if (announced.current) return;
    announced.current = true;
    platform.done({ title: tr('imager.done.notifyTitle'), body: tr('imager.done.notifyBody', { name: s.drive?.model || '' }), sound: s.options.sound });
  }, []);
  const pi = dev.kind === 'pi';
  const custom = s.customise && dev.customise;
  return (
    <ScrollView style={{ flex: 1 }} contentStyle={{ paddingHorizontal: 28, paddingTop: 26, paddingBottom: 26 }}>
      <View style={{ alignItems: 'center', gap: 8, marginBottom: spacing.xl }}>
        <Lumo pose="celebrate" size={128} />
        <Text style={{ fontSize: typeScale.largeTitle, fontWeight: 600, color: t.text, textAlign: 'center' }}>{tr('imager.done.title')}</Text>
        <Text style={{ fontSize: typeScale.body, color: t.textDim, textAlign: 'center', maxWidth: 520, lineHeight: 1.45 }}>
          {tr(r.ejected ? 'imager.done.ejected' : 'imager.done.notEjected', { name: s.drive?.model || '' })}
        </Text>
        <View style={{ flexDirection: 'row', gap: 6, flexWrap: 'wrap', justifyContent: 'center' }}>
          <Badge tone="ok">{tr('imager.done.verified')}</Badge>
          {s.image?.official ? <Badge tone="ok">{tr('imager.image.signed')}</Badge> : <Badge tone="warn">{tr('imager.done.custom')}</Badge>}
          {r.mbps ? <Badge tone="dim">{tr('imager.done.stats', { s: r.seconds || 0, speed: `${formatSize(r.mbps * 1e6)}/s` })}</Badge> : null}
        </View>
      </View>
      <Group title={tr('imager.done.next')}>
        {pi ? (
          <>
            <StepRow n={1} title={tr('imager.done.pi1')} text={tr('imager.done.pi1Text')} />
            <StepRow n={2} title={tr('imager.done.pi2')} text={tr(custom ? 'imager.done.pi2Custom' : 'imager.done.pi2Setup')} />
            <StepRow n={3} title={tr('imager.done.pi3')} text={tr('imager.done.pi3Text', { host })} />
            {custom && s.cfg.ssh !== 'off' && s.cfg.user.name ? <StepRow n={4} title={tr('imager.done.ssh')} text={`ssh ${s.cfg.user.name}@${host}`} mono /> : null}
            {custom && s.cfg.remote ? <StepRow n={custom && s.cfg.ssh !== 'off' ? 5 : 4} title={tr('imager.done.remote')} text={`https://${host}:5673`} mono /> : null}
          </>
        ) : (
          <>
            <StepRow n={1} title={tr('imager.done.pc1')} text={tr('imager.done.pc1Text')} />
            <StepRow n={2} title={tr('imager.done.pc2')} text={tr('imager.done.pc2Text')} />
          </>
        )}
      </Group>
      {r.sha256 ? (
        <Text style={{ fontSize: typeScale.caption, color: t.textDim, marginTop: spacing.l, fontFamily: 'monospace' }}>{`SHA-256 ${r.sha256}`}</Text>
      ) : null}
      <View style={{ flexDirection: 'row', justifyContent: 'center', gap: spacing.s, marginTop: spacing.xl }}>
        <Button label={tr('imager.done.another')} onClick={() => restart({ keepCfg: true })} />
        <Button primary label={tr('imager.done.newSetup')} onClick={() => restart({ keepCfg: false })} />
      </View>
    </ScrollView>
  );
}
