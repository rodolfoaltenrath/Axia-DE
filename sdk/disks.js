// disks.js — disks and partitions (Disks app; see docs/SYSTEM.md, "Disks").
//
//   const [list, reload] = useDisks()       // updates itself on mount, unmount or plug-in
//   disks.list()  disks.tools()             // lord.disks, read-only and without root
//
// Files mounts/unmounts/ejects through the Rust UDisks2 adapter. The Disks app's
// legacy admin actions retain the platform's existing availability policy.
// Change notifications (mountinfo and udev, no polling) are only active while
// someone uses useDisks().
import { useEffect, useState } from 'react';
import { wm } from './windows.js';

const listeners = new Set();
let timer = null;

globalThis.__lordDisks = () => {
  // udev events come in bursts: coalesce them into one
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = null;
    for (const fn of listeners) { try { fn(); } catch (e) { console.log(`disks: ${e}`); } }
  }, 150);
};

const api = () => (typeof lord !== 'undefined' && lord.disks ? lord.disks : null);

export const disks = {
  available: () => !!api(),
  list: () => (api() ? api().list() || [] : []),
  tools: () => (api() ? api().tools() || {} : {}),
  watch(fn) {
    listeners.add(fn);
    if (listeners.size === 1) api()?.watch(true);
    return () => {
      listeners.delete(fn);
      if (!listeners.size) api()?.watch(false);
    };
  },
};

// Fedora provides drivers via the kernel/RPMs. The volume backend reports missing
// support; the old distribution's store marker must not block ordinary USB disks.
export const COMMERCIAL_FORMATS = ['vfat', 'exfat', 'ntfs', 'hfsplus', 'apfs'];
export const diskFormatsOn = () => true;
export const needsDiskFormats = () => false;
export const openDiskFormats = () => wm.open('disks');

export function useDisks() {
  const [list, setList] = useState(disks.list);
  useEffect(() => disks.watch(() => setList(disks.list())), []);
  return [list, () => setList(disks.list())];
}
