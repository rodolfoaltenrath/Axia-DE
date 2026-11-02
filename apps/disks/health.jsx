// health.jsx — disk health (SMART). Read only on request, never in the background, so the disk is
// not woken up and nothing stalls.
//
// With smartmontools (Debian's smartctl) the card shows the full report: overall health and why,
// temperature and its limits, SSD wear, power-on hours and cycles, reallocated and pending
// sectors, every attribute with a plain name, the error log, the SCT temperature history and
// self-tests (short/long) with progress — all through the root helper's closed smart-* actions.
// Without it, LordOS's own reader (disk-smart) gives the basic health, and the card offers to
// install smartmontools with the package engine (pkg-install, an administrator), like Settings →
// Sharing does for SSH.
import {
  View, Text, Button, useTheme, useLocale, admin, formatBytes, formatNumber, GroupBox, FormRow, spacing, typeScale,
  smart, smartSummaries, prettyAttrName,
} from '@lordos/sdk';
import { useEffect, useRef, useState } from 'react';

const cache = new Map();          // disk → last result (while the app is open)

const HEALTH_COLOR = { ok: '#30d158', warning: '#ff9f0a', failing: '#ff453a', unknown: '#8e8e93' };
const KNOWN = [1, 3, 4, 5, 7, 9, 10, 12, 177, 183, 184, 187, 188, 190, 192, 193, 194, 196, 197, 198, 199, 231, 233, 241, 242];

function ownAttrName(tr, id) { return KNOWN.includes(id) ? tr(`disks.attr.a${id}`) : tr('disks.attr.unknown', { id }); }
// smartctl attribute: our plain name when we have one, else smartctl's own (from its drive database)
function attrName(tr, a) { return a.key ? tr(`disks.attr.${a.key}`) : prettyAttrName(a.name) || tr('disks.attr.unknown', { id: a.id }); }

// LordOS's own reader (no smartmontools): summary and ATA attributes
function readOwn(name) {
  const res = { summary: null, attrs: [] };
  const t0 = Date.now();
  return admin.run('disk-smart', name, (line) => {
    try {
      if (line.startsWith('smart ')) res.summary = JSON.parse(line.slice(6));
      else if (line.startsWith('attr ')) res.attrs.push(JSON.parse(line.slice(5)));
    } catch (e) { /* malformed line */ }
  }, { quiet: true }).then((ms) => ({ src: 'own', ...res, ms: ms || `${Date.now() - t0} ms` }));
}

function readFull(name) {
  const t0 = Date.now();
  return smart.info(name).then((m) => {
    smartSummaries.set((all) => ({ ...all, [name]: { health: m.health, reasons: m.reasons, temp: m.temp, wear: m.wear ? m.wear.used : null, hours: m.hours, standby: false, supported: m.supported, t: Date.now() } }));
    return { src: 'smartctl', m, ms: Date.now() - t0 };
  });
}

const useTr = () => useLocale().t;

function Dim({ children, color }) {
  const t = useTheme();
  return <Text style={{ fontSize: typeScale.callout, color: color || t.textDim, lineHeight: 1.4, textAlign: 'start' }}>{children}</Text>;
}

function Line({ label, value, color }) {
  const t = useTheme();
  if (value == null || value === '') return null;
  return (
    <FormRow label={label} labelWidth={190} minHeight={30}>
      <Text style={{ fontSize: typeScale.body, color: color || t.text, textAlign: 'start' }}>{value}</Text>
    </FormRow>
  );
}

function Bar({ frac, color }) {
  const t = useTheme();
  return (
    <View style={{ height: 8, borderRadius: 4, backgroundColor: t.control, overflow: 'hidden', flexDirection: 'row', direction: 'ltr' }}>
      <View style={{ flexGrow: Math.max(0, Math.min(1, frac)), backgroundColor: color || t.accent }} />
      <View style={{ flexGrow: 1 - Math.max(0, Math.min(1, frac)) }} />
    </View>
  );
}

// ---- the card ----

