// power.jsx — Restart… and Shut Down… from the session (0.8.1; docs/SESSIONS.md, "Restart and Shut
// Down from a session").
//
// session.restart() / session.shutdown() open <PowerDialog>, which a theme renders in the «modal»
// layer inside its ThemeProvider (so the sheet takes the theme's colors; default and classic do):
// what will happen, a 60 s countdown that does it if nobody cancels, the apps still open and the
// other users logged in. Fedora sessions ask logind asynchronously through the
// Rust service and C D-Bus adapter. The host decides authorization, including other
// sessions and apps preventing shutdown. The original LordOS session daemon is
// still supported for sessions managed by it.
import { createElement as h, useEffect, useState } from 'react';
import { View, Text, addListener, mainDisplay } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { t } from './i18n.js';
import { useTheme } from './theme.js';
import { spacing, typeScale, controlSize } from './controls.jsx';
import { windows } from './windows.js';
import { windowAppName } from './apps.js';
import { remote } from './remote.js';
import { flushSession, resumeSession } from './sessionrestore.jsx';
import { onSessionPowerResult } from './hardware.jsx';

export const POWER_SECONDS = 60;
// null, or { action, until (ms), host, state: ask/sent/denied/inhibited/unavailable }
export const powerAsk = createStore(null);

// A local Fedora seat, or the monitor connection of the original session daemon.
const hostPower = () => typeof lord.session?.hostPower === 'function' && lord.session.hostPower();
export const canPower = () => typeof lord.session !== 'undefined' && typeof lord.session.power === 'function'
  && (hostPower() || (lord.session.managed()
    && (typeof lord.session.onMonitor !== 'function' || lord.session.onMonitor())));

export function askPower(action) {
  if (action !== 'reboot' && action !== 'poweroff') return;
  if (powerAsk.get()?.state === 'sent') return;
  powerAsk.set({ action, until: Date.now() + POWER_SECONDS * 1000, state: 'ask', host: hostPower() });
}

onSessionPowerResult((data) => {
  if (data?.ok !== false || powerAsk.get()?.state !== 'sent') return;
  resumeSession();
  const state = data.why === 'denied' ? 'denied' : data.why === 'inhibited' ? 'inhibited' : 'unavailable';
  powerAsk.set((p) => p ? { ...p, state } : p);
});

// lord-sessiond's answers reach the session through the login screen's channel (RP_GREETER_IN);
// the login screen theme has its own handler, so this one is only set in a user session.
if (typeof lord.greeter === 'undefined') {
  globalThis.__lordGreeter = (json) => {
    let m = null;
    try { m = JSON.parse(json); } catch (e) { return; }
    if (m && m.t === 'power' && m.ok === false && powerAsk.get()) { resumeSession(); powerAsk.set((p) => (p ? { ...p, state: 'denied' } : p)); }
  };
}

// Who else is logged in, and may this user do it with them there?
function sessionInfo() {
  let me = null;
  try { me = (typeof lord.users !== 'undefined' ? lord.users.list().users : []).find((u) => u.self) || null; } catch (e) { me = null; }
  const st = remote.status();
  const others = [...new Set((st.sessions || []).map((s) => s.user).filter((u) => u && u !== me?.name))];
  return { others, admin: !!me?.admin };
}

function openApps(list) {
  const names = [];
  for (const w of list) {
    const n = windowAppName(w);
    if (n && !names.includes(n)) names.push(n);
  }
  return names;
}

function Button({ th, label, onClick, primary, disabled }) {
  const [hover, setHover] = useState(false);
  const bg = primary ? '#e5483f' : th.control;
  return h(View, {
    onClick: disabled ? undefined : onClick, onMouseEnter: () => setHover(true), onMouseLeave: () => setHover(false),
    style: { height: controlSize.regular, paddingHorizontal: 14, borderRadius: th.controlRadius ?? controlSize.radius, justifyContent: 'center', alignItems: 'center',
      backgroundColor: bg, opacity: disabled ? 0.45 : hover ? 0.85 : 1 },
  }, h(Text, { style: { fontSize: typeScale.body, fontWeight: 500, color: primary ? '#fff' : th.text, whiteSpace: 'nowrap' } }, label));
}

