// Step 2 — the system image: the latest LordOS release (signed; downloaded and cached, resumable),
// an older one, or a file of the user's own (with a clear warning: nobody vouches for it).
import { View, Text, Button, LordOSLogo, useTheme, useLocale, formatDate, typeScale, spacing } from '@lordos/sdk';
import { useState } from 'react';
import { platform } from '../platform.js';
import { app, set, loadReleases, officialImage, setOptions } from '../state.js';
import { deviceById } from '../core/catalog.js';
import { Card, Badge, Notice, Press } from './kit.jsx';
import { StepHeader } from './StepDevice.jsx';
import { formatSize } from './format.js';

function ReleaseCard({ r, selected, latest }) {
  const t = useTheme();
  const { t: tr } = useLocale();
  const cached = platform.isCached(r.tag, r.image);
  const partial = !cached && platform.partialSize(r.tag, r.image);
  return (
    <Card id={`rel-${r.tag}`} label={`LordOS ${r.version}`} selected={selected} onClick={() => set({ image: officialImage(r) })}
      style={{ flexDirection: 'row', alignItems: 'center', gap: spacing.l }}>
      <LordOSLogo size={44} />
      <View style={{ flex: 1, gap: 3 }}>
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <Text style={{ fontSize: typeScale.headline, fontWeight: 600, color: t.text }}>{`LordOS ${r.version}`}</Text>
          {latest ? <Badge tone="info">{tr('imager.image.latest')}</Badge> : null}
          {r.prerelease ? <Badge tone="warn">{tr('imager.image.beta')}</Badge> : null}
        </View>
        <Text style={{ fontSize: typeScale.caption, color: t.textDim }}>
          {tr('imager.image.meta', { date: r.date ? formatDate(new Date(r.date), 'long') : '—', size: formatSize(r.image.size) })}
        </Text>
        <View style={{ flexDirection: 'row', gap: 6, marginTop: 2, flexWrap: 'wrap' }}>
          <Badge tone="ok">{tr('imager.image.signed')}</Badge>
          {cached ? <Badge tone="dim">{tr('imager.image.cached')}</Badge> : partial ? <Badge tone="dim">{tr('imager.image.partial', { size: formatSize(partial) })}</Badge> : null}
        </View>
      </View>
    </Card>
  );
}

export function ImageStep({ s }) {
  const t = useTheme();
  const { t: tr } = useLocale();
  const dev = deviceById(s.device);
  const [picking, setPicking] = useState(false);
  const list = s.releases.list;
  const showOld = s.options.showOld;
  const shown = showOld ? list : list.slice(0, 1);
  const pickFile = async () => {
    setPicking(true);
    try {
      const info = await platform.pickImage();
      if (info) set({ image: { official: false, name: info.name, size: info.size, raw: info.raw, path: info.path, kind: info.kind } });
    } finally { setPicking(false); }
  };
  const custom = s.image && !s.image.official ? s.image : null;
  return (
    <View>
      <StepHeader title={tr('imager.image.title')} text={tr(dev.kind === 'pi' ? 'imager.image.textPi' : 'imager.image.textPc')} />
      {s.releases.state === 'loading' && !list.length ? (
        <Notice tone="info" title={tr('imager.image.loading')} />
      ) : null}
      {s.releases.state === 'error' ? (
        <Notice tone="bad" title={tr('imager.image.offlineTitle')}>{tr('imager.image.offlineText')}</Notice>
      ) : null}
      {s.releases.state === 'stale' ? (
        <Notice tone="warn" title={tr('imager.image.staleTitle')} style={{ marginBottom: spacing.m }}>{tr('imager.image.staleText')}</Notice>
      ) : null}
      <View style={{ gap: spacing.m }}>
        {shown.map((r, i) => <ReleaseCard key={r.tag} r={r} latest={i === 0} selected={!!s.image?.official && s.image.release.tag === r.tag} />)}
      </View>
      <View style={{ flexDirection: 'row', gap: spacing.s, marginTop: spacing.m, alignItems: 'center' }}>
        {list.length > 1 ? (
          <Press id="old" label={tr(showOld ? 'imager.image.hideOld' : 'imager.image.showOld', { n: list.length - 1 })} onClick={() => setOptions({ showOld: !showOld })}
            style={{ paddingVertical: 4, paddingHorizontal: 6, borderRadius: 6 }}>
            <Text style={{ fontSize: typeScale.callout, color: t.accent }}>{tr(showOld ? 'imager.image.hideOld' : 'imager.image.showOld', { n: list.length - 1 })}</Text>
          </Press>
        ) : null}
        <View style={{ flex: 1 }} />
        <Button small label={tr('imager.image.refresh')} onClick={() => loadReleases()} disabled={s.releases.state === 'loading'} />
      </View>
      <View style={{ height: 1, backgroundColor: t.separator, marginVertical: spacing.xl }} />
      <Card id="custom" label={tr('imager.image.custom')} selected={!!custom} onClick={pickFile} style={{ flexDirection: 'row', alignItems: 'center', gap: spacing.l }}>
        <View style={{ width: 44, height: 44, borderRadius: 11, backgroundColor: t.control, alignItems: 'center', justifyContent: 'center' }}>
          <Text style={{ fontSize: 20, color: t.textDim }}>⊕</Text>
        </View>
        <View style={{ flex: 1, gap: 3 }}>
          <Text style={{ fontSize: typeScale.body, fontWeight: 600, color: t.text }}>{custom ? custom.name : tr('imager.image.custom')}</Text>
          <Text style={{ fontSize: typeScale.caption, color: t.textDim }}>
            {custom ? tr('imager.image.customMeta', { size: formatSize(custom.size), raw: custom.raw ? formatSize(custom.raw) : '?' }) : tr(picking ? 'imager.image.picking' : 'imager.image.customHint')}
          </Text>
        </View>
      </Card>
      {custom ? (
        <Notice tone="warn" title={tr('imager.image.customWarnTitle')} style={{ marginTop: spacing.m }}>{tr('imager.image.customWarn')}</Notice>
      ) : null}
      {custom && custom.raw == null && custom.kind === 'xz' ? (
        <Notice tone="bad" title={tr('imager.image.damagedTitle')} style={{ marginTop: spacing.m }}>{tr('imager.image.damaged')}</Notice>
      ) : null}
    </View>
  );
}