export function HealthCard({ disk, isAdmin, autoCheck }) {
  const t = useTheme();
  const tr = useTr();
  const [st, setSt] = useState(() => cache.get(disk.name) || null);
  const [installed, setInstalled] = useState(() => smart.installed());
  const [inst, setInst] = useState(null);            // null, { busy } or { error }
  useEffect(() => { setSt(cache.get(disk.name) || null); }, [disk.name]);
  const can = smart.smartable(disk.name) && disk.transport !== 'loop';
  const ownCan = /^(sd|hd|nvme)/.test(disk.name) && disk.transport !== 'loop';
  const save = (r) => { cache.set(disk.name, r); setSt(r); };
  const check = () => {
    setSt({ loading: true });
    (installed && can ? readFull(disk.name) : readOwn(disk.name)).then(save, (e) => save({ error: e.message }));
  };
  const install = () => {
    setInst({ busy: true });
    smart.install().then(() => {
      setInst(null);
      const ok = smart.installed();
      setInstalled(ok);
      if (ok && can) { setSt({ loading: true }); readFull(disk.name).then(save, (e) => save({ error: e.message })); }   // the full report right away
    }, (e) => setInst({ error: e.message }));
  };
  // opened from the Monitor or a notice ("Open in Disks"): the user asked for this disk's health
  useEffect(() => { if (autoCheck && ownCan && !cache.get(disk.name)) check(); }, [autoCheck]);
  const m = st && st.src === 'smartctl' ? st.m : null;
  const own = st && st.src === 'own' ? st.summary : null;
  const health = m ? m.health : own ? own.health : null;
  const head = (
    <View key="head" style={{ minHeight: 44, flexDirection: 'row', alignItems: 'center', gap: spacing.m, paddingVertical: spacing.s }}>
      {health ? <View style={{ width: 10, height: 10, borderRadius: 5, backgroundColor: HEALTH_COLOR[health] || HEALTH_COLOR.unknown }} /> : null}
      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
        <Text style={{ fontSize: typeScale.body, fontWeight: 600, color: t.text, textAlign: 'start' }}>{health ? tr(`disks.healthState.${health}`) : tr('disks.notChecked')}</Text>
        {m && !m.supported ? <Dim>{m.messages.find((x) => x.severity === 'error') ? tr('disks.smart.unsupportedBridge') : tr('disks.noSmart')}</Dim>
          : health ? <Dim>{tr(`disks.healthHint.${health}`)}</Dim>
          : !ownCan ? <Dim>{tr('disks.noSmart')}</Dim>
          : st && st.loading ? <Dim>{tr('disks.reading')}</Dim>
          : st && st.error ? <Dim color="#ff9f0a">{st.error}</Dim> : null}
        {m && m.reasons.length ? <Dim color={HEALTH_COLOR[health]}>{m.reasons.map((r) => tr(`disks.reason.${r}`, { n: reasonCount(m, r) })).join(' · ')}</Dim> : null}
      </View>
      {ownCan ? <Button small label={st && !st.loading ? tr('disks.checkAgain') : tr('disks.checkHealth')} disabled={!!(st && st.loading)} onClick={check} /> : null}
    </View>
  );
  const footer = st && st.ms != null && !st.loading ? (m ? tr('disks.smart.readWith', { ms: `${st.ms} ms` }) : tr('disks.readIn', { ms: st.ms })) : null;
  return (
    <View style={{ gap: spacing.l }}>
      <GroupBox title={tr('disks.health')} footer={footer}>
        {[head, ...(m ? fullRows(tr, m) : own ? ownRows(tr, own) : []), own && st.attrs.length ? <OwnAttrs key="oa" attrs={st.attrs} /> : null].filter(Boolean)}
      </GroupBox>
      {ownCan && !installed ? <InstallBox isAdmin={isAdmin} inst={inst} onInstall={install} /> : null}
      {m && m.supported ? <SelfTests disk={disk} m={m} isAdmin={isAdmin} onDone={check} onUpdate={(nm) => save({ ...st, m: nm })} /> : null}
      {m && m.tempHistory ? <TempHistory h={m.tempHistory} max={m.tempMax} /> : null}
      {m && m.supported ? <ErrorLog m={m} /> : null}
      {m && (m.attrs.length || m.nvme) ? <Attributes m={m} /> : null}
    </View>
  );
}

