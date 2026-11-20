// Session clock bridge. Rust owns recurrence, scheduling, alarms, timer and stopwatch.
// The shell adapter publishes compatibility snapshots and executes settings/sound effects.
import { createElement as h, useEffect, useState } from 'react';
import { View, Text } from '@lordos/runtime';
import { createStore, useStore } from './store.js';
import { settings } from './settings.js';
import { t, formatTime } from './i18n.js';
import { notify, dismissNotification, notifications, appNotifyPrefs, dndActive, ringingNotifications } from './notify.js';
import { sound } from './sound.js';
import { registerStatusItem } from './statusitems.js';
import { wm } from './windows.js';
import { NativeView } from './nativeview.js';
import { clockBridge } from './nativeclock.js';
import { notificationBridge } from './nativenotifications.js';
import { nativeAppRequest } from './nativeappmodel.js';
import { syncNativeCatalog } from './nativecatalog.js';
const appHost = typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__;
const NATIVE_CLOCK = { id: 'shell:clock', shell: true };
const NATIVE_CLOCK_STATUS = { id: 'shell:clock-status', shell: true };
const MIN = 60000, SNOOZE = 9 * MIN, RING_MS = 60000;
const conf = () => settings.get().clock || {};
export const clockTimer = createStore({ state: 'idle', duration: 0, endsAt: 0, left: 0 });
export const stopwatch = createStore({ running: false, start: 0, acc: 0, laps: [] });
export const nextAlarm = createStore(null);
export const clockState = createStore({ timer: clockTimer.get(), stopwatch: stopwatch.get() });
let handle = null, started = false;
function publish(store, value) {
  if (JSON.stringify(store.get()) !== JSON.stringify(value)) store.set(() => value);
}
export function clockRequest(cmd, extra = {}) {
  const input = { now: Date.now(), ...extra };
  if (!appHost) delete input.config;
  const reply = appHost ? lord.clock.request({ cmd, config: conf(), ...input })
    : nativeAppRequest(syncNativeCatalog(), { cmd: 'clockCommand', action: cmd, ...input });
  return applyClockReply(reply);
}
function applyClockReply(reply) {
  if (!reply) return null;
  if ('timer' in reply) {
    publish(clockTimer, reply.timer); publish(stopwatch, reply.stopwatch); publish(nextAlarm, reply.nextAlarm);
    publish(clockState, { timer: reply.timer, stopwatch: reply.stopwatch });
    for (const effect of reply.effects || []) {
      if (effect.kind === 'settings') settings.set(effect.patch);
      else if (appHost && effect.kind === 'alarm') fireAlarm(effect.alarm, effect.due);
      else if (appHost && effect.kind === 'timer') fireTimer();
    }
    for (const effect of reply.notificationEffects || []) notificationBridge.perform(effect);
    if (appHost) {
      clearTimeout(handle);
      if (started && reply.wait != null) handle = setTimeout(arm, reply.wait);
    }
  }
  return reply;
}
clockBridge.perform = effect => { if (effect.kind === 'clockReply') applyClockReply(effect.reply); };
export function ClockSession() {
  if (appHost || typeof lord === 'undefined' || typeof lord.appId === 'string' || typeof lord.greeter !== 'undefined') return null;
  return h(NativeView, { manifest: NATIVE_CLOCK, stateSource: lord.windowList,
    style: { position: 'absolute', left: 0, top: 0, width: 1, height: 1, pointerEvents: 'none' } });
}
export const nextFire = (alarm, now = Date.now()) => clockRequest('nextFire', { alarm, now })?.value ?? null;
export const clockAlarms = {
  list: () => (appHost ? conf() : clockRequest('snapshot')?.config)?.alarms || [],
  add: alarm => clockRequest('alarmAdd', { alarm })?.value,
  update: (id, patch) => clockRequest('alarmUpdate', { id, patch }),
  remove: id => clockRequest('alarmRemove', { id }), nextFire,
};
export const timerStart = seconds => clockRequest('timerStart', { seconds });
export const timerPause = () => clockRequest('timerPause');
export const timerResume = () => clockRequest('timerResume');
export const timerReset = () => clockRequest('timerReset');
export const timerLeft = (state = clockTimer.get()) => clockRequest('timerLeft', { state })?.value || 0;
export const swElapsed = (state = stopwatch.get()) => clockRequest('swElapsed', { state })?.value || 0;
export const swStart = () => clockRequest('swStart');
export const swStop = () => clockRequest('swStop');
export const swLap = () => clockRequest('swLap');
export const swReset = () => clockRequest('swReset');
export const arm = () => clockRequest('arm');

