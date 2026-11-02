// editor.jsx — the event sheet (new event, details and editing) and the calendar sheet
// (new calendar or edit name and color).
import { View, Text, useTheme, Button, Toggle, TextInput, ScrollView, calendars, calendarEvents, t } from '@lordos/sdk';
import { useRef, useState } from 'react';
import {
  RED, COLORS, sod, addDays, ymd, parseYmd, minutesOf, atMinutes, Sheet, Field, Choice, TextArea, DateField, TimeField, ColorSwatches,
  Popover, MenuList,
} from './ui.jsx';

const ALERTS = [0, 5, 10, 15, 30, 60, 1440];
const FREQS = ['DAILY', 'WEEKLY', 'MONTHLY', 'YEARLY'];

// the first occurrence of a series (saving moves the whole series by the change made to this one)
async function seriesStart(ev) {
  try {
    const list = await calendarEvents.range(0, ev.occ + 1, [ev.cal]);
    const mine = list.filter((x) => x.uid === ev.uid && !x.rid).map((x) => x.start);
    return mine.length ? Math.min(...mine) : ev.start;
  } catch (e) { return ev.start; }
}

function draftOf(ev, cals) {
  const allDay = !!ev.allDay;
  const s = allDay ? parseYmd(ev.sdate) || sod(ev.start) : new Date(ev.start);
  const eRaw = allDay ? parseYmd(ev.edate) || sod(ev.end) : new Date(ev.end);
  const eDay = allDay ? addDays(eRaw, -1) : sod(eRaw);
  return {
    title: ev.title || '', loc: ev.loc || '', notes: ev.notes || '', allDay,
    sDay: sod(s), sMin: allDay ? 9 * 60 : minutesOf(s), eDay: eDay < sod(s) ? sod(s) : eDay, eMin: allDay ? 10 * 60 : minutesOf(eRaw),
    rrule: ev.rrule || '', alarm: ev.alarms && ev.alarms.length ? ev.alarms[0] : -1, cal: ev.cal || (cals[0] && cals[0].id) || '',
  };
}

function psInfo(ps) {
  const p = String(ps || '').toUpperCase();
  if (p === 'ACCEPTED') return ['✓', '#34c759', t('calendar.ps.accepted')];
  if (p === 'DECLINED') return ['✕', RED, t('calendar.ps.declined')];
  if (p === 'TENTATIVE') return ['?', '#ff9500', t('calendar.ps.tentative')];
  if (p === 'DELEGATED') return ['→', '#8e8e93', t('calendar.ps.delegated')];
  return ['•', '#8e8e93', t('calendar.ps.pending')];
}