function reasonCount(m, r) {
  return r === 'reallocated' ? m.reallocated : r === 'pending' ? m.pending : r === 'uncorrectable' ? m.uncorrectable
    : r === 'mediaErrors' ? m.nvme?.mediaErrors : r === 'worn' ? m.wear?.used : r === 'grownDefects' ? m.grownDefects : 0;
}

const hoursText = (tr, h) => tr('disks.hours', { count: h, v: formatNumber(h, 0), d: formatNumber(h / 24, 0) });

function fullRows(tr, m) {
  const temp = m.temp == null ? null
    : m.tempMax != null ? tr('disks.smart.tempWithLimit', { v: m.temp, max: m.tempMax, crit: m.tempCrit ?? '—' }) : tr('disks.celsius', { v: m.temp });
  const n = m.nvme;
  return [
    <Line key="t" label={tr('disks.temperature')} value={temp} />,
    <Line key="h" label={tr('disks.powerOn')} value={m.hours != null ? hoursText(tr, m.hours) : null} />,
    <Line key="c" label={tr('disks.powerCycles')} value={m.cycles != null ? formatNumber(m.cycles, 0) : null} />,
    <Line key="w" label={tr('disks.smart.wear')} value={m.wear ? tr('disks.smart.wearValue', { v: m.wear.used }) : null} color={m.wear && m.wear.used >= 90 ? '#ff9f0a' : null} />,
    <Line key="r" label={tr('disks.reallocated')} value={m.reallocated != null ? formatNumber(m.reallocated, 0) : null} color={m.reallocated > 0 ? '#ff9f0a' : null} />,
    <Line key="p" label={tr('disks.pending')} value={m.pending != null ? formatNumber(m.pending, 0) : null} color={m.pending > 0 ? '#ff9f0a' : null} />,
    <Line key="u" label={tr('disks.uncorrectable')} value={m.uncorrectable != null ? formatNumber(m.uncorrectable, 0) : null} color={m.uncorrectable > 0 ? '#ff9f0a' : null} />,
    n ? <Line key="s" label={tr('disks.spare')} value={n.spare != null ? tr('disks.spareValue', { v: n.spare, min: n.spareThreshold ?? '—' }) : null} /> : null,
    n ? <Line key="me" label={tr('disks.mediaErrors')} value={n.mediaErrors != null ? formatNumber(n.mediaErrors, 0) : null} color={n.mediaErrors > 0 ? '#ff9f0a' : null} /> : null,
    n ? <Line key="us" label={tr('disks.unsafeShutdowns')} value={n.unsafeShutdowns != null ? formatNumber(n.unsafeShutdowns, 0) : null} /> : null,
    <Line key="dr" label={tr('disks.dataRead')} value={m.read != null ? formatBytes(m.read) : null} />,
    <Line key="dw" label={tr('disks.dataWritten')} value={m.written != null ? formatBytes(m.written) : null} />,
    <Line key="st" label={tr('disks.selfStatus')} value={m.passed === true ? tr('disks.statusOk') : m.passed === false ? tr('disks.statusFail') : null} color={m.passed === false ? '#ff453a' : null} />,
    <Line key="fw" label={tr('disks.smart.firmware')} value={m.firmware || null} />,
  ].filter((x) => x && x.props.value != null);
}

