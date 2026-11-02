// md.js — what Mail sends: the body is written as markdown (in rich-text mode the toolbar adds
// the marks for you and a preview shows the result), and goes out as text/plain plus a simple
// text/html made here. The HTML is ours, built from parsed markdown: nothing the user types is
// passed through as raw HTML.
import { parseMarkdown, parseInline } from '@lordos/sdk';

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const safeUrl = (u) => (/^(https?:|mailto:)/i.test(u) ? u : '#');

function inlineHtml(text) {
  return parseInline(text).map((r) => {
    if (r.image) return esc(r.alt || '');
    let s = esc(r.text).replace(/\n/g, '<br>');
    if (r.code) s = `<code style="font-family:monospace;background:#f2f2f2;padding:0 3px">${s}</code>`;
    if (r.bold) s = `<b>${s}</b>`;
    if (r.italic) s = `<i>${s}</i>`;
    if (r.strike) s = `<s>${s}</s>`;
    if (r.link) s = `<a href="${esc(safeUrl(r.link))}">${s}</a>`;
    return s;
  }).join('');
}

function blocksHtml(blocks) {
  const out = [];
  for (const b of blocks) {
    if (b.type === 'heading') out.push(`<h${Math.min(3, b.level)}>${inlineHtml(b.text)}</h${Math.min(3, b.level)}>`);
    else if (b.type === 'paragraph') out.push(`<p style="margin:0 0 10px 0">${inlineHtml(b.text)}</p>`);
    else if (b.type === 'quote') out.push(`<blockquote style="margin:0 0 10px 0;padding-left:10px;border-left:3px solid #ccc;color:#555">${blocksHtml(b.blocks)}</blockquote>`);
    else if (b.type === 'code') out.push(`<pre style="font-family:monospace;background:#f5f5f5;padding:8px">${esc(b.text)}</pre>`);
    else if (b.type === 'hr') out.push('<hr>');
    else if (b.type === 'list') {
      const ordered = b.items[0]?.ordered;
      out.push(ordered ? '<ol>' : '<ul>');
      for (const it of b.items) {
        const box = it.task === null || it.task === undefined ? '' : it.task ? '☑ ' : '☐ ';
        out.push(`<li style="margin-left:${it.depth * 16}px">${box}${inlineHtml(it.text)}</li>`);
      }
      out.push(ordered ? '</ol>' : '</ul>');
    } else if (b.type === 'table') {
      out.push('<table border="1" cellpadding="4" style="border-collapse:collapse"><tr>');
      out.push(b.header.map((c) => `<th>${inlineHtml(c)}</th>`).join(''));
      out.push('</tr>');
      for (const r of b.rows) out.push(`<tr>${r.map((c) => `<td>${inlineHtml(c)}</td>`).join('')}</tr>`);
      out.push('</table>');
    }
  }
  return out.join('\n');
}

export function mdToHtml(src) {
  return `<div style="font-family:Inter,Arial,sans-serif;font-size:14px;line-height:1.45">${blocksHtml(parseMarkdown(src))}</div>`;
}

// The text/plain part: the markdown stays readable; links become "text <url>".
export function mdToText(src) {
  return String(src).replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (m, a, u) => (a === u ? u : `${a} <${u}>`)).replace(/\*\*(.+?)\*\*/g, '*$1*');
}

export const quote = (text) => String(text || '').replace(/\r\n/g, '\n').split('\n').map((l) => (l.startsWith('>') ? `>${l}` : `> ${l}`)).join('\n');
