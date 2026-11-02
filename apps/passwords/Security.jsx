// Security.jsx — the security report (weak, reused and old passwords; breached ones from the
// list of common passwords and, if the user picks one, a local file of known-breached hashes:
// never over the network) and the view with every 2FA code and its countdown.
import { View, Text, useTheme, useStore, Button, ScrollView, EmptyState, t } from '@lordos/sdk';
import { useEffect, useRef, useState } from 'react';
import { app, req, errorText, toast, copySecret } from './api.js';
import { allCategories, builtinById } from './templates.js';
import { EntryIcon, Hint, FilePicker, strengthColor, strengthLabel, RED, ORANGE, YELLOW, GREEN, IconButton, fileReq } from './ui.jsx';
import { Glyph } from './glyphs.jsx';
import { Center } from './Detail.jsx';

// One finding: a colored dot, the title and the count, a hint line and the items, in an inset box.
function Card({ title, count, color, children, hint }) {
  const th = useTheme();
  const kids = (Array.isArray(children) ? children.flat() : [children]).filter(Boolean);
  return (
    <View style={{ borderRadius: 10, border: `1px solid ${th.separator}`, backgroundColor: th.mode === 'dark' ? 'rgba(255,255,255,0.04)' : 'rgba(0,0,0,0.025)',
      paddingHorizontal: 14, paddingTop: 12, paddingBottom: kids.length ? 6 : 12, gap: 4 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <View style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: color }} />
        <Text style={{ flex: 1, minWidth: 0, fontSize: 13, fontWeight: 600, color: th.text }}>{title}</Text>
        <Text style={{ fontSize: 17, fontWeight: 600, color: count ? color : th.textDim }}>{String(count)}</Text>
      </View>
      {hint && <Hint>{hint}</Hint>}
      {kids.length ? <View style={{ marginTop: 4 }}>{kids}</View> : null}
    </View>
  );
}