function ownRows(tr, s) {
  return [
    <Line key="t" label={tr('disks.temperature')} value={s.temp >= 0 ? tr('disks.celsius', { v: s.temp }) : null} />,
    <Line key="h" label={tr('disks.powerOn')} value={s.hours >= 0 ? hoursText(tr, s.hours) : null} />,
    <Line key="c" label={tr('disks.powerCycles')} value={s.cycles >= 0 ? formatNumber(s.cycles, 0) : null} />,
    ...(s.kind === 'ata' ? [
      <Line key="r" label={tr('disks.reallocated')} value={s.reallocated >= 0 ? formatNumber(s.reallocated, 0) : null} />,
      <Line key="p" label={tr('disks.pending')} value={s.pending >= 0 ? formatNumber(s.pending, 0) : null} />,
      <Line key="u" label={tr('disks.uncorrectable')} value={s.uncorrectable >= 0 ? formatNumber(s.uncorrectable, 0) : null} />,
      <Line key="s" label={tr('disks.selfStatus')} value={s.status === 1 ? tr('disks.statusOk') : s.status === 0 ? tr('disks.statusFail') : null} />,
    ] : [
      <Line key="l" label={tr('disks.lifeUsed')} value={tr('disks.pctValue', { v: s.used })} />,
      <Line key="sp" label={tr('disks.spare')} value={tr('disks.spareValue', { v: s.spare, min: s.spareThreshold })} />,
      <Line key="dr" label={tr('disks.dataRead')} value={formatBytes(s.read)} />,
      <Line key="dw" label={tr('disks.dataWritten')} value={formatBytes(s.written)} />,
      <Line key="me" label={tr('disks.mediaErrors')} value={formatNumber(s.mediaErrors, 0)} />,
      <Line key="us" label={tr('disks.unsafeShutdowns')} value={formatNumber(s.unsafeShutdowns, 0)} />,
    ]),
  ].filter((x) => x.props.value != null);
}

function InstallBox({ isAdmin, inst, onInstall }) {
  const tr = useTr();
  return (
    <GroupBox title={tr('disks.smart.advanced')}>
      <View style={{ paddingVertical: spacing.m, gap: spacing.s }}>
        <Dim>{tr('disks.smart.installHint')}</Dim>
        {inst && inst.error ? <Dim color="#ff453a">{inst.error}</Dim> : null}
        {!isAdmin ? <Dim>{tr('disks.smart.installAdmin')}</Dim> : null}
        <View style={{ flexDirection: 'row' }}>
          <Button small primary label={inst && inst.busy ? tr('disks.smart.installing') : tr('disks.smart.install')} disabled={!isAdmin || !!(inst && inst.busy)} onClick={onInstall} />
        </View>
      </View>
    </GroupBox>
  );
}

// ---- self-tests ----

