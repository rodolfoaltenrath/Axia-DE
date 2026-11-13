// I/O adapters only. Rust chooses files, render parameters and mutation order.
import { documentImageService } from './native_doc_services.js';
export function fontServices({ fontInfo, fontSample, fontGrid, releaseImage, list }) {
  const image = fn => documentImageService(fn, releaseImage);
  return {
    'fonts.list': { run: ([rescan]) => list({ rescan: !!rescan }) },
    'fonts.info': { run: args => fontInfo(...args) },
    'fonts.sample': image(fontSample),
    'fonts.grid': image(fontGrid),
    'fonts.release': { run: ([ids], owner) => { for (const id of ids) owner.release(`image:${id}`); } },
  };
}