function Item({ e, cats, sub, onOpen }) {
  const th = useTheme();
  const [hover, setHover] = useState(false);
  if (!e) return null;
  const cat = cats.find((c) => c.id === e.cat) || builtinById(e.cat);
  return (
    <View onClick={() => onOpen(e.id)} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{ flexDirection: 'row', alignItems: 'center', gap: 10, minHeight: 40, paddingHorizontal: 6, marginHorizontal: -6, borderRadius: 6,
        borderTopWidth: 1, borderTopColor: th.separator, backgroundColor: hover ? th.hover : 'transparent' }}>
      <EntryIcon entry={e} cat={cat} size={24} />
      <View style={{ flex: 1, minWidth: 0 }}>
        <Text style={{ fontSize: 13, color: th.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{e.title || t('passwords.untitled')}</Text>
        {sub && <Text style={{ fontSize: 11, color: th.textDim, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{sub}</Text>}
      </View>
      <Text style={{ fontSize: 14, color: th.textDim }}>›</Text>
    </View>
  );
}

export function SecurityView({ onOpen }) {
  const th = useTheme();
  const st = useStore(app);
  const cats = allCategories(st.cats);
  const [rep, setRep] = useState(null);
  const [hibp, setHibp] = useState(null);
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);
  const run = async (file) => {
    setBusy(true);
    try { setRep(await req('report', {}, file ? fileReq([file, 'r']) : undefined)); } catch (e) { toast(errorText(e.code)); }
    setBusy(false);
  };
  useEffect(() => { run(hibp); }, [st.entries.length]);
  const byId = new Map(st.entries.map((e) => [e.id, e]));
  if (!rep) return <Center text={t('passwords.report.loading')} />;
  const issues = rep.weak.length + rep.reused.reduce((a, g) => a + g.length, 0) + rep.old.length + (rep.breached?.length || 0);
  const total = st.entries.filter((e) => !e.trash).length;
  return (
    <ScrollView style={{ flex: 1 }}>
      <View style={{ paddingHorizontal: 24, paddingTop: 20, paddingBottom: 24, gap: 12, width: '100%', maxWidth: 760, alignSelf: 'center' }}>
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 14, marginBottom: 4 }}>
          <View style={{ width: 48, height: 48, borderRadius: 12, alignItems: 'center', justifyContent: 'center',
            background: issues ? 'linear-gradient(180deg, #ffb340, #ff9500)' : 'linear-gradient(180deg, #4cd964, #28b84a)' }}>
            <Glyph kind="shield" size={28} color="#fff" />
            <View style={{ position: 'absolute', left: 0, right: 0, top: 0, bottom: 0, alignItems: 'center', justifyContent: 'center', paddingBottom: 3 }}>
              <Text style={{ fontSize: 13, fontWeight: 700, color: '#fff' }}>{issues ? '!' : '✓'}</Text>
            </View>
          </View>
          <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
            <Text style={{ fontSize: 22, fontWeight: 600, color: th.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{t('passwords.report.title')}</Text>
            <Text style={{ fontSize: 12, color: th.textDim }}>{issues ? t('passwords.report.issues', { count: issues, total }) : t('passwords.report.allGood', { total })}</Text>
          </View>
          <Button small label={busy ? t('passwords.report.loading') : t('passwords.report.again')} onClick={() => run(hibp)} />
        </View>
        <Card title={t('passwords.report.weak')} count={rep.weak.length} color={RED} hint={t('passwords.report.weakHint')}>
          {rep.weak.map((w) => <Item key={w.id} e={byId.get(w.id)} cats={cats} onOpen={onOpen}
            sub={w.common ? t('passwords.report.common') : `${strengthLabel(w.score)} · ${t('passwords.report.bits', { bits: w.bits })}`} />)}
        </Card>
        <Card title={t('passwords.report.reused')} count={rep.reused.length} color={ORANGE} hint={t('passwords.report.reusedHint')}>
          {rep.reused.map((g, i) => (
            <View key={i}>
              <Text style={{ fontSize: 11, fontWeight: 600, color: th.textDim, paddingTop: 8, paddingBottom: 4 }}>{t('passwords.report.sameIn', { count: g.length })}</Text>
              {g.map((id) => <Item key={id} e={byId.get(id)} cats={cats} onOpen={onOpen} />)}
            </View>
          ))}
        </Card>
        <Card title={t('passwords.report.old')} count={rep.old.length} color={YELLOW} hint={t('passwords.report.oldHint')}>
          {rep.old.map((o) => <Item key={o.id} e={byId.get(o.id)} cats={cats} onOpen={onOpen} sub={t('passwords.report.age', { days: o.days, limit: o.limit })} />)}
        </Card>
        <Card title={t('passwords.report.breached')} count={rep.breached ? rep.breached.length : rep.weak.filter((w) => w.common).length} color={RED}
          hint={hibp ? t('passwords.report.hibpUsing') : t('passwords.report.breachedHint')}>
          {(rep.breached || []).map((b) => <Item key={b.id} e={byId.get(b.id)} cats={cats} onOpen={onOpen} sub={t('passwords.report.seen', { count: b.count })} />)}
          <View style={{ paddingTop: 4, paddingBottom: 6, alignItems: 'flex-start' }}><Button small label={t('passwords.report.hibpChoose')} onClick={() => setPicking(true)} /></View>
        </Card>
        <View style={{ paddingHorizontal: 4 }}><Hint>{t('passwords.report.offline')}</Hint></View>
      </View>
      {picking && <FilePicker title={t('passwords.report.hibpChoose')} onClose={() => setPicking(false)}
        onPick={(p) => { setPicking(false); setHibp(typeof p === 'string' ? p : null); run(p); }} />}
    </ScrollView>
  );
}

// ---- every 2FA code ----

export function CodesView({ onOpen }) {
  const th = useTheme();
  const st = useStore(app);
  const cats = allCategories(st.cats);
  const [codes, setCodes] = useState([]);
  const base = useRef({ at: 0 });
  const [, tick] = useState(0);
  const load = async () => {
    try { const r = await req('totp'); base.current = { at: Date.now() / 1000 }; setCodes(r.codes || []); } catch (e) { setCodes([]); }
  };
  useEffect(() => { load(); }, [st.entries.length]);
  useEffect(() => {
    const iv = setInterval(() => {
      const el = Date.now() / 1000 - base.current.at;
      if (codes.some((c) => c.period && c.remaining - el <= 0)) load(); else tick((x) => x + 1);
    }, 1000);
    return () => clearInterval(iv);
  }, [codes]);
  const byId = new Map(st.entries.map((e) => [e.id, e]));
  if (!codes.length) return <EmptyState icon={<Glyph kind="clock" size={40} color={th.textDim} />} title={t('passwords.side.codes')} hint={t('passwords.codes.empty')} />;
  const el = Date.now() / 1000 - base.current.at;
  return (
    <ScrollView style={{ flex: 1 }}>
      <View style={{ paddingHorizontal: 24, paddingTop: 20, paddingBottom: 24, gap: 8, width: '100%', maxWidth: 760, alignSelf: 'center' }}>
        <Text style={{ fontSize: 22, fontWeight: 600, color: th.text, marginBottom: 8 }}>{t('passwords.side.codes')}</Text>
        {codes.map((c) => {
          const e = byId.get(c.id);
          const left = Math.max(0, Math.round(c.remaining - el));
          const shown = c.code.length === 6 ? `${c.code.slice(0, 3)} ${c.code.slice(3)}` : c.code;
          return (
            <View key={`${c.id}-${c.k}`} onClick={() => onOpen(c.id)} style={{ flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 12, height: 60,
              borderRadius: 10, border: `1px solid ${th.separator}`, backgroundColor: th.mode === 'dark' ? 'rgba(255,255,255,0.04)' : 'rgba(0,0,0,0.025)' }}>
              {e && <EntryIcon entry={e} cat={cats.find((x) => x.id === e.cat)} size={32} />}
              <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                <Text style={{ fontSize: 13, fontWeight: 600, color: th.text, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{e?.title || c.issuer || ''}</Text>
                <Text style={{ fontSize: 11, color: th.textDim, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{c.account || e?.sub || ''}</Text>
              </View>
              <Text style={{ fontSize: 20, fontWeight: 600, fontFamily: 'monospace', color: left <= 5 ? RED : th.text, direction: 'ltr' }}>{shown}</Text>
              <View style={{ width: 34, alignItems: 'center', gap: 4 }}>
                <Text style={{ fontSize: 12, color: left <= 5 ? RED : th.textDim }}>{`${left}s`}</Text>
                <View style={{ width: 30, height: 3, borderRadius: 2, backgroundColor: th.windowBg, direction: 'ltr' }}>
                  <View style={{ width: Math.round(30 * (c.period ? left / c.period : 0)), height: 3, borderRadius: 2, backgroundColor: left <= 5 ? RED : GREEN }} />
                </View>
              </View>
              <IconButton glyph="⧉" tip={t('passwords.copy')} onClick={() => copySecret(c.code, t('passwords.totp.code'))} />
            </View>
          );
        })}
      </View>
    </ScrollView>
  );
}

export { strengthColor };
