// The helper owns I/O; the native model retains thumbnail and bookmark policy.
import { createPdfModel } from './model.js';
let sequence = 0;

export class SidebarModel {
  constructor() { this.request = createPdfModel(); this.id = ++sequence; }
  call(op, facts = {}) { return this.request({ cmd: 'sidebar', op, ...facts }); }
  configure(sizes, rot) { this.call('configure', { sizes, rot }); }
  viewport(height, page) { return this.call('viewport', { height, page }); }
  scroll(delta) { return this.call('scroll', { delta }); }
  outline(items) { return this.call('outline', { items }); }
  toggle(key) { return this.call('toggle', { key }); }
  pick(key) { return this.call('pick', { key }); }
  tree(facts) { return this.call('tree', facts); }
}