// ---- ringing ----

export const ringing = appHost ? createStore([]) : ringingNotifications;
function ring(notifyOpts) {
  let iv = null, stopped = false, offNotices = () => {}, offPrefs = () => {};
  const stopAt = Date.now() + RING_MS;
  const stop = () => {
    if (stopped) return;
    stopped = true;
    clearInterval(iv);
    offNotices(); offPrefs();
    ringing.set((l) => l.filter((x) => x !== id));
  };
  const id = notify({
    ...notifyOpts, sticky: true, sound: true,
    onClose: stop,
  });
  const audible = () => {
    const prefs = appNotifyPrefs(notifyOpts.app);
    return id > 0 && notifications.get().some((n) => n.id === id)
      && prefs.allow && prefs.sound && !dndActive() && Date.now() < stopAt;
  };
  // History-only, denied and silent notifications must never arm a repeating sound.
  if (!audible()) return id;
  const check = () => { if (!audible()) stop(); };
  offNotices = notifications.subscribe(check);
  offPrefs = settings.subscribe(check);
  iv = setInterval(() => { if (!audible()) { stop(); return; } sound.play('notify'); }, 2500);
  ringing.set((l) => [...l, id]);
  return id;
}

function fireAlarm(a, due) {
  const label = a.label || t('clock.alarm.default');
  ring({
    app: 'clock', title: label, body: t('clock.alarm.ringing', { time: formatTime(new Date(due)) }),
    actions: [
      { id: 'stop', label: t('clock.alarm.stop'), primary: true, run: () => {} },
      { id: 'snooze', label: t('clock.alarm.snooze', { n: SNOOZE / MIN }), run: () => clockRequest('alarmSnooze', { id: a.id }) },
    ],
  });
}

function fireTimer() {
  ring({
    app: 'clock', title: t('clock.timer.done'), body: t('clock.timer.doneBody'),
    actions: [
      { id: 'stop', label: t('clock.alarm.stop'), primary: true, run: () => {} },
      { id: 'again', label: t('clock.timer.again'), run: () => timerStart(clockTimer.get().duration / 1000) },
    ],
  });
}


// The bar: ⏱ with the time left on the timer, or ⏰ if an alarm rings within the next 24 h.
function TimerLeft({ color }) {
  const s = useStore(clockTimer);
  useTick(s.state === 'running');
  const ms = timerLeft(s);
  const sec = Math.ceil(ms / 1000);
  const txt = `${sec >= 3600 ? `${Math.floor(sec / 3600)}:` : ''}${String(Math.floor(sec / 60) % 60).padStart(sec >= 3600 ? 2 : 1, '0')}:${String(sec % 60).padStart(2, '0')}`;
  return h(Text, { style: { fontSize: 12, fontWeight: 600, color, fontFamily: 'monospace' } }, `⏱ ${txt}`);
}
// redraws every second, only while it runs
function useTick(on) {
  const [, set] = useState(0);
  useEffect(() => { if (!on) return undefined; const id = setInterval(() => set((x) => x + 1), 1000); return () => clearInterval(id); }, [on]);
}

function LegacyClockStatus({ color, height }) {
  const tm = useStore(clockTimer);
  const na = useStore(nextAlarm);
  const soon = na && na.at - Date.now() < 24 * 60 * MIN;
  if (tm.state === 'idle' && !soon) return null;
  return h(View, {
    onClick: () => wm.open('clock', { args: { tab: tm.state !== 'idle' ? 'timer' : 'alarms' } }),
    style: { height, paddingHorizontal: 7, flexDirection: 'row', alignItems: 'center', gap: 6 },
  },
  tm.state !== 'idle' ? h(TimerLeft, { color }) : null,
  soon && tm.state === 'idle' ? h(Text, { style: { fontSize: 12, color } }, `⏰ ${formatTime(new Date(na.at))}`) : null);
}
function ClockStatus(props) {
  return appHost ? h(LegacyClockStatus, props)
    : h(NativeView, { manifest: NATIVE_CLOCK_STATUS, stateSource: lord.windowList, args: props });
}


// Called once by the shell; state survives every Clock window.
export function startAlarms() {
  if (started) return;
  started = true;
  if (appHost) {
    let last = JSON.stringify(conf());
    settings.subscribe(() => {
      const current = JSON.stringify(conf());
      if (current !== last) { last = current; clockRequest('sync'); }
    });
  }
  registerStatusItem('clock', ClockStatus, 10);
  clockRequest('sync');
}
export { dismissNotification };