function SelfTests({ disk, m, isAdmin, onDone, onUpdate }) {
  const t = useTheme();
  const tr = useTr();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);
  const live = useRef(m);
  live.current = m;
  const allowed = isAdmin || disk.removable;
  const running = m.selfTest.running;
  // while a test runs, ask every 15 s (smart-health: short) and read everything once it ends
  useEffect(() => {
    if (!running) return undefined;
    const id = setInterval(() => {
      smart.health(disk.name).then((h) => {
        if (h.standby) return;
        const nm = { ...live.current, selfTest: h.selfTest, selfTests: h.selfTests.length ? h.selfTests : live.current.selfTests };
        onUpdate(nm);
        if (!h.selfTest.running) onDone();
      }, () => {});
    }, 15000);
    return () => clearInterval(id);
  }, [running, disk.name]);
  const start = (kind) => {
    setBusy(true);
    setErr(null);
    smart.test(disk.name, kind).then(() => smart.health(disk.name)).then((h) => {
      onUpdate({ ...live.current, selfTest: { ...h.selfTest, running: true, remaining: h.selfTest.running ? h.selfTest.remaining : 100 } });   // the next poll says when it ended
    }, (e) => setErr(e.message)).finally(() => setBusy(false));
  };
  const abort = () => {
    setBusy(true);
    smart.abort(disk.name).then(onDone, (e) => setErr(e.message)).finally(() => setBusy(false));
  };
  const p = m.selfTest.polling;
  const rows = [
    <View key="h" style={{ paddingVertical: spacing.m, gap: spacing.s }}>
      <Dim>{tr('disks.smart.testHint')}</Dim>
      {running ? (
        <View style={{ gap: 6 }}>
          <Text style={{ fontSize: typeScale.body, color: t.text, textAlign: 'start' }}>
            {m.selfTest.remaining != null ? tr('disks.smart.testRunning', { pct: 100 - m.selfTest.remaining }) : tr('disks.smart.testRunningNoPct')}</Text>
          <Bar frac={m.selfTest.remaining != null ? (100 - m.selfTest.remaining) / 100 : 0.05} />
        </View>
      ) : null}
      {err ? <Dim color="#ff453a">{err}</Dim> : null}
      {!allowed ? <Dim>{tr('disks.adminNote')}</Dim> : null}
      {!m.selfTest.supported ? <Dim>{tr('disks.smart.testUnsupported')}</Dim> : (
        <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: spacing.s }}>
          {running ? <Button small label={tr('disks.smart.testAbort')} disabled={busy || !allowed} onClick={abort} /> : [
            <Button key="s" small label={p.short != null ? tr('disks.smart.testShortMin', { n: p.short }) : tr('disks.smart.testShort')} disabled={busy || !allowed} onClick={() => start('short')} />,
            <Button key="l" small label={p.long != null ? tr('disks.smart.testLongMin', { n: p.long }) : tr('disks.smart.testLong')} disabled={busy || !allowed} onClick={() => start('long')} />,
          ]}
        </View>
      )}
    </View>,
    ...m.selfTests.slice(0, 8).map((x, i) => (
      <FormRow key={`r${i}`} label={x.type === 'other' ? x.typeText || tr('disks.smart.testOther') : tr(`disks.smart.test.${x.type}`)} labelWidth={190} minHeight={30}>
        <Text style={{ fontSize: typeScale.body, color: x.passed ? t.text : '#ff453a', textAlign: 'start' }}>
          {`${x.resultKey ? tr(`disks.smart.nvmeResult.${x.resultKey}`) : x.result}${x.hours != null ? ` · ${tr('disks.smart.atHour', { h: formatNumber(x.hours, 0) })}` : ''}`}</Text>
      </FormRow>
    )),
  ];
  if (!m.selfTests.length) rows.push(<Line key="none" label={tr('disks.smart.testResults')} value={tr('disks.smart.testNone')} />);
  return <GroupBox title={tr('disks.smart.tests')}>{rows}</GroupBox>;
}

// ---- SCT temperature history (ATA): one thin bar per sample ----

function TempHistory({ h, max }) {
  const t = useTheme();
  const tr = useTr();
  const vals = h.values.slice(-128);
  const known = vals.filter((v) => v != null);
  const lo = Math.min(...known) - 2, hi = Math.max(...known, max || 0) + 2;
  return (
    <GroupBox title={tr('disks.smart.tempHistory')} footer={tr('disks.smart.tempHistoryFoot', { n: vals.length, min: h.interval, lo: Math.min(...known), hi: Math.max(...known) })}>
      <View style={{ height: 70, flexDirection: 'row', alignItems: 'flex-end', gap: 1, paddingVertical: spacing.s, direction: 'ltr' }}>
        {vals.map((v, i) => (
          <View key={i} style={{ flex: 1, height: v == null ? 1 : Math.max(2, ((v - lo) / Math.max(1, hi - lo)) * 54),
            backgroundColor: v == null ? t.separator : max != null && v >= max ? '#ff453a' : '#ff9f0a', borderRadius: 1 }} />
        ))}
      </View>
    </GroupBox>
  );
}

// ---- error log ----

