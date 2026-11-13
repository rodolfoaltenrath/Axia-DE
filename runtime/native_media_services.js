// Transport SDK media events unchanged. Sessions and sound subscriptions belong
// to a native window; Rust owns playback/recording state and edit policy.
export function mediaServices({ media, userDirs, soundState, events }) {
  const sessions = new Map();
  const key = (owner, id) => `${owner.id}:${id}`;
  const owned = (owner, id) => sessions.has(key(owner, id));
  const session = start => ({
    async run(args, owner) {
      const id = await start(...args);
      if (!id) return 0;
      if (!owner.alive()) { media.close(id); return 0; }
      const off = media.on(id, (type, data) => owner.emit({ kind: 'media', id, type, data }));
      const k = key(owner, id);
      sessions.set(k, id);
      owner.retain(`media:${id}`, { id, off, k }, value => {
        sessions.delete(value.k); value.off(); media.close(value.id);
      });
      return id;
    },
    dispose(id, owner) { if (id) owner.release(`media:${id}`); },
  });
  return {
    'media.dirs': { run: () => userDirs() },
    'media.sound': { run(args, owner) {
      owner.cleanup(soundState.subscribe(() => owner.emit({ kind: 'sound', data: soundState.get() })));
      return soundState.get();
    } },
    'media.events': { run: (args, owner) => events(owner.id) },
    'media.record': session((app, path, options) => media.audioRecord(app, path, options)),
    'media.play': session((app, path, options) => media.play(app, path, options)),
    'media.close': { run([id], owner) { owner.release(`media:${id}`); } },
    'media.control': { run([id, cmd, value], owner) { return owned(owner, id) ? media.control(id, cmd, value) : false; } },
    'media.stop': { run([id], owner) { return owned(owner, id) ? media.stop(id) : false; } },
    'media.waveform': { run: args => media.waveform(...args) },
    'media.transcode': { run: args => media.transcode(...args) },
  };
}
