// Engine I/O for native text editors. Rust draws composition and candidates.
import { imeState } from '../sdk/ime.js';
import { textFieldActive } from '../sdk/textfocus.js';
export function inputServices({ engine = lord.ime, state = imeState, fieldActive = textFieldActive } = {}) {
  let active = 0;
  const tracked = new Set();
  function release(id) {
    if (active !== id) return;
    active = 0;
    // A SDK field can have acquired the engine before the blur job arrives.
    if (!fieldActive()) engine?.setContext(false);
  }
  return {
    'inputMethod.context': { run([on], owner) {
      if (!tracked.has(owner.id)) {
        tracked.add(owner.id);
        owner.cleanup(() => { tracked.delete(owner.id); release(owner.id); });
      }
      if (on && !fieldActive()) {
        if (active && active !== owner.id) engine?.reset?.();
        active = owner.id; engine?.setContext(true);
      }
      else release(owner.id);
      return null;
    } },
    'inputMethod.state': { run: (_, owner) => active === owner.id && !fieldActive() ? state.get() : null },
  };
}