export function PowerDialog() {
  const ask = useStore(powerAsk);
  const list = useStore(windows);
  const th = useTheme();
  const [, tick] = useState(0);
  const [info, setInfo] = useState(null);
  const open = !!ask;
  useEffect(() => { setInfo(open ? sessionInfo() : null); }, [open, ask?.action]);
  const blocked = !!(!ask?.host && info && info.others.length && !info.admin);
  const counting = open && ask.state === 'ask' && info && !blocked;

  const cancel = () => powerAsk.set(null);
  const confirm = () => {
    const p = powerAsk.get();
    if (!p || p.state !== 'ask' || blocked) return;
    flushSession();                                // what is open, before anything starts closing
    const sent = canPower() && lord.session.power(p.action);
    if (!sent) resumeSession();
    powerAsk.set({ ...p, state: sent ? 'sent' : 'unavailable' });
  };

  useEffect(() => {
    if (!counting) return undefined;
    const id = setInterval(() => {
      if (Date.now() >= powerAsk.get()?.until) { clearInterval(id); confirm(); } else tick((n) => n + 1);
    }, 250);
    return () => clearInterval(id);
  }, [counting, ask?.until]);

  useEffect(() => {
    if (!open) return undefined;
    return addListener('keydown', (e) => {
      if (powerAsk.get()?.state === 'sent') return;
      if (e.key === 'Escape') cancel();
      else if (e.key === 'Enter') confirm();
    });
  }, [open, blocked]);

  if (!ask) return null;
  const reboot = ask.action === 'reboot';
  const left = Math.max(0, Math.ceil((ask.until - Date.now()) / 1000));
  const apps = openApps(list);
  const a = mainDisplay();
  const lines = [];
  const say = (text, color) => lines.push(h(Text, { key: lines.length, style: { fontSize: typeScale.body, color: color || th.textDim, lineHeight: 1.4, textAlign: 'start' } }, text));
  if (ask.state === 'sent') say(t(reboot ? 'shell.power.restarting' : 'shell.power.shuttingDown'));
  else if (ask.state === 'unavailable') say(t('shell.power.unavailable'), '#e5483f');
  else if (ask.state === 'inhibited') say(t('shell.power.inhibited'), '#e5483f');
  else if (ask.state === 'denied' && ask.host) say(t('shell.power.denied'), '#e5483f');
  else if (ask.state === 'denied' || blocked) {
    if (info?.others.length) say(t('shell.power.others', { count: info.others.length, list: info.others.join(', ') }));
    say(t(reboot ? 'shell.power.needAdminRestart' : 'shell.power.needAdminShutdown'), '#e5483f');
  } else {
    say(t(reboot ? 'shell.power.restartCountdown' : 'shell.power.shutdownCountdown', { count: left }));
    if (apps.length) say(t('shell.power.apps', { list: apps.join(', ') }));
    if (info?.others.length) say(t('shell.power.others', { count: info.others.length, list: info.others.join(', ') }));
  }
  const asking = ask.state === 'ask' && !blocked;
  return h(View, {
    onClick: ask.state === 'sent' ? undefined : cancel,
    style: { position: 'absolute', left: a.x, top: a.y, width: a.w, height: a.h, backgroundColor: 'rgba(0,0,0,0.3)', alignItems: 'center', justifyContent: 'center' },
  }, h(View, {
    onClick: (e) => e.stopPropagation(),
    style: { width: 400, padding: spacing.xl, gap: spacing.s, borderRadius: (th.groupRadius ?? 10) + 2, backgroundColor: th.windowBg, border: `1px solid ${th.separator}`,
      boxShadow: '0 14px 36px rgba(0,0,0,0.45)' },
  },
  h(Text, { style: { fontSize: typeScale.headline, fontWeight: 600, color: th.text, textAlign: 'start' } },
    t(reboot ? 'shell.power.restartTitle' : 'shell.power.shutdownTitle')),
  ...lines,
  ask.state === 'sent' ? null : h(View, { style: { flexDirection: 'row', justifyContent: 'flex-end', gap: spacing.s, marginTop: spacing.m } },
    h(Button, { th, label: t('shell.power.cancel'), onClick: cancel }),
    asking ? h(Button, { th, primary: true, label: t(reboot ? 'shell.power.restart' : 'shell.power.shutdown'), onClick: confirm }) : null)));
}