function ErrorLog({ m }) {
  const t = useTheme();
  const tr = useTr();
  const e = m.errors;
  return (
    <GroupBox title={tr('disks.smart.errorLog')}>
      {[
        <Line key="c" label={tr('disks.smart.errorCount')} value={e.count ? formatNumber(e.count, 0) : tr('disks.smart.errorNone')} color={e.count && m.kind === 'ata' ? '#ff9f0a' : null} />,
        ...(m.kind === 'nvme' && e.count ? [<View key="n" style={{ paddingVertical: 6 }}><Dim>{tr('disks.smart.nvmeErrorsHint')}</Dim></View>] : []),
        ...e.list.slice(0, 6).map((x, i) => (
          <FormRow key={i} label={x.hours != null ? tr('disks.smart.atHour', { h: formatNumber(x.hours, 0) }) : `#${x.n ?? i + 1}`} labelWidth={190} minHeight={30}>
            <Text style={{ fontSize: typeScale.caption, color: t.text, fontFamily: 'monospace', textAlign: 'start' }}>{x.text}</Text>
          </FormRow>
        )),
      ]}
    </GroupBox>
  );
}

// ---- every attribute (ATA) or the NVMe health log ----

function Attributes({ m }) {
  const t = useTheme();
  const tr = useTr();
  const [open, setOpen] = useState(false);
  const toggle = (
    <View key="tg" onClick={() => setOpen(!open)} style={{ alignSelf: 'flex-start', paddingVertical: spacing.s }}>
      <Text style={{ fontSize: typeScale.callout, color: t.accent, textAlign: 'start' }}>
        {open ? tr('disks.hideAttrs') : tr('disks.showAttrs', { n: m.attrs.length || nvmeRows(tr, m.nvme).length })}</Text>
    </View>
  );
  if (!open) return <GroupBox title={tr('disks.smart.attributes')}>{[toggle]}</GroupBox>;
  if (!m.attrs.length) {
    return <GroupBox title={tr('disks.smart.attributes')}>{[toggle, ...nvmeRows(tr, m.nvme).map(([k, v]) => <Line key={k} label={k} value={v} />)]}</GroupBox>;
  }
  const cols = [['disks.attrName', 2.6], ['disks.attrValue', 0.8], ['disks.attrWorst', 0.8], ['disks.attrThresh', 0.9], ['disks.attrRaw', 1.6]];
  return (
    <GroupBox title={tr('disks.smart.attributes')} footer={tr('disks.prefailNote')}>
      {[toggle,
        <View key="hd" style={{ flexDirection: 'row', gap: spacing.s, paddingVertical: 4 }}>
          {cols.map(([k, f]) => <Text key={k} style={{ flex: f, fontSize: typeScale.caption, fontWeight: 600, color: t.textDim, textAlign: 'start' }}>{tr(k)}</Text>)}
        </View>,
        ...m.attrs.map((a) => (
          <View key={a.id} style={{ flexDirection: 'row', gap: spacing.s, paddingVertical: 3 }}>
            <View style={{ flex: 2.6, minWidth: 0 }}>
              <Text style={{ fontSize: typeScale.caption, color: a.failed === 'now' ? '#ff453a' : t.text, whiteSpace: 'nowrap', overflow: 'hidden', textAlign: 'start' }}>{`${a.id} · ${attrName(tr, a)}${a.prefail ? ' *' : ''}`}</Text>
              {a.key ? <Text style={{ fontSize: 10, color: t.textDim, whiteSpace: 'nowrap', overflow: 'hidden', textAlign: 'start' }}>{a.name}</Text> : null}
            </View>
            {[a.value, a.worst, a.thresh].map((v, i) => <Text key={i} style={{ flex: i === 2 ? 0.9 : 0.8, fontSize: typeScale.caption, color: t.text, textAlign: 'start' }}>{v == null ? '—' : String(v)}</Text>)}
            <Text style={{ flex: 1.6, fontSize: typeScale.caption, color: t.text, whiteSpace: 'nowrap', overflow: 'hidden', textAlign: 'start' }}>{a.raw}</Text>
          </View>
        ))]}
    </GroupBox>
  );
}

