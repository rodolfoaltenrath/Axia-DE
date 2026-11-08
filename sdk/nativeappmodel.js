// Shared transport: Rust selects effects and folds results; existing host calls execute them.
import { wm } from './windows.js';
export const nativeAppRequest = (owner, value) => owner.request(JSON.stringify(value));
export function nativeAppEffects(owner, reply) {
  while (reply.effect) {
    const effect = reply.effect;
    let result = '';
    try {
      switch (effect.kind) {
        case 'kill':
          result = typeof lord.wayland.kill === 'function' ? lord.wayland.kill(effect.surface) : 'app.forceQuitFailed';
          break;
        case 'closeSurface': lord.wayland.close(effect.surface); break;
        case 'closeWindow': wm.close(effect.id, true); break;
        default: result = 'app.forceQuitFailed';
      }
    } catch (error) { result = error?.code || 'app.forceQuitFailed'; }
    reply = nativeAppRequest(owner, { cmd: 'next', result });
  }
  return reply.error || '';
}