export function EventSheet({ ev, isNew, cals, onClose }) {
  const th = useTheme();
  const [d, setD] = useState(() => draftOf(ev, cals));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [askDelete, setAskDelete] = useState(false);
  const delRef = useRef(null);
  const set = (patch) => setD((x) => ({ ...x, ...patch }));
  const recurring = !isNew && !!ev.rrule && !!ev.occ;

  // moving the start keeps the duration
  const setStartDay = (day) => setD((x) => ({ ...x, sDay: sod(day), eDay: addDays(sod(day), Math.round((x.eDay - x.sDay) / 86400000)) }));
  const setStartMin = (m) => setD((x) => {
    const sameDayEvent = +x.eDay === +x.sDay;
    let eMin = x.eMin + (m - x.sMin);
    if (!sameDayEvent || eMin >= 1440 || eMin <= m) eMin = sameDayEvent ? Math.min(1439, m + 60) : x.eMin;
    return { ...x, sMin: m, eMin };
  });

  const save = async () => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      let start, end;
      if (d.allDay) {
        start = sod(d.sDay).getTime();
        end = addDays(d.eDay < d.sDay ? d.sDay : d.eDay, 1).getTime();
      } else {
        start = atMinutes(d.sDay, d.sMin).getTime();
        end = atMinutes(d.eDay < d.sDay ? d.sDay : d.eDay, d.eMin).getTime();
        if (end <= start) end = start + 3600000;
      }
      if (recurring) {
        const first = await seriesStart(ev);
        const len = end - start;
        start = first + (start - ev.start);
        end = start + len;
      }
      let cal = d.cal;
      if (!cal) cal = (await calendars.create({ name: t('calendar.defaultName'), color: '#007aff', events: true, todos: false })).uid;
      const rest = (ev.alarms || []).slice(1);
      const event = {
        title: d.title.trim(), loc: d.loc.trim(), notes: d.notes, allDay: d.allDay, start, end, rrule: d.rrule,
        alarms: d.alarm >= 0 ? [d.alarm, ...rest] : [],
      };
      if (!isNew) event.uid = ev.uid;
      if (ev.url) event.url = ev.url;
      if (ev.status) event.status = ev.status;
      if (!d.allDay && ev.tz) event.tz = ev.tz;
      if (d.allDay) { event.sdate = ymd(new Date(start)); event.edate = ymd(new Date(end)); }
      if (ev.att && ev.att.length) {
        event.att = ev.att;
        if (ev.org) { event.orgEmail = ev.org.email; event.orgName = ev.org.name; }
      }
      await calendarEvents.save(cal, event, !isNew && ev.cal && ev.cal !== cal ? { from: ev.cal } : {});
      onClose();
    } catch (e) {
      setError(t('calendar.saveFailed'));
      setBusy(false);
    }
  };

  const remove = async (opts) => {
    try { await calendarEvents.remove(ev.cal, ev.uid, opts); onClose(); } catch (e) { setError(t('calendar.deleteFailed')); }
  };

  const freq = (/FREQ=(\w+)/.exec(d.rrule) || [])[1] || '';
  const custom = d.rrule && d.rrule !== `FREQ=${freq}`;
  const repeatOpts = [['', t('calendar.repeat.none')], ...FREQS.map((f) => [`FREQ=${f}`, t(`calendar.repeat.${f.toLowerCase()}`)])];
  if (custom) repeatOpts.push([d.rrule, t('calendar.repeat.custom')]);
  const alertOpts = [[-1, t('calendar.alert.none')], ...ALERTS.map((m) => [m, m === 0 ? t('calendar.alert.atTime')
    : m === 1440 ? t('calendar.alert.dayBefore') : m === 60 ? t('calendar.alert.hourBefore') : t('calendar.alert.minutes', { count: m })])];
  if (d.alarm >= 0 && !ALERTS.includes(d.alarm)) alertOpts.push([d.alarm, t('calendar.alert.minutes', { count: d.alarm })]);
  const calOpts = cals.map((c) => [c.id, c.name, c.color]);
  const att = ev.att || [];

  return (
    <Sheet width={500} title={isNew ? t('calendar.newEvent') : null} footer={[
      !isNew ? (
        <View key="del" ref={delRef}>
          <Button destructive label={t('calendar.delete')} onClick={() => (recurring ? setAskDelete(true) : remove({}))} />
          {askDelete && (
            <Popover anchor={delRef} width={240} onClose={() => setAskDelete(false)}>
              <MenuList onDone={() => setAskDelete(false)} items={[
                { label: t('calendar.deleteThis'), destructive: true, onClick: () => remove({ occ: ev.occ }) },
                { label: t('calendar.deleteAll'), destructive: true, onClick: () => remove({ all: true }) },
              ]} />
            </Popover>
          )}
        </View>
      ) : null,
      <Text key="err" style={{ flex: 1, fontSize: 12, color: RED }}>{error}</Text>,
      <Button key="c" label={t('calendar.cancel')} onClick={onClose} />,
      <Button key="s" primary disabled={busy} label={isNew ? t('calendar.add') : t('calendar.save')} onClick={save} />,
    ]}>
      <TextInput value={d.title} onChange={(v) => set({ title: v })} placeholder={t('calendar.titlePlaceholder')} size={15} autoFocus={isNew} onSubmit={save} />
      <TextInput value={d.loc} onChange={(v) => set({ loc: v })} placeholder={t('calendar.locationPlaceholder')} onSubmit={save} />
      <View style={{ gap: 8, marginTop: 4 }}>
        <Field label={t('calendar.allDay')}><Toggle on={d.allDay} onChange={(v) => set({ allDay: v })} /></Field>
        <Field label={t('calendar.starts')}>
          <DateField value={d.sDay} onChange={setStartDay} />
          {!d.allDay ? <TimeField value={d.sMin} onChange={setStartMin} /> : null}
        </Field>
        <Field label={t('calendar.ends')}>
          <DateField value={d.eDay} onChange={(v) => set({ eDay: sod(v) < d.sDay ? d.sDay : sod(v) })} />
          {!d.allDay ? <TimeField value={d.eMin} onChange={(v) => set({ eMin: v })} /> : null}
        </Field>
        <Field label={t('calendar.repeat')}><Choice width={200} value={custom ? d.rrule : freq ? `FREQ=${freq}` : ''} options={repeatOpts} onChange={(v) => set({ rrule: v })} /></Field>
        <Field label={t('calendar.alert')}><Choice width={200} value={d.alarm} options={alertOpts} onChange={(v) => set({ alarm: v })} /></Field>
        <Field label={t('calendar.calendar')}>
          {calOpts.length ? <Choice width={200} value={d.cal} options={calOpts} onChange={(v) => set({ cal: v })} />
            : <Text style={{ fontSize: 12, color: th.textDim }}>{t('calendar.defaultName')}</Text>}
        </Field>
        {recurring ? <Field label=""><Text style={{ fontSize: 11, color: th.textDim }}>{t('calendar.seriesNote')}</Text></Field> : null}
      </View>
      {ev.org || att.length ? (
        <View style={{ gap: 4 }}>
          <Text style={{ fontSize: 11, fontWeight: 600, color: th.textDim }}>{t('calendar.invitees', { count: att.length })}</Text>
          <View style={{ maxHeight: 110 }}>
            <ScrollView style={{ flex: 1 }}>
              {ev.org ? <Person name={ev.org.name || ev.org.email} detail={t('calendar.organizer')} glyph="★" color="#8e8e93" /> : null}
              {att.map((a) => { const [g, c, label] = psInfo(a.ps); return <Person key={a.email} name={a.name || a.email} detail={label} glyph={g} color={c} />; })}
            </ScrollView>
          </View>
        </View>
      ) : null}
      <TextArea value={d.notes} onChange={(v) => set({ notes: v })} placeholder={t('calendar.notesPlaceholder')} minHeight={60} />
    </Sheet>
  );
}