function nvmeRows(tr, n) {
  if (!n) return [];
  const num = (v) => (v == null ? null : formatNumber(v, 0));
  return [
    [tr('disks.nvme.criticalWarning'), n.criticalWarning ? `0x${n.criticalWarning.toString(16).padStart(2, '0')}` : tr('disks.nvme.none')],
    [tr('disks.temperature'), n.temp != null ? tr('disks.celsius', { v: n.temp }) : null],
    [tr('disks.nvme.sensors'), n.sensors.length ? n.sensors.map((v) => tr('disks.celsius', { v })).join(' · ') : null],
    [tr('disks.spare'), n.spare != null ? tr('disks.spareValue', { v: n.spare, min: n.spareThreshold ?? '—' }) : null],
    [tr('disks.lifeUsed'), n.used != null ? tr('disks.pctValue', { v: n.used }) : null],
    [tr('disks.dataRead'), n.read != null ? formatBytes(n.read) : null],
    [tr('disks.dataWritten'), n.written != null ? formatBytes(n.written) : null],
    [tr('disks.nvme.hostReads'), num(n.hostReads)],
    [tr('disks.nvme.hostWrites'), num(n.hostWrites)],
    [tr('disks.nvme.busy'), n.busyMinutes != null ? tr('disks.nvme.minutes', { v: num(n.busyMinutes) }) : null],
    [tr('disks.powerCycles'), num(n.cycles)],
    [tr('disks.powerOn'), n.hours != null ? hoursText(tr, n.hours) : null],
    [tr('disks.unsafeShutdowns'), num(n.unsafeShutdowns)],
    [tr('disks.mediaErrors'), num(n.mediaErrors)],
    [tr('disks.nvme.errorEntries'), num(n.errorLogEntries)],
    [tr('disks.nvme.warnTemp'), n.warningTempMinutes != null ? tr('disks.nvme.minutes', { v: num(n.warningTempMinutes) }) : null],
    [tr('disks.nvme.critTemp'), n.criticalTempMinutes != null ? tr('disks.nvme.minutes', { v: num(n.criticalTempMinutes) }) : null],
  ].filter(([, v]) => v != null);
}

// LordOS's own reader's attribute list (no smartmontools)
function OwnAttrs({ attrs }) {
  const t = useTheme();
  const tr = useTr();
  const [open, setOpen] = useState(false);
  const cols = [['disks.attrName', 2], ['disks.attrValue', 1], ['disks.attrWorst', 1], ['disks.attrThresh', 1], ['disks.attrRaw', 1.4]];
  return (
    <View style={{ paddingVertical: spacing.s, gap: 2 }}>
      <View onClick={() => setOpen(!open)} style={{ alignSelf: 'flex-start' }}>
        <Text style={{ fontSize: typeScale.callout, color: t.accent, textAlign: 'start' }}>{open ? tr('disks.hideAttrs') : tr('disks.showAttrs', { n: attrs.length })}</Text>
      </View>
      {open ? (
        <View style={{ marginTop: spacing.xs }}>
          <View style={{ flexDirection: 'row', gap: spacing.s, paddingVertical: 4 }}>
            {cols.map(([k, f]) => <Text key={k} style={{ flex: f, fontSize: typeScale.caption, fontWeight: 600, color: t.textDim, textAlign: 'start' }}>{tr(k)}</Text>)}
          </View>
          {attrs.map((a) => (
            <View key={a.id} style={{ flexDirection: 'row', gap: spacing.s, paddingVertical: 3 }}>
              <Text style={{ flex: 2, fontSize: typeScale.caption, color: a.failing ? '#ff453a' : t.text, whiteSpace: 'nowrap', overflow: 'hidden', textAlign: 'start' }}>{`${a.id} · ${ownAttrName(tr, a.id)}${a.prefail ? ' *' : ''}`}</Text>
              {[a.value, a.worst, a.thresh].map((v, i) => <Text key={i} style={{ flex: 1, fontSize: typeScale.caption, color: t.text, textAlign: 'start' }}>{String(v)}</Text>)}
              <Text style={{ flex: 1.4, fontSize: typeScale.caption, color: t.text, textAlign: 'start' }}>{String(a.raw)}</Text>
            </View>
          ))}
          <Text style={{ fontSize: typeScale.caption, color: t.textDim, marginTop: spacing.xs, textAlign: 'start' }}>{tr('disks.prefailNote')}</Text>
        </View>
      ) : null}
    </View>
  );
}
