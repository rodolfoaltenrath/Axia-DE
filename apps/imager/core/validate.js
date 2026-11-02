// validate.js — the LordOS Imager's checks, the same rules as the first-run parser on the board
// (server/src/firstrun/firstrun.c, docs/IMAGER.md). Shared by the native app and the desktop app;
// plain JavaScript, no lord.* and no DOM. Each check returns null when the value is fine, or the
// i18n key (imager.err.*) of the problem.

// UTF-8 bytes of a string (QuickJS has no TextEncoder)
export function utf8(s) {
  const out = [];
  for (const ch of String(s)) {
    let c = ch.codePointAt(0);
    if (c < 0x80) out.push(c);
    else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
    else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
  }
  return out;
}
export const hex = (bytes) => bytes.map((b) => b.toString(16).padStart(2, '0')).join('');

export function hostname(s) {
  if (!s) return 'imager.err.hostEmpty';
  if (s.length > 63) return 'imager.err.hostLong';
  if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/.test(s)) return 'imager.err.hostChars';
  if (/^[0-9]+$/.test(s) || s === 'localhost') return 'imager.err.hostChars';
  return null;
}

// "Ana Pérez" → "ana-perez" (a suggestion; the user can change it)
export function suggestHostname(name, device = 'pi') {
  const base = String(name || '').normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase()
    .replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40);
  return base ? `${base}-${device}`.slice(0, 63).replace(/-+$/, '') : `lordos-${device}`;
}

export const RESERVED_USERS = ['root', 'daemon', 'bin', 'sys', 'sync', 'games', 'man', 'lp', 'mail', 'news', 'uucp', 'proxy',
  'www-data', 'backup', 'list', 'irc', 'nobody', 'lordos', 'lordos-greeter', 'messagebus', 'polkitd', 'sshd', 'pipewire',
  'sudo', 'admin', 'wheel', 'users', 'staff', 'adm', 'disk', 'audio', 'video', 'input', 'render', 'plugdev', 'netdev',
  'bluetooth', 'avahi', '_apt', 'lordos-remote', 'operator', 'shadow', 'utmp', 'tty', 'kmem', 'dialout', 'cdrom'];

export function username(s) {
  if (!s) return 'imager.err.userEmpty';
  if (s.length > 32) return 'imager.err.userLong';
  if (!/^[a-z_][a-z0-9_-]*$/.test(s)) return 'imager.err.userChars';
  if (s.startsWith('systemd-') || RESERVED_USERS.includes(s)) return 'imager.err.userReserved';
  return null;
}

// "Ana Pérez" → "ana"
export function suggestUser(full) {
  const first = String(full || '').normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase().trim().split(/\s+/)[0] || '';
  const u = first.replace(/[^a-z0-9_-]/g, '').replace(/^[^a-z_]+/, '').slice(0, 32);
  return username(u) ? '' : u;
}

const BAD_FULLNAME = /[\u0000-\u001f\u007f-\u009f\u2028-\u202e\u2066-\u2069:,=\\]/;
export function fullname(s) {
  if (!s) return null;                                   // optional: the user name is used
  if (utf8(s).length > 128) return 'imager.err.nameLong';
  if (BAD_FULLNAME.test(s) || s !== s.trim() || s.startsWith('-')) return 'imager.err.nameChars';
  return null;
}

// Password: 1..1024 bytes for the hash; the strength is advice, never a rule.
export function password(p, confirm) {
  if (!p) return 'imager.err.passEmpty';
  if (utf8(p).length > 1024) return 'imager.err.passLong';
  if (confirm !== undefined && p !== confirm) return 'imager.err.passMismatch';
  return null;
}

const COMMON = ['password', '12345678', '123456789', 'qwertyui', 'iloveyou', 'raspberry', 'lordos', 'admin123', 'contrasena',
  'contraseña', 'senha123', 'letmein1', 'welcome1', '11111111', '00000000', 'abc12345', 'password1'];
// 0 very weak … 4 strong, with the reasons to show
export function strength(p) {
  if (!p) return { score: 0, hints: [] };
  const hints = [];
  const len = [...p].length;
  let classes = 0;
  if (/[a-z]/.test(p)) classes++;
  if (/[A-Z]/.test(p)) classes++;
  if (/[0-9]/.test(p)) classes++;
  if (/[^A-Za-z0-9]/.test(p)) classes++;
  let score = len >= 16 ? 3 : len >= 12 ? 2 : len >= 8 ? 1 : 0;
  if (classes >= 3) score++;
  if (len < 12) hints.push('imager.hint.longer');
  if (classes < 3) hints.push('imager.hint.mix');
  if (/(.)\1\1/.test(p) || /(0123|1234|2345|3456|4567|5678|6789|abcd|qwer|asdf)/i.test(p)) { score--; hints.push('imager.hint.pattern'); }
  if (COMMON.includes(p.toLowerCase())) { score = 0; hints.unshift('imager.hint.common'); }
  return { score: Math.max(0, Math.min(4, score)), hints };
}

