// store.js — minimal global state with subscription, to use from React with useStore().
import { useSyncExternalStore } from 'react';

export function createStore(initial) {
  let state = initial;
  const subs = new Set();
  return {
    get: () => state,
    set(patch) {
      // function: new state; object over object: merged; null or another value: replaces
      const next = typeof patch === 'function' ? patch(state)
        : patch !== null && typeof patch === 'object' && state !== null && typeof state === 'object' && !Array.isArray(patch) ? { ...state, ...patch }
          : patch;
      if (next === state) return;
      state = next;
      for (const fn of subs) fn();
    },
    subscribe(fn) { subs.add(fn); return () => subs.delete(fn); },
  };
}

export function useStore(store, select = (s) => s) {
  return useSyncExternalStore(store.subscribe, () => select(store.get()));
}
