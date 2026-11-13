// I/O adapters only. Rust chooses files, render parameters and mutation order.
export function fontServices({ fs, fontInfo, fontSample, fontGrid, releaseImage, list }) {
  const image = fn => ({
    async run(args, owner) {
      const result = await fn(...args);
      owner.retain(`image:${result.image}`, result.image, releaseImage);
      return result;
    },
    dispose(result, owner) { if (result?.image) owner.release(`image:${result.image}`); },
  });
  return {
    'fonts.list': { run: ([rescan]) => list({ rescan: !!rescan }) },
    'fonts.info': { run: args => fontInfo(...args) },
    'fonts.sample': image(fontSample),
    'fonts.grid': image(fontGrid),
    'fonts.release': { run: ([ids], owner) => { for (const id of ids) owner.release(`image:${id}`); } },
    'fs.stat': { run: ([path]) => fs.stat(path) },
    'fs.mkdir': { run: ([path]) => fs.mkdir(path) },
    'fs.copy': { run: ([from, to]) => fs.copy(from, to) },
    'fs.jobs': { run: () => fs.jobs() },
    'fs.forgetJob': { run: ([id]) => fs.forgetJob(id) },
    'fs.trash': { run: ([path]) => fs.trash(path) },
  };
}
