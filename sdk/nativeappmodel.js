// Shared transport: Rust selects effects and folds results; existing host calls execute them.
import { wm } from './windows.js';
export const nativeAppRequest = (owner, value) => owner.request(JSON.stringify(value));
// One executor for both native views and callers of the app-control service.
export function executeNativeEffect(effect, manifest = {}) {
  let result = '';
  try {
    switch (effect.kind) {
      case 'settings': settings.set(effect.patch); break;
      case 'openApp': wm.open(effect.app, { args: effect.args || {} }); break;
      case 'nativeCall': {
        if (!manifest.calls?.[effect.service]?.includes(effect.method)) throw new Error('app.invalidRequest');
        result = lord[effect.service][effect.method](...(effect.args || []));
        break;
      }
      case 'nativeRead': {
        if (!manifest.reads?.[effect.service]?.includes(effect.method)) throw new Error('app.invalidRequest');
        result = { method: effect.method, value: lord[effect.service][effect.method](...(effect.args || [])) };
        break;
      }
      case 'kill':
        result = typeof lord.wayland.kill === 'function' ? lord.wayland.kill(effect.surface) : 'app.forceQuitFailed';
        break;
      case 'closeSurface': lord.wayland.close(effect.surface); break;
      case 'closeWindow': wm.close(effect.id, true); break;
      default: result = 'app.forceQuitFailed';
    }
  } catch (error) {
    const code = error?.code || error?.message || 'app.forceQuitFailed';
    result = effect.kind === 'nativeCall' || effect.kind === 'nativeRead' ? { error: code } : code;
  }
  return result;
}
export function nativeAppEffects(owner, reply, manifest = {}) {
  while (reply.effect) {
    const result = executeNativeEffect(reply.effect, manifest);
    reply = nativeAppRequest(owner, { cmd: 'next', result });
  }
  return reply.error || '';
}
