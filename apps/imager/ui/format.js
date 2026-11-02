// format.js — sizes, speeds and times for the LordOS Imager. Card and stick sizes are shown in
// decimal units, as they are printed on them (a "32 GB" card holds about 31.9 × 10⁹ bytes).
import { formatNumber, t } from '@lordos/sdk';

export function formatSize(n) {
  if (!Number.isFinite(n) || n <= 0) return '—';
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (n >= 1000 && i < u.length - 1) { n /= 1000; i++; }
  return `${formatNumber(n, n < 10 && i ? 1 : 0)} ${u[i]}`;
}
export const formatSpeed = (bps) => (bps > 0 ? `${formatSize(bps)}/s` : '—');
export function formatEta(s) {
  if (s == null || !Number.isFinite(s)) return t('imager.write.etaUnknown');
  s = Math.max(0, Math.round(s));
  if (s < 60) return t('imager.write.etaSeconds', { n: s });
  const m = Math.floor(s / 60), r = s % 60;
  return m < 60 ? t('imager.write.etaMinutes', { m, s: String(r).padStart(2, '0') }) : t('imager.write.etaHours', { h: Math.floor(m / 60), m: m % 60 });
}
