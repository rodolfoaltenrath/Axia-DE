// Shared Files service calls and asynchronous effects. No widget renderer lives here.
import { errorText } from './errors.js';
import { archive } from './archive.js';
import { pdfThumb, fontSample } from './doc.js';
import { media, thumbRev } from './media.js';
import { lan } from './lan.js';
import { disks } from './disks.js';

export function filesRequest(value) {
  const result = lord.files.request(JSON.stringify(value));
  if (result?.error) throw new Error(errorText(result.error));
  return result;
}
// These are integrations with existing desktop services, selected by Rust.
// All requests/results go back through the owning Rust window handle.
export async function hostEffect(effect) {
  switch (effect.kind) {
    case 'diskMount':
    case 'diskRelease': {
      const { job } = filesRequest({ op: 'volumeStart', action: effect.kind === 'diskMount' ? 'mount' : 'release', device: effect.device });
      for (;;) {
        await new Promise((resolve) => setTimeout(resolve, 80));
        const result = filesRequest({ op: 'volumePoll', job });
        if (!result.done) continue;
        if (result.failure) throw new Error(errorText(result.failure));
        return { result: result.result, disks: disks.list() };
      }
    }
    case 'videoThumbnail': {
      if (!media.available()) return { result: null };
      // The confined worker signals completion. Avoid repeatedly stat'ing
      // videos on slow USB/network mounts from the UI's polling timer.
      return new Promise((resolve) => {
        let timer = 0, off = null;
        const finish = (path) => { clearTimeout(timer); off?.(); resolve({ result: path }); };
        const check = () => { const path = media.thumb(effect.path, 320); if (path) finish(path); };
        off = thumbRev.subscribe(check);
        timer = setTimeout(() => finish(null), 15000);
        check();
      });
    }
    case 'pdfThumbnail': { const result = await pdfThumb(effect.path, { width: 128, height: 176 }); return { result: { id: result.image, w: result.w, h: result.h } }; }
    case 'networkScan': {
      if (!lan.available()) return { result: { state: 'no-connector', hosts: [] } };
      lan.start();
      const deadline = lord.now() + 30000;
      let state = lan.poll();
      while (state.running && lord.now() < deadline) { await new Promise((resolve) => setTimeout(resolve, 500)); state = lan.poll(); }
      return { result: state };
    }
    case 'archiveList': return { result: await archive.list(effect.path) };
    case 'archiveExtract': return { result: await archive.extract(effect.path, effect.destination) };
    case 'archiveCreate': return { result: await archive.create(effect.paths, effect.destination) };
    case 'archivePreview':
    case 'archiveOpen': {
      const destination = `${lord.fs.home()}/.cache/axia/files/extract/${Date.now()}`;
      lord.fs.mkdir('.cache'); lord.fs.mkdir('.cache/axia'); lord.fs.mkdir('.cache/axia/files'); lord.fs.mkdir('.cache/axia/files/extract');
      await archive.extract(effect.path, destination, { only: [effect.entry] });
      return { path: `${destination}/${effect.entry}` };
    }
    case 'documentPreview': { const result = effect.format === 'pdf' ? await pdfThumb(effect.path, { width: 600, height: 500 }) : await fontSample(effect.path, { width: 600, lines: ['Axia', 'Aa Bb Cc 0123456789'] }); return { result: { id: result.image, w: result.w, h: result.h } }; }
    case 'mediaPreview': return { result: media.play('files', effect.path, { audio: 1 }) };
    default: return null;
  }
}
