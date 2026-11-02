// card.jsx — a contact's card (read-only, with group membership and mail links) and its editor
// (name, photo, phones, emails, addresses, web pages, birthday and notes).
import {
  View, Text, useTheme, Button, TextInput, ScrollView, Hairline, ContactPhoto, contacts, contactName, wm, getApp, formatDate, FilePicker, t,
  isolatedPim, pickFileFd,
} from '@lordos/sdk';
import { useEffect, useRef, useState } from 'react';
import { RED, Popover, MenuList, Choice, TextArea, DateField, ymd, parseYmd, monthName } from './ui.jsx';

const TYPES = {
  phones: ['cell', 'home', 'work', 'main', 'other'],
  emails: ['home', 'work', 'other'],
  urls: ['home', 'work', 'other'],
  addrs: ['home', 'work', 'other'],
};
const KINDS = ['phones', 'emails', 'urls', 'addrs'];

export const blankContact = () => ({ n: { given: '', family: '' }, org: '', title: '', phones: [], emails: [], urls: [], addrs: [], notes: '', kind: 'individual' });

// vCard types come as "CELL", "voice,cell", "pref,work"…: the first one we know, or the raw text
function typeOf(kind, raw) {
  const parts = String(raw || '').toLowerCase().split(/[,;\s]+/).filter(Boolean);
  const known = parts.find((p) => TYPES[kind].includes(p) || (kind === 'phones' && p === 'mobile'));
  if (known) return known === 'mobile' ? 'cell' : known;
  return parts.find((p) => p !== 'pref' && p !== 'voice' && p !== 'internet') || 'other';
}
const typeLabel = (kind, raw) => {
  const ty = typeOf(kind, raw);
  return TYPES[kind].includes(ty) ? t(`contacts.type.${ty}`) : ty;
};

function bdayText(s) {
  const full = parseYmd(s);
  if (full) return formatDate(full, 'long');
  const m = /^--(\d{2})-?(\d{2})$/.exec(s || '');
  return m ? t('contacts.dayMonth', { day: +m[2], month: monthName(+m[1] - 1) }) : String(s || '');
}

const addrLines = (a) => [a.street, [a.code, a.city].filter(Boolean).join(' '), a.region, a.country].filter(Boolean);

function openMail(name, email) {
  if (!getApp('mail') && !isolatedPim()) return;    // in its own process, the session opens Mail
  wm.open('mail', { new: true, args: { compose: { to: [{ name: name || '', email }] } } });
}

// ---- card ----

