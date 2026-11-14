// Transport SDK media events unchanged. Sessions and sound subscriptions belong
// to a native window; Rust owns playback/recording state and edit policy.
export function mediaServices({ media, userDirs, soundState, thumbRev, events }) {
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
      const value = { id, off, k, recordApp: null };
      sessions.set(k, value);
      owner.retain(`media:${id}`, value, value => {
        sessions.delete(value.k); value.off();
        // cameraRecord also owns the SDK microphone-use indicator. Release it
        // before closing the camera, including a pending permission request.
        if (value.recordApp) Promise.resolve(media.cameraRecord(value.id, value.recordApp, null)).catch(() => {});
        media.close(value.id);
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
    'media.camera': session((app, device, options) => media.camera(app, device, options)),
    'media.cameras': { run: () => media.cameras() },
    'media.view': { run([id, options], owner) { return owned(owner, id) ? media.view(id, options) : false; } },
    'media.photo': { run([id, path, mirror], owner) { return owned(owner, id) ? media.photo(id, path, mirror) : false; } },
    'media.cameraRecord': { async run([id, app, path, options], owner) {
      const value = sessions.get(key(owner, id));
      if (!value) return false;
      if (path) value.recordApp = app;
      const ok = await media.cameraRecord(id, app, path, options);
      if (!owner.alive() || sessions.get(key(owner, id)) !== value) {
        if (ok && path) await media.cameraRecord(id, app, null);
        return false;
      }
      if (!path || !ok) value.recordApp = null;
      return ok;
    } },
    'media.thumbs': { run(args, owner) {
      owner.cleanup(thumbRev.subscribe(() => owner.emit({ kind: 'thumb' })));
      return null;
    } },
    'media.thumb': { run: args => media.thumb(...args) },
    'media.close': { run([id], owner) { owner.release(`media:${id}`); } },
    'media.control': { run([id, cmd, value], owner) { return owned(owner, id) ? media.control(id, cmd, value) : false; } },
    'media.stop': { run([id], owner) { return owned(owner, id) ? media.stop(id) : false; } },
    'media.waveform': { run: args => media.waveform(...args) },
    'media.transcode': { run: args => media.transcode(...args) },
  };
}
