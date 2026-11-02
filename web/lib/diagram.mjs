// Diagrams from content JSON, rendered as accessible HTML boxes and arrows (never images of
// text). Two kinds:
//   stack: layers drawn top to bottom;
//   flow:  nodes left to right (top to bottom on a phone), edges in order.
// Each node has a tone (ours, reused, kernel, user, allow, deny) shown by colour AND by a small
// icon with a text label, and the legend lists the tones used.
import { esc } from './md.mjs';

const loc = (v, l) => (v == null ? '' : typeof v === 'string' ? v : v[l] ?? v.en ?? '');

const TONE_ICON = {
  ours: '<path d="M4 17l2-9 4 4 2-6 2 6 4-4 2 9z"/>',                 // crown
  reused: '<path d="M4 7h16M4 12h16M4 17h16"/>',                      // stack of parts
  kernel: '<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3"/>', // chip
  user: '<circle cx="12" cy="8" r="4"/><path d="M4 21c1-4 4-6 8-6s7 2 8 6"/>', // person
  allow: '<path d="M5 12l5 5 9-10"/>',                                 // check
  deny: '<path d="M6 6l12 12M18 6L6 18"/>',                            // cross
};
const toneIcon = (tone) => `<svg class="tone-ico" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${TONE_ICON[tone] || TONE_ICON.reused}</svg>`;

let seq = 0;

/**
 * @param {object} d  diagram {id, kind, title, nodes[{id,label,sub,tone}], edges[{from,to,label}], caption}
 * @param {string} l  language
 * @param {object} t  strings: {tones:{ours,…}, legend, then, connections}
 */
export function diagram(d, l, t) {
  const id = `dia-${esc(d.id || ++seq)}`;
  const kind = d.kind === 'stack' ? 'stack' : 'flow';
  const nodes = d.nodes || [];
  const edges = d.edges || [];
  const byId = Object.fromEntries(nodes.map((n) => [n.id, n]));
  const toneName = (tone) => (t.tones && t.tones[tone]) || tone;

  // edges between neighbours are drawn as arrows; the rest are listed under the diagram
  const between = new Map();
  const extra = [];
  for (const e of edges) {
    const a = nodes.findIndex((n) => n.id === e.from), b = nodes.findIndex((n) => n.id === e.to);
    if (a >= 0 && b === a + 1 && !between.has(a)) between.set(a, e);
    else if (a >= 0 && b >= 0) extra.push(e);
  }

  const items = nodes.map((n, i) => {
    const tone = n.tone || 'reused';
    const e = between.get(i);
    const sub = loc(n.sub, l);
    const box = `<div class="dn tone-${esc(tone)}"><span class="dn-tone">${toneIcon(tone)}${esc(toneName(tone))}</span><strong class="dn-label">${esc(loc(n.label, l))}</strong>${sub ? `<span class="dn-sub">${esc(sub)}</span>` : ''}</div>`;
    const arrow = i < nodes.length - 1
      ? `<div class="de${e ? '' : ' de-plain'}"><span class="de-line" aria-hidden="true"></span>${e && loc(e.label, l) ? `<span class="de-label"><span class="vh">${esc(t.then || '→')} </span>${esc(loc(e.label, l))}</span>` : ''}</div>`
      : '';
    return `<li>${box}${arrow}</li>`;
  }).join('');

  const tones = [...new Set(nodes.map((n) => n.tone || 'reused'))];
  const legend = tones.length > 1 || tones[0] !== 'reused'
    ? `<p class="dia-legend"><span class="vh">${esc(t.legend || 'Legend')}: </span>${tones.map((tn) => `<span class="lg tone-${esc(tn)}">${toneIcon(tn)}${esc(toneName(tn))}</span>`).join('')}</p>`
    : '';
  const more = extra.length
    ? `<ul class="dia-edges" aria-label="${esc(t.connections || 'Connections')}">${extra.map((e) => `<li><strong>${esc(loc(byId[e.from].label, l))}</strong> <span aria-hidden="true">→</span><span class="vh">${esc(t.then || 'to')}</span> <strong>${esc(loc(byId[e.to].label, l))}</strong>${loc(e.label, l) ? `: ${esc(loc(e.label, l))}` : ''}</li>`).join('')}</ul>`
    : '';
  const title = loc(d.title, l);
  const caption = loc(d.caption, l);
  return `<figure class="dia dia-${kind}" id="${id}"${title ? ` aria-labelledby="${id}-t"` : ''}>
${title ? `<p class="dia-title" id="${id}-t">${esc(title)}</p>` : ''}
<ol class="dia-nodes">${items}</ol>
${more}${legend}${caption ? `<figcaption class="dia-cap">${esc(caption)}</figcaption>` : ''}
</figure>`;
}