export function Card({ contact: c, groups, onEdit }) {
  const th = useTheme();
  const groupRef = useRef(null);
  const [groupMenu, setGroupMenu] = useState(false);
  const name = contactName(c) || t('contacts.noName');
  const sub = [c.title, c.dept, c.org].filter(Boolean).join(' · ');
  const n = c.n || {};
  const showOrg = c.org && (n.given || n.family) && name !== c.org;
  const toggleGroup = (g) => {
    const m = g.members || [];
    const members = m.includes(c.uid) ? m.filter((x) => x !== c.uid) : [...m, c.uid];
    contacts.save(g.book, { ...g, members }, {}).catch(() => {});
  };
  const mine = groups.filter((g) => (g.members || []).includes(c.uid));
  const box = useRef(null);
  const w = useWidth(box);
  const narrow = w < 400;                 // a narrow card stacks the photo above the name, centered
  const lw = w < 300 ? 0 : narrow ? 70 : 96;    // 0: the label goes above its value
  const head = [
    <ContactPhoto key="p" contact={c} size={narrow ? 72 : 88} />,
    <View key="n" style={narrow ? { alignSelf: 'stretch', alignItems: 'center', gap: 3 } : { flex: 1, gap: 3 }}>
      <Text style={{ fontSize: narrow ? 20 : 24, fontWeight: 600, color: th.text, textAlign: narrow ? 'center' : 'start' }}>{name}</Text>
      {sub && (showOrg || c.title) ? <Text style={{ fontSize: 13, color: th.textDim, textAlign: narrow ? 'center' : 'start' }}>{sub}</Text> : null}
      {c.nick ? <Text style={{ fontSize: 12, color: th.textDim }}>{`“${c.nick}”`}</Text> : null}
      {mine.length ? <Text style={{ fontSize: 11, color: th.textDim }}>{mine.map((g) => contactName(g)).join(', ')}</Text> : null}
    </View>,
  ];
  return (
    <View ref={box} style={{ flex: 1 }}>
      <ScrollView style={{ flex: 1 }}>
        <View style={{ paddingHorizontal: narrow ? 16 : 28, paddingTop: 24, paddingBottom: 24, gap: 20 }}>
          <View style={narrow ? { alignItems: 'center', gap: 12 } : { flexDirection: 'row', alignItems: 'center', gap: 20 }}>{head}</View>
          <View style={{ gap: 0 }}>
            {(c.phones || []).map((p, i) => <Row key={`p${i}`} lw={lw} label={typeLabel('phones', p.type)} value={p.value} />)}
            {(c.emails || []).map((p, i) => <Row key={`e${i}`} lw={lw} label={typeLabel('emails', p.type)} value={p.value} link onClick={() => openMail(contactName(c), p.value)} />)}
            {(c.urls || []).map((p, i) => <Row key={`u${i}`} lw={lw} label={typeLabel('urls', p.type)} value={p.value} />)}
            {(c.addrs || []).map((a, i) => <Row key={`a${i}`} lw={lw} label={typeLabel('addrs', a.type)} value={addrLines(a).join('\n')} />)}
            {c.bday ? <Row lw={lw} label={t('contacts.birthday')} value={bdayText(c.bday)} /> : null}
            {c.notes ? <Row lw={lw} label={t('contacts.notes')} value={c.notes} /> : null}
          </View>
        </View>
      </ScrollView>
      <View style={{ height: 40, flexDirection: 'row', alignItems: 'center', gap: 8, paddingHorizontal: 12 }}>
        <Hairline edge="top" />
        <View ref={groupRef}>
          <Button small label={t('contacts.groups')} disabled={!groups.length} onClick={() => setGroupMenu(true)} />
          {groupMenu ? (
            <Popover anchor={groupRef} width={220} onClose={() => setGroupMenu(false)}>
              <MenuList onDone={() => setGroupMenu(false)} items={groups.map((g) => ({ label: contactName(g) || t('contacts.untitledGroup'),
                checked: (g.members || []).includes(c.uid), onClick: () => toggleGroup(g) }))} />
            </Popover>
          ) : null}
        </View>
        <View style={{ flex: 1 }} />
        <Button small label={t('contacts.edit')} onClick={onEdit} />
      </View>
    </View>
  );
}

// the width of a box after layout (re-measured on every render)
function useWidth(ref, initial = 600) {
  const [w, setW] = useState(initial);
  useEffect(() => {
    const id = setTimeout(() => { if (ref.current) { const nw = lord.getLayout(ref.current.id)[2]; if (nw && Math.abs(nw - w) > 0.5) setW(nw); } }, 20);
    return () => clearTimeout(id);
  });
  return w;
}

function Row({ label, value, link, onClick, lw = 96 }) {
  const th = useTheme();
  if (!lw) {
    return (
      <View style={{ paddingVertical: 5, gap: 1 }}>
        <Text style={{ fontSize: 11, color: th.textDim }}>{label}</Text>
        <Text onClick={onClick} style={{ fontSize: 13, color: link ? th.accent : th.text, lineHeight: 18 }}>{value}</Text>
      </View>
    );
  }
  return (
    <View style={{ flexDirection: 'row', gap: 12, paddingVertical: 5 }}>
      <Text style={{ width: lw, fontSize: 12, color: th.textDim, textAlign: 'end', paddingTop: 1, whiteSpace: 'nowrap', overflow: 'hidden' }}>{label}</Text>
      <Text onClick={onClick} style={{ flex: 1, fontSize: 13, color: link ? th.accent : th.text, lineHeight: 18 }}>{value}</Text>
    </View>
  );
}

// ---- editor ----

const clean = (s) => String(s || '').trim();

function buildContact(d) {
  const c = { ...d, n: { ...(d.n || {}) } };
  for (const k of ['given', 'family', 'middle', 'prefix', 'suffix']) if (c.n[k] != null) c.n[k] = clean(c.n[k]);
  c.org = clean(c.org);
  c.title = clean(c.title);
  for (const k of ['phones', 'emails', 'urls']) c[k] = (c[k] || []).map((x) => ({ type: x.type || 'other', value: clean(x.value) })).filter((x) => x.value);
  c.addrs = (c.addrs || []).map((a) => ({ type: a.type || 'home', street: clean(a.street), city: clean(a.city), region: clean(a.region), code: clean(a.code), country: clean(a.country) }))
    .filter((a) => a.street || a.city || a.region || a.code || a.country);
  const full = [c.n.prefix, c.n.given, c.n.middle, c.n.family, c.n.suffix].filter(Boolean).join(' ');
  c.fn = full || c.org || (c.emails[0] && c.emails[0].value) || (c.phones[0] && c.phones[0].value) || '';
  return c;
}