// Wi-Fi: the network name is 1..32 bytes; the WPA2 password 8..63 printable ASCII (or none: open)
export function ssid(s) {
  if (!s) return 'imager.err.ssidEmpty';
  const b = utf8(s);
  if (b.length > 32) return 'imager.err.ssidLong';
  if (b.includes(0)) return 'imager.err.ssidChars';
  return null;
}
export function wifiPassword(p) {
  if (!p) return null;
  if (p.length < 8 || p.length > 63) return 'imager.err.wifiPassLength';
  if (!/^[\x20-\x7e]+$/.test(p)) return 'imager.err.wifiPassChars';
  return null;
}

export function timezone(s) {
  if (!s) return null;
  if (s.length < 3 || s.length > 63 || !/^[A-Z][A-Za-z0-9_+\-/]*$/.test(s) || /\.\.|\/\/|\/$/.test(s)) return 'imager.err.zone';
  return null;
}

// ---- SSH public keys ----

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
export function base64Decode(s) {
  if (!s || s.length % 4) return null;
  const out = [];
  for (let i = 0; i < s.length; i += 4) {
    const v = [];
    let pad = 0;
    for (let k = 0; k < 4; k++) {
      const c = s[i + k];
      if (c === '=') { if (i + 4 !== s.length || k < 2) return null; pad++; v.push(0); continue; }
      if (pad) return null;
      const x = B64.indexOf(c);
      if (x < 0) return null;
      v.push(x);
    }
    const t = (v[0] << 18) | (v[1] << 12) | (v[2] << 6) | v[3];
    const b = [(t >> 16) & 255, (t >> 8) & 255, t & 255];
    out.push(...b.slice(0, 3 - pad));
  }
  return out;
}

export const KEY_TYPES = ['ssh-ed25519', 'sk-ssh-ed25519@openssh.com', 'ecdsa-sha2-nistp256', 'ecdsa-sha2-nistp384',
  'ecdsa-sha2-nistp521', 'sk-ecdsa-sha2-nistp256@openssh.com', 'ssh-rsa'];

// One line of a .pub file → { line, type, comment } (normalized: single spaces, no trailing space) or { error }
export function sshKey(raw) {
  const line = String(raw || '').trim().replace(/\s+/g, ' ');
  if (!line) return { error: 'imager.err.keyEmpty' };
  const parts = line.split(' ');
  const [type, blob] = parts;
  const comment = parts.slice(2).join(' ');
  if (!KEY_TYPES.includes(type)) return { error: type === 'ssh-dss' ? 'imager.err.keyDss' : 'imager.err.keyType' };
  const bytes = base64Decode(blob || '');
  if (!bytes || bytes.length < 4 + type.length) return { error: 'imager.err.keyData' };
  const sl = ((bytes[0] << 24) >>> 0) + (bytes[1] << 16) + (bytes[2] << 8) + bytes[3];
  if (sl !== type.length || String.fromCharCode(...bytes.slice(4, 4 + sl)) !== type) return { error: 'imager.err.keyData' };
  if (type === 'ssh-ed25519' && bytes.length !== 4 + 11 + 4 + 32) return { error: 'imager.err.keyData' };
  if (type === 'ssh-rsa' && bytes.length < 260) return { error: 'imager.err.keyRsaShort' };
  if (utf8(comment).length > 256 || BAD_FULLNAME.test(comment.replace(/[:,=\\]/g, ''))) return { error: 'imager.err.keyComment' };
  const out = comment ? `${type} ${blob} ${comment}` : `${type} ${blob}`;
  if (utf8(out).length >= 4096) return { error: 'imager.err.keyData' };
  return { line: out, type, comment };
}

// Every key line of a pasted text or a .pub file (comments and blank lines skipped)
export function sshKeys(text) {
  const keys = [], errors = [];
  for (const l of String(text || '').split(/\r?\n/)) {
    if (!l.trim() || l.trim().startsWith('#')) continue;
    const k = sshKey(l);
    if (k.error) errors.push(k.error); else if (!keys.some((x) => x.line === k.line)) keys.push(k);
  }
  return { keys: keys.slice(0, 16), errors, tooMany: keys.length > 16 };
}
