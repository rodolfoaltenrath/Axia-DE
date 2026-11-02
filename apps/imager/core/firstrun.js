// firstrun.js — writes lordos-firstrun.txt, the file the Raspberry Pi applies on its first boot
// (lordos-firstrun, server/src/firstrun; format in docs/IMAGER.md, "The first-run file"). Shared
// by the native and desktop apps. Secrets never go in: the password arrives here already hashed
// (yescrypt or SHA-512 crypt) and the Wi-Fi password as its derived WPA2 key.
import * as v from './validate.js';
import { coolerHasFan } from './catalog.js';

export const FIRSTRUN_NAME = 'lordos-firstrun.txt';

// Checks a whole customisation; returns [] or a list of { field, key } problems (key = i18n key).
// secrets: { passwordHash, psk } (already derived) or { password, confirm, wifiPassword } (raw, in
// the form, to check before deriving).
export function check(cfg, secrets = {}) {
  const out = [];
  const add = (field, key) => key && out.push({ field, key });
  if (cfg.hostname) add('hostname', v.hostname(cfg.hostname));
  add('timezone', v.timezone(cfg.timezone));
  if (cfg.user?.name || cfg.user?.fullname) {
    add('user', v.username(cfg.user.name));
    add('fullname', v.fullname(cfg.user.fullname));
    if (!secrets.passwordHash) add('password', v.password(secrets.password, secrets.confirm));
  }
  if (cfg.wifi?.enabled) {
    add('ssid', v.ssid(cfg.wifi.ssid));
    if (!cfg.wifi.open && !secrets.psk) add('wifiPassword', secrets.wifiPassword ? v.wifiPassword(secrets.wifiPassword) : 'imager.err.wifiPassLength');
  }
  if (cfg.ssh !== 'off' && !cfg.user?.name) add('ssh', 'imager.err.sshNeedsUser');
  if (cfg.ssh === 'keys' && !(cfg.sshKeys || []).length) add('ssh', 'imager.err.sshNeedsKey');
  if (cfg.remote && !cfg.user?.name) add('remote', 'imager.err.remoteNeedsUser');
  if (cfg.overclock && !coolerHasFan(cfg.cooler)) add('overclock', 'imager.err.overclockNeedsFan');
  if ((cfg.performance === 'performance' || cfg.performance === 'quiet') && !coolerHasFan(cfg.cooler)) add('performance', 'imager.err.profileNeedsFan');
  return out;
}

// The file. Only what was chosen is written; everything else keeps the image's defaults.
export function build(cfg, { passwordHash, psk } = {}) {
  const L = ['# LordOS first-run settings, written by the LordOS Imager.',
    '# Read once on the first boot by lordos-firstrun and then deleted. docs/IMAGER.md', 'version=1'];
  const put = (k, val) => { if (val !== undefined && val !== null && val !== '') L.push(`${k}=${val}`); };
  const bool = (k, b) => L.push(`${k}=${b ? 1 : 0}`);
  put('hostname', cfg.hostname);
  put('lang', cfg.lang);
  put('timezone', cfg.timezone);
  if (cfg.keyboard) { put('keyboard', cfg.keyboard); L.push(`keyboard_variant=${cfg.variant || ''}`); }
  put('wifi_country', cfg.wifiCountry);
  const hasUser = !!cfg.user?.name;
  if (hasUser) {
    if (!passwordHash || !/^\$(6|y)\$/.test(passwordHash)) throw new Error('imager.err.passEmpty');
    put('user', cfg.user.name);
    put('fullname', cfg.user.fullname && cfg.user.fullname.trim());
    put('password_hash', passwordHash);
    bool('autologin', cfg.user.autologin);
  }
  if (cfg.wifi?.enabled && cfg.wifi.ssid) {
    put('wifi_ssid_hex', v.hex(v.utf8(cfg.wifi.ssid)));
    if (!cfg.wifi.open) {
      if (!/^[0-9a-f]{64}$/.test(psk || '')) throw new Error('imager.err.wifiPassLength');
      put('wifi_psk', psk);
    }
    bool('wifi_hidden', cfg.wifi.hidden);
  }
  if (hasUser) {
    put('ssh', cfg.ssh || 'off');
    if (cfg.ssh && cfg.ssh !== 'off') for (const k of cfg.sshKeys || []) put('ssh_key', k.line || k);
    bool('remote_desktop', cfg.remote);
  }
  bool('bluetooth', cfg.bluetooth !== false);
  put('theme', cfg.theme);
  put('mode', cfg.mode);
  put('accent', cfg.accent);
  put('wallpaper', cfg.wallpaper);
  if (cfg.cooler) {
    put('cooler', cfg.cooler);
    put('performance', cfg.performance);
    bool('overclock', cfg.overclock && coolerHasFan(cfg.cooler));
  }
  bool('auto_updates', cfg.autoUpdates !== false);
  return L.join('\n') + '\n';
}

// A saved profile: everything but secrets (no password, no hash, no Wi-Fi password or key).
const PROFILE_KEYS = ['hostname', 'lang', 'timezone', 'keyboard', 'variant', 'wifiCountry', 'ssh', 'remote', 'bluetooth', 'theme',
  'mode', 'accent', 'wallpaper', 'cooler', 'performance', 'overclock', 'autoUpdates'];
export function toProfile(name, cfg) {
  const { user, wifi } = cfg;
  const rest = Object.fromEntries(PROFILE_KEYS.filter((k) => k in cfg).map((k) => [k, cfg[k]]));     // a whitelist: nothing else gets in
  rest.sshKeys = (cfg.sshKeys || []).map((k) => k.line || k);    // public keys only
  return {
    format: 'lordos-imager-profile', version: 1, name: String(name).slice(0, 64),
    config: { ...rest, user: { name: user?.name || '', fullname: user?.fullname || '', autologin: !!user?.autologin },
      wifi: { enabled: !!wifi?.enabled, ssid: wifi?.ssid || '', hidden: !!wifi?.hidden, open: !!wifi?.open } },
  };
}
export function fromProfile(p, base) {
  if (!p || p.format !== 'lordos-imager-profile' || p.version !== 1 || typeof p.config !== 'object') return null;
  const c = { ...base, ...p.config };
  c.user = { ...base.user, ...(p.config.user || {}) };
  c.wifi = { ...base.wifi, ...(p.config.wifi || {}) };
  c.sshKeys = (Array.isArray(p.config.sshKeys) ? p.config.sshKeys : []).map((k) => v.sshKey(k.line || k)).filter((k) => !k.error);
  return c;
}
