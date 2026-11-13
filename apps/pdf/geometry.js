// Page geometry is Rust-owned (geometry.rs). Keep each layout's positions in
// its native owner so scrolling sends two numbers, not the entire page list.
import { createPdfModel } from './model.js';
let shared;
const request = value => (shared ||= createPdfModel())(value);
const layouts = new WeakMap();
export const shown = (size, rot) => request({ cmd: 'shown', size, rot });
export const shownPages = (sizes, rot) => request({ cmd: 'shownMany', sizes, rot });
export const toView = (x, y, size, rot, scale) => request({ cmd: 'toView', point: [x, y], size, rot, scale });
export const toPage = (x, y, size, rot, scale) => request({ cmd: 'toPage', point: [x, y], size, rot, scale });
export function rectToView(rect, size, rot, scale) {
  const value = request({ cmd: 'rect', rect, size, rot, scale });
  return { left: value.left, top: value.top, width: value.width, height: value.height };
}
export function layoutPages(sizes, rot, scale, gap) {
  const owner = createPdfModel();
  const value = owner({ cmd: 'layout', sizes, rot, scale, gap });
  layouts.set(value.tops, owner);
  return { tops: value.tops, height: value.height, width: value.width };
}
export function visibleRange(tops, sizes, rot, scale, y0, y1) {
  const owner = layouts.get(tops);
  return owner ? owner({ cmd: 'visible', y0, y1 }) : request({ cmd: 'visible', tops, y0, y1 });
}
const viewRequest = (layout, op, facts) => {
  const owner = layouts.get(layout.tops);
  if (!owner) throw new Error('pdf.invalidNavigation');
  return owner({ cmd: 'view', op, ...facts });
};
export const viewport = (layout, facts) => viewRequest(layout, 'viewport', facts);
export const navigate = (layout, page, rect) => viewRequest(layout, 'goto', { page, rect });
export const frameFor = (layout, page) => viewRequest(layout, 'frame', { page });
export const hitPoint = (layout, x, y) => viewRequest(layout, 'hit', { x, y });