export function Editor({ contact, groups, onCancel, onSave, onDelete }) {
  const th = useTheme();
  const [d, setD] = useState(() => ({ ...blankContact(), ...contact, n: { ...(contact.n || {}) } }));
  const [photo, setPhoto] = useState('keep');           // 'keep' | 'remove' | a path
  const [photoMenu, setPhotoMenu] = useState(false);
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const photoRef = useRef(null);
  const set = (p) => setD((x) => ({ ...x, ...p }));
  const setN = (p) => setD((x) => ({ ...x, n: { ...x.n, ...p } }));
  const save = async () => {
    if (busy) return;
    const c = buildContact(d);
    if (!c.fn) { setError(t('contacts.needName')); return; }
    setBusy(true);
    setError('');
    try { await onSave(c, photo); } catch (e) { setBusy(false); setError(t('contacts.saveFailed')); }
  };
  const shownPhoto = photo === 'remove' ? { ...d, photo: false } : d;
  return (
    <View style={{ flex: 1 }}>
      <ScrollView style={{ flex: 1 }}>
        <View style={{ padding: 24, gap: 16 }}>
          <View style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 18 }}>
            <View ref={photoRef} onClick={() => setPhotoMenu(true)} style={{ alignItems: 'center', gap: 4 }}>
              <ContactPhoto contact={shownPhoto} size={96} />
              <Text style={{ fontSize: 11, color: th.accent }}>{photo !== 'keep' && photo !== 'remove' ? t('contacts.photoChosen') : t('contacts.editPhoto')}</Text>
              {photoMenu ? (
                <Popover anchor={photoRef} width={200} onClose={() => setPhotoMenu(false)}>
                  <MenuList onDone={() => setPhotoMenu(false)} items={[
                    { label: t('contacts.choosePhoto'), onClick: () => {
                      if (!isolatedPim()) { setPicking(true); return; }
                      let f = null;           // the system's file dialog (only the chosen image reaches this process)
                      try { f = pickFileFd(); } catch (e) { f = null; }
                      if (f) setPhoto(f);
                    } },
                    { label: t('contacts.removePhoto'), destructive: true, disabled: !d.photo && photo === 'keep', onClick: () => setPhoto('remove') },
                  ]} />
                </Popover>
              ) : null}
            </View>
            <View style={{ flex: 1, gap: 6 }}>
              <TextInput value={d.n.given || ''} onChange={(v) => setN({ given: v })} placeholder={t('contacts.given')} autoFocus={!!contact._new} />
              <TextInput value={d.n.family || ''} onChange={(v) => setN({ family: v })} placeholder={t('contacts.family')} />
              <TextInput value={d.title || ''} onChange={(v) => set({ title: v })} placeholder={t('contacts.jobTitle')} />
              <TextInput value={d.org || ''} onChange={(v) => set({ org: v })} placeholder={t('contacts.company')} />
            </View>
          </View>
          {KINDS.map((k) => <MultiField key={k} kind={k} rows={d[k] || []} onChange={(rows) => set({ [k]: rows })} />)}
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
            {d.bday ? (
              <>
                <RoundButton glyph="−" color={RED} onClick={() => set({ bday: '' })} />
                <Text style={{ width: 90, fontSize: 12, color: th.textDim, textAlign: 'end' }}>{t('contacts.birthday')}</Text>
                <DateField value={parseYmd(d.bday) || (/^--(\d{2})-?(\d{2})$/.test(d.bday) ? new Date(2000, +d.bday.slice(2, 4) - 1, +d.bday.slice(-2)) : new Date())}
                  onChange={(v) => set({ bday: ymd(v) })} width={130} />
              </>
            ) : (
              <AddRow label={t('contacts.addBirthday')} onClick={() => set({ bday: ymd(new Date(1990, 0, 1)) })} />
            )}
          </View>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 11, fontWeight: 600, color: th.textDim }}>{t('contacts.notes')}</Text>
            <TextArea value={d.notes || ''} onChange={(v) => set({ notes: v })} minHeight={70} />
          </View>
        </View>
      </ScrollView>
      <View style={{ height: 40, flexDirection: 'row', alignItems: 'center', gap: 8, paddingHorizontal: 12 }}>
        <Hairline edge="top" />
        {onDelete ? <Button small destructive label={t('contacts.deleteContact')} onClick={onDelete} /> : null}
        <Text style={{ flex: 1, fontSize: 12, color: RED, textAlign: 'center' }}>{error}</Text>
        <Button small label={t('contacts.cancel')} onClick={onCancel} />
        <Button small primary disabled={busy} label={t('contacts.done')} onClick={save} />
      </View>
      {picking ? (
        <FilePicker title={t('contacts.choosePhoto')} filter={/\.(png|jpe?g|gif|bmp|webp)$/i} onClose={() => setPicking(false)}
          onPick={(p) => { setPicking(false); if (typeof p === 'string') setPhoto(p); }} />
      ) : null}
    </View>
  );
}

