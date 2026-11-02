// Step 3 — the card or stick to write: only removable drives are listed, never an internal disk or
// the disk this computer runs from (the write action refuses those as well). Lumo warns that the
// drive will be erased.
import { View, Text, useTheme, useLocale, typeScale, spacing } from '@lordos/sdk';
import { useEffect, useState } from 'react';
import { platform } from '../platform.js';
import { set } from '../state.js';
import { deviceById } from '../core/catalog.js';
import { fits } from '../core/releases.js';
import { Card, Badge, Notice } from './kit.jsx';
import { DriveArt } from './art.jsx';
import { StepHeader } from './StepDevice.jsx';
import { formatSize } from './format.js';

// the smallest drive worth writing: the Pi image needs about 3 GB and then grows into the card
const MIN = { pi: 8e9, pc: 2e9 };
const BIG = 300e9;          // a removable drive this large is probably someone's backup disk

export function driveProblem(s, d) {
  const dev = deviceById(s.device);
  if (d.readOnly) return 'imager.storage.readOnly';
  if (d.size > 2 ** 41) return 'imager.storage.tooLargeMbr';          // MBR images: 2 TiB at most (and not a card)
  const need = s.image?.raw || (s.image?.official ? null : s.image?.size);
  if (need && fits(need, d.size) === 'no') return 'imager.storage.tooSmall';
  if (d.size < MIN[dev.kind]) return dev.kind === 'pi' ? 'imager.storage.tooSmallPi' : 'imager.storage.tooSmallPc';
  return null;
}

export function StorageStep({ s }) {
  const t = useTheme();
  const { t: tr } = useLocale();
  const [list, setList] = useState(() => platform.drives());
  useEffect(() => platform.watchDrives(() => setList(platform.drives())), []);
  // the chosen drive left: forget it
  useEffect(() => {
    if (s.drive && !list.some((d) => d.id === s.drive.id && d.size === s.drive.size && d.serial === s.drive.serial)) set({ drive: null });
  }, [list]);
  const hidden = platform.hiddenDriveCount?.() || 0;
  return (
    <View>
      <StepHeader title={tr('imager.storage.title')} text={tr('imager.storage.text')} />
      {!list.length ? (
        <View style={{ alignItems: 'center', gap: spacing.s, paddingVertical: 36 }}>
          <View style={{ flexDirection: 'row', gap: spacing.l, opacity: 0.6 }}><DriveArt transport="sd" size={48} /><DriveArt transport="usb" size={48} /></View>
          <Text style={{ fontSize: typeScale.headline, fontWeight: 600, color: t.text }}>{tr('imager.storage.noneTitle')}</Text>
          <Text style={{ fontSize: typeScale.callout, color: t.textDim, textAlign: 'center', maxWidth: 380, lineHeight: 1.4 }}>{tr('imager.storage.noneText')}</Text>
        </View>
      ) : (
        <View style={{ gap: spacing.m }}>
          {list.map((d) => {
            const problem = driveProblem(s, d);
            const sel = s.drive?.id === d.id;
            return (
              <Card key={d.id} id={`drive-${d.id}`} label={`${d.model}, ${formatSize(d.size)}`} selected={sel} disabled={!!problem}
                onClick={() => set({ drive: d })} style={{ flexDirection: 'row', alignItems: 'center', gap: spacing.l }}>
                <DriveArt transport={d.transport} size={40} />
                <View style={{ flex: 1, gap: 3, minWidth: 0 }}>
                  <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
                    <Text style={{ fontSize: typeScale.body, fontWeight: 600, color: t.text, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{d.model}</Text>
                    <Badge tone="dim">{tr(`imager.storage.kind.${d.transport === 'sd' ? 'sd' : 'usb'}`)}</Badge>
                  </View>
                  <Text style={{ fontSize: typeScale.caption, color: t.textDim, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                    {tr('imager.storage.meta', { size: formatSize(d.size), dev: d.name })}
                    {d.labels.length ? ` · ${d.labels.join(', ')}` : ''}
                  </Text>
                  {problem ? <Text style={{ fontSize: typeScale.caption, color: '#ff9f0a' }}>{tr(problem)}</Text>
                    : d.mounted.length ? <Text style={{ fontSize: typeScale.caption, color: t.textDim }}>{tr('imager.storage.inUse', { names: d.mounted.join(', ') })}</Text> : null}
                </View>
                <Text style={{ fontSize: typeScale.headline, fontWeight: 600, color: t.text }}>{formatSize(d.size)}</Text>
              </Card>
            );
          })}
        </View>
      )}
      {s.drive && s.drive.size > BIG ? (
        <Notice tone="warn" title={tr('imager.storage.bigTitle')} style={{ marginTop: spacing.m }}>{tr('imager.storage.bigText', { size: formatSize(s.drive.size) })}</Notice>
      ) : null}
      {s.drive ? (
        <Notice tone="bad" title={tr('imager.storage.eraseTitle', { name: s.drive.model })} style={{ marginTop: spacing.xl }}>
          {tr('imager.storage.eraseText', { size: formatSize(s.drive.size) })}
        </Notice>
      ) : null}
      <Text style={{ fontSize: typeScale.caption, color: t.textDim, marginTop: spacing.xl, lineHeight: 1.45 }}>
        {tr(hidden ? 'imager.storage.hiddenSome' : 'imager.storage.hidden', { n: hidden })}
      </Text>
    </View>
  );
}
