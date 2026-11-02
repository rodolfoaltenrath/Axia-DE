// errors.js — translated error and status messages from stable codes.
//
// The server never sends prose to the UI: an error is a *code line* (server/src/errcode.h),
//   "pkg.removeEssential name=dpkg"
// that is shown with t('errors.pkg.removeEssential', { name: 'dpkg' }).
//
//   errorText(e)          Error, code line or { code, params } → text in the user's language
//   statusText(line)      same for OK/LOG statuses (t('status.<code>'))
//   codeError(line)       new Error(errorText(line)) with .code and .params
//   parseCode(line)       { code, params } or null
//
// Text that is not a code line, or a code without a translation, is shown as is (the output of a
// program, an older server). Two parameters are special: `errno` (ENOENT…) becomes its
// translated description (errors.errno.ENOENT) and `reason` is a nested code line.
// Errors thrown by lord.* (server/src/ui/js_err.h) already carry a translated message, `code`
// and `params`, through globalThis.__lordErrorText.
import { tOptional } from './i18n.js';

const CODE_RE = /^([a-z][a-zA-Z0-9]*(?:\.[a-zA-Z0-9]+)+)(?= |$)/;

const decode = (v) => { try { return decodeURIComponent(v); } catch (e) { return v; } };

export function parseCode(line) {
  if (typeof line !== 'string') return null;
  const m = CODE_RE.exec(line);
  if (!m) return null;
  const params = {};
  for (const part of line.slice(m[1].length).split(' ')) {
    const eq = part.indexOf('=');
    if (eq > 0) params[part.slice(0, eq)] = decode(part.slice(eq + 1));
  }
  return { code: m[1], params };
}

function translate(ns, code, params = {}) {
  const p = { ...params };
  if (p.errno) p.errno = tOptional(`errors.errno.${p.errno}`) ?? p.errno;
  if (p.reason) p.reason = errorText(p.reason);
  return tOptional(`${ns}.${code}`, p);
}

function textOf(ns, x) {
  if (x == null) return '';
  if (typeof x === 'object') {
    if (x.code) {
      const s = translate(ns, x.code, x.params);
      if (s != null) return s;
    }
    if (x.message != null) return textOf(ns, String(x.message));
    return String(x);
  }
  const line = String(x);
  const p = parseCode(line);
  return (p && translate(ns, p.code, p.params)) ?? line;
}

export const errorText = (x) => textOf('errors', x);
export const statusText = (x) => textOf('status', x);

export function codeError(line) {
  const p = parseCode(String(line ?? ''));
  const e = new Error(errorText(line));
  if (p) { e.code = p.code; e.params = p.params; }
  return e;
}

// For lord.* exceptions (js_err.h): the message in the user's language, or null to keep the line.
globalThis.__lordErrorText = (code, params) => translate('errors', code, params || {}) ?? null;