function RoundButton({ glyph, color, onClick }) {
  return (
    <View onClick={onClick} style={{ width: 18, height: 18, borderRadius: 9, alignItems: 'center', justifyContent: 'center', backgroundColor: color }}>
      <Text style={{ fontSize: 13, fontWeight: 700, color: '#fff' }}>{glyph}</Text>
    </View>
  );
}

function AddRow({ label, onClick }) {
  const th = useTheme();
  return (
    <View onClick={onClick} style={{ flexDirection: 'row', alignItems: 'center', gap: 8, height: 26 }}>
      <RoundButton glyph="+" color="#34c759" onClick={onClick} />
      <Text style={{ fontSize: 12, color: th.textDim }}>{label}</Text>
    </View>
  );
}

function MultiField({ kind, rows, onChange }) {
  const th = useTheme();
  const types = TYPES[kind];
  const update = (i, p) => onChange(rows.map((r, j) => (j === i ? { ...r, ...p } : r)));
  const opts = (raw) => {
    const cur = typeOf(kind, raw);
    const o = types.map((ty) => [ty, t(`contacts.type.${ty}`)]);
    if (!types.includes(cur)) o.push([cur, cur]);
    return o;
  };
  return (
    <View style={{ gap: 6 }}>
      {rows.map((r, i) => (
        <View key={i} style={{ flexDirection: 'row', alignItems: kind === 'addrs' ? 'flex-start' : 'center', gap: 8 }}>
          <View style={{ paddingTop: kind === 'addrs' ? 4 : 0 }}><RoundButton glyph="−" color={RED} onClick={() => onChange(rows.filter((_, j) => j !== i))} /></View>
          <Choice width={90} value={typeOf(kind, r.type)} options={opts(r.type)} onChange={(v) => update(i, { type: v })} />
          {kind === 'addrs' ? (
            <View style={{ flex: 1, gap: 4 }}>
              <TextInput value={r.street || ''} onChange={(v) => update(i, { street: v })} placeholder={t('contacts.street')} />
              <View style={{ flexDirection: 'row', gap: 4 }}>
                <View style={{ flex: 2 }}><TextInput value={r.city || ''} onChange={(v) => update(i, { city: v })} placeholder={t('contacts.city')} /></View>
                <View style={{ flex: 1 }}><TextInput value={r.code || ''} onChange={(v) => update(i, { code: v })} placeholder={t('contacts.postalCode')} /></View>
              </View>
              <View style={{ flexDirection: 'row', gap: 4 }}>
                <View style={{ flex: 1 }}><TextInput value={r.region || ''} onChange={(v) => update(i, { region: v })} placeholder={t('contacts.region')} /></View>
                <View style={{ flex: 1 }}><TextInput value={r.country || ''} onChange={(v) => update(i, { country: v })} placeholder={t('contacts.country')} /></View>
              </View>
            </View>
          ) : (
            <View style={{ flex: 1 }}>
              <TextInput value={r.value || ''} onChange={(v) => update(i, { value: v })} placeholder={t(`contacts.placeholder.${kind}`)} />
            </View>
          )}
        </View>
      ))}
      <View style={{ flexDirection: 'row' }}>
        <AddRow label={t(`contacts.add.${kind}`)} onClick={() => onChange([...rows, kind === 'addrs' ? { type: types[0], street: '', city: '', region: '', code: '', country: '' } : { type: types[0], value: '' }])} />
      </View>
    </View>
  );
}
