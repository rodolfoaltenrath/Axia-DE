// PIM I/O for native windows. Rust selects operations and owns the card policy.
// Files and images are retained by the window lease, including late replies.
export function pimServices({ request, onEvent, pick, close, releaseImage, events }) {
  const files = new Map();
  let sequence = 0;
  return {
    'pim.request': { run([op, args, ids = []], owner) {
      const chosen = ids.map(id => {
        const file = files.get(id);
        if (!file || file.owner !== owner.id) throw new Error('file');
        return { fd: file.fd };
      });
      return request(op, args, { files: chosen });
    } },
    'pim.watch': { run([names], owner) {
      owner.cleanup(onEvent(event => { if (!Array.isArray(names) || names.includes(event.ev)) owner.emit(event); }));
      return null;
    } },
    'pim.events': { run: (args, owner) => events(owner.id) },
    'pim.filePick': {
      run(args, owner) {
        const file = pick();
        if (!file || file.fd < 0) return null;
        const id = ++sequence;
        files.set(id, { fd: file.fd, owner: owner.id });
        owner.retain(`pim-file:${id}`, file.fd, fd => { files.delete(id); try { close(fd); } catch (error) { /* already closed */ } });
        return { file: id, name: String(file.name || '') };
      },
      dispose(result, owner) { if (result?.file) owner.release(`pim-file:${result.file}`); },
    },
    'pim.fileRelease': { run([id], owner) { owner.release(`pim-file:${id}`); return null; } },
    'pim.image': {
      async run([op, args], owner) {
        const result = await request(op, args);
        if (result.image > 0) owner.retain(`pim-image:${result.image}`, result.image, releaseImage);
        return result;
      },
      dispose(result, owner) { if (result?.image) owner.release(`pim-image:${result.image}`); },
    },
    'pim.imageRelease': { run([id], owner) { owner.release(`pim-image:${id}`); return null; } },
  };
}