function Person({ name, detail, glyph, color }) {
  const th = useTheme();
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8, height: 22 }}>
      <View style={{ width: 16, height: 16, borderRadius: 8, alignItems: 'center', justifyContent: 'center', backgroundColor: color }}>
        <Text style={{ fontSize: 9, fontWeight: 700, color: '#fff' }}>{glyph}</Text>
      </View>
      <Text style={{ flex: 1, fontSize: 12, color: th.text, whiteSpace: 'nowrap', overflow: 'hidden' }}>{name}</Text>
      <Text style={{ fontSize: 11, color: th.textDim }}>{detail}</Text>
    </View>
  );
}

// ---- new calendar / edit calendar ----

export function CalendarSheet({ cal, onClose }) {
  const [name, setName] = useState(cal ? cal.name : '');
  const [color, setColor] = useState(cal ? cal.color : COLORS[5]);
  const [error, setError] = useState('');
  const th = useTheme();
  const save = async () => {
    if (!name.trim()) return;
    try {
      if (cal) await calendars.update(cal.id, { name: name.trim(), color });
      else await calendars.create({ name: name.trim(), color, events: true, todos: false });
      onClose();
    } catch (e) { setError(t('calendar.saveFailed')); }
  };
  return (
    <Sheet width={380} title={cal ? t('calendar.editCalendar') : t('calendar.newCalendar')} footer={[
      <Text key="e" style={{ flex: 1, fontSize: 12, color: RED }}>{error}</Text>,
      <Button key="c" label={t('calendar.cancel')} onClick={onClose} />,
      <Button key="s" primary disabled={!name.trim()} label={cal ? t('calendar.save') : t('calendar.create')} onClick={save} />,
    ]}>
      <TextInput value={name} onChange={setName} placeholder={t('calendar.calendarName')} autoFocus onSubmit={save} onCancel={onClose} />
      <Text style={{ fontSize: 11, fontWeight: 600, color: th.textDim }}>{t('calendar.color')}</Text>
      <ColorSwatches value={color} onChange={setColor} />
    </Sheet>
  );
}

