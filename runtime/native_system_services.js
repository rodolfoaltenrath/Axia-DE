// Shared SDK I/O for native system tools. Screens, drafts, validation, timing and
// administrative action selection belong to Rust. No app nodes are created here.
import * as sdk from '../sdk/index.js';
import { version as reactVersion } from 'react';
import { screen } from '@lordos/runtime';
import { soundState, soundPeak } from '../sdk/sound.js';
import { videoHardware, videoError } from '../sdk/video.js';
import { browserFrameResult } from '../sdk/browserframe.js';
import { indicatorFacts } from '../sdk/privacy.js';

const getters = {
  platform: () => ({ standalone: sdk.standaloneDesktop, screen: { width: screen.width, height: screen.height }, version: lord.version, reactVersion }),
  'system.info': () => lord.system.info(),
  'system.stats': () => lord.stats(),
  'admin.available': () => sdk.admin.available(),
  'admin.busy': () => sdk.admin.busy.get(),
  'themes.list': () => sdk.themes.list(),
  languages: () => sdk.languages(),
  'locale.available': () => lord.locale.available(),
  'locale.get': () => lord.locale.get(),
  'time.zone': () => lord.time.zone(),
  'time.zones': () => lord.time.zones(),
  'time.zoneOffset': (...args) => lord.time.zoneOffset(...args),
  'timedate.state': () => sdk.timedate.get(),
  'keyboard.list': () => sdk.keyboard.list(),
  'keyboard.missingDictionary': () => sdk.keyboard.missingDictionary(),
  'shortcuts.list': () => sdk.shortcutList(),
  'input.devices': () => lord.input.devices(),
  'display.list': () => lord.display.list(),
  'display.brightness': () => lord.display.brightness(JSON.stringify({ op: 'refresh' })),
  'display.sun': (...args) => lord.display.sun(...args),
  'bt.state': () => lord.bt.state(),
  'sound.state': () => soundState.get(),
  'sound.peak': () => soundPeak.get(),
  'power.state': () => lord.power.state(),
  'power.profiles': () => lord.power.profiles(),
  'power.buttons': () => lord.power.buttons(),
  'fan.status': () => lord.fan.status(),
  'fan.case': () => lord.fan.caseConfig(),
  'net.interfaces': () => lord.net.interfaces(),
  'net.routes': () => lord.net.routes(),
  'net.dns': () => lord.net.dns(),
  'net.config': (...args) => lord.net.config(...args),
  'net.vpnList': () => lord.net.vpnList(),
  'net.vpnTypes': () => lord.net.vpnTypes(),
  'print.hasCups': () => sdk.print.hasCups(),
  'users.list': () => lord.users.list(),
  'perms.list': () => sdk.perms.list(),
  'fw.status': () => lord.fw.status(),
  'fw.state': () => lord.fw.state(),
  'fw.poll': () => lord.fw.poll(),
  'fw.lists': () => lord.fw.lists(),
  'privacy.indicators': () => indicatorFacts(),
  'clipboard.history': () => sdk.clipboardHistory.get(),
  'browserFrame.state': () => sdk.browserFrameState(),
  'browserFrame.result': () => browserFrameResult.get(),
  'video.hardware': () => videoHardware.get(),
  'video.error': () => videoError.get(),
  'defaults.info': (...args) => sdk.defaultInfo(...args),
  'defaults.candidates': (...args) => sdk.candidatesFor(...args).map(({ Icon, ...facts }) => facts),
  'defaults.screenshotLauncher': () => sdk.screenshotLauncher(),
  'packages.list': () => sdk.packages.list().map(({ Component, Icon, ...facts }) => facts),
  'pkg.available': () => lord.pkg.available(),
  'pkg.upgrades': () => lord.pkg.upgrades(),
  'pkg.lastUpdate': () => lord.pkg.lastUpdate(),
  'session.managed': () => sdk.session.managed(),
  'session.canPower': () => sdk.session.canPower(),
};
const calls = {
  'admin.run': (action, arg, quiet, log) => sdk.admin.run(action, arg, line => log.push(line), { quiet: !!quiet }),
  'themes.use': (...args) => sdk.themes.use(...args),
  'language.set': (...args) => sdk.setLanguage(...args),
  'locale.set': (...args) => lord.locale.set(...args),
  'locale.installPack': (...args) => lord.locale.installPack(...args),
  'locale.reload': () => sdk.reloadCatalogs(),
  'keyboard.reloadCompose': () => lord.keyboard?.reloadCompose?.(),
  'time.reload': () => lord.time.reload(),
  'timedate.refresh': () => lord.timedate.refresh(),
  'osk.show': () => sdk.osk.show(),
  'session.quit': () => sdk.session.quit(),
  'session.restart': () => sdk.session.restart(),
  'session.restoreMode': (...args) => sdk.setRestoreMode(...args),
  'session.restoreBrowsers': (...args) => sdk.setRestoreBrowsers(...args),
  'dock.reset': () => sdk.dock.reset(),
  'browserFrame.set': (...args) => sdk.setBrowserTabsInTitlebar(...args),
  'video.set': (...args) => sdk.setVideoPrefs(...args),
  'display.apply': (...args) => sdk.applyDisplay(...args),
  'display.arrange': (...args) => sdk.arrangeDisplay(...args),
  'display.mirror': (...args) => sdk.setDisplayMirror(...args),
  'display.main': (...args) => sdk.setMainDisplay(...args),
  'display.capture': (...args) => sdk.setCaptureAllDisplays(...args),
  'display.identify': (...args) => sdk.identifyDisplays(...args),
  'display.brightness': arg => lord.display.brightness(JSON.stringify(arg)),
  'night.manual': (...args) => sdk.setNightManual(...args),
  'night.preview': (...args) => sdk.previewNight(...args),
  'night.tick': () => sdk.nightTick(),
  'power.profile': (...args) => lord.power.setProfile(...args),
  'bt.power': (...args) => lord.bt.power(...args),
  'bt.pair': (...args) => lord.bt.pair(...args),
  'bt.connect': (...args) => lord.bt.connect(...args),
  'bt.disconnect': (...args) => lord.bt.disconnect(...args),
  'bt.forget': (...args) => lord.bt.forget(...args),
  'bt.discover': (...args) => lord.bt.discover(...args),
  'sound.volume': (...args) => sdk.sound.setVolume(...args),
  'sound.mute': (...args) => sdk.sound.setMute(...args),
  'sound.default': (...args) => sdk.sound.setDefault(...args),
  'sound.port': (...args) => sdk.sound.setPort(...args),
  'sound.meter': (...args) => sdk.sound.meter(...args),
  'sound.play': (...args) => sdk.sound.play(...args),
  'fan.profile': (...args) => lord.fan.setProfile(...args),
  'fan.curve': (...args) => lord.fan.setCurve(...args),
  'fan.force': (...args) => lord.fan.force(...args),
  'perms.set': (...args) => sdk.perms.set(...args),
  'notify': (...args) => sdk.notify(...args),
  'clipboard.clear': (...args) => sdk.clipboard.clear(...args),
  'clipboard.open': () => sdk.clipboardPanel.set('full'),
  'defaults.set': (...args) => sdk.setDefaultFor(...args),
  'defaults.reset': (...args) => sdk.resetDefaultFor(...args),
  'print.printers': () => sdk.print.printers(),
  'print.jobs': () => sdk.print.jobs(),
  'print.discover': () => sdk.print.discover(),
  'print.add': (...args) => sdk.print.add(...args),
  'print.remove': (...args) => sdk.print.remove(...args),
  'print.default': (...args) => sdk.print.setDefault(...args),
  'print.cancel': (...args) => sdk.print.cancel(...args),
  'print.open': (...args) => sdk.print.open(...args),
  'wallpaper.import': (...args) => lord.settings.importWallpaper(...args),
  'keyring.set': (...args) => sdk.keyring.set(...args),
  'keyring.search': (...args) => sdk.keyring.search(...args),
  'keyring.get': (...args) => sdk.keyring.get(...args),
  'autofill.list': (...args) => sdk.autofill(...args),
  'autofill.reveal': (...args) => sdk.autofillReveal(...args),
  'net.prepareOvpn': (...args) => lord.net.prepareOvpn(...args),
};
export function systemServices(events) {
  const discovering = new Set(), metering = new Set();
  return {
    'desktop.read': { run([requests]) { const errors = {}; const result = Object.fromEntries(requests.map(({ key, name, args = [] }) => {
      try { if (!Object.hasOwn(getters, name)) throw new Error('app.invalidRequest'); return [key, getters[name](...args) ?? null]; }
      catch (error) { errors[key] = error?.message || 'app.invalidRequest'; return [key, null]; }
    })); if (Object.keys(errors).length) result.__errors = errors; return result; } },
    'desktop.call': { async run([name, args = []], owner) {
      if (!Object.hasOwn(calls, name)) throw new Error('app.invalidRequest');
      if (name === 'bt.discover') { if (args[0]) discovering.add(owner.id); else discovering.delete(owner.id); }
      if (name === 'sound.meter') { if (args[0] >= 0) metering.add(owner.id); else metering.delete(owner.id); }
      const log = [];
      try { const value = await (name === 'admin.run' ? calls[name](...args, log) : calls[name](...args)); return { ok: true, value: value ?? null, log }; }
      catch (error) { return { ok: false, error: error?.message || String(error), code: error?.code || '', detail: error?.detail || '', log }; }
    } },
    'desktop.watch': { run(args, owner) {
      owner.cleanup(() => { if (discovering.delete(owner.id)) lord.bt?.discover?.(false); });
      owner.cleanup(() => { if (metering.delete(owner.id)) sdk.sound.meter(-1); });
      owner.cleanup(sdk.admin.onEvent(text => owner.emit({ kind: 'admin', text })));
      owner.cleanup(sdk.onBluetoothResult(result => owner.emit({ kind: 'bluetooth', result })));
      owner.cleanup(sdk.onPowerProfileResult(result => owner.emit({ kind: 'powerProfile', result })));
      return null;
    } },
    'desktop.events': { run: (args, owner) => events(owner.id) },
  };
}
