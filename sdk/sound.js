// sound.js — session sound (PipeWire; the client is in C, server/src/sound — see
// docs/SOUND.md). A store with the full state that updates only when something changes,
// also if another program changed it.
//
//   const s = useSound();          // { connected, sinks, sources, inputs, outputs, defaultSink… }
//   const out = defaultSink(s);    // the default output (or null)
//   sound.setVolume('sink', out.index, 0.5); sound.setMute('sink', out.index, true)
import { createStore, useStore } from './store.js';

const has = () => typeof lord.sound !== 'undefined';
const EMPTY = { connected: false, sinks: [], sources: [], inputs: [], outputs: [] };
export const soundState = createStore(has() ? { ...EMPTY, ...lord.sound.state() } : EMPTY);
export const soundPeak = createStore({ source: -1, level: 0 });

globalThis.__lordSound = (type, data) => {
  if (type === 'state') soundState.set({ ...EMPTY, ...data });
  else if (type === 'peak') soundPeak.set(data);
};

const call = (fn, ...a) => (has() ? lord.sound[fn](...a) : false);
export const sound = {
  available: has,
  setVolume: (kind, index, v) => call('setVolume', kind, index, v),
  setMute: (kind, index, on) => call('setMute', kind, index, !!on),
  setDefault: (kind, name) => call('setDefault', kind, name),
  setPort: (kind, index, port) => call('setPort', kind, index, port),
  move: (kind, app, to) => call('move', kind, app, to),
  meter: (source) => call('meter', source),
  play: (name) => call('play', name),
};

export const useSound = () => useStore(soundState);
export const useSoundPeak = () => useStore(soundPeak);
export const defaultSink = (s) => s.sinks.find((x) => x.default) || s.sinks[0] || null;
// Real microphones (not the outputs' monitors).
export const microphones = (s) => s.sources.filter((x) => x.monitorOf === -1);
export const defaultSource = (s) => microphones(s).find((x) => x.default) || microphones(s)[0] || null;
