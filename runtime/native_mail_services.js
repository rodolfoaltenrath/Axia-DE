// Mail I/O for the native window: the mail helper, the accounts file and keyring, and files.
// Rust owns what is asked and how it is shown. Descriptors are leased to the window.
// Events and images use the shared pim.* services.
export function mailServices({ api, accounts, config, status, track, closeFd }) {
  const files = new Map();
  const watching = new Set();
  let sequence = 0;
  const reply = async work => {
    try { return { value: await work() }; }
    catch (error) { return { error: { code: String(error?.code || 'error'), message: String(error?.message || error) } }; }
  };
  function lease(owner, fd) {
    const id = ++sequence;
    files.set(id, { fd, owner: owner.id });
    owner.retain(`mail-file:${id}`, fd, value => { files.delete(id); try { closeFd(value); } catch (error) { /* already closed */ } });
    return id;
  }
  return {
    // The facts the window starts from; later changes of accounts or status arrive as events.
    'mail.snapshot': { run(args, owner) {
      track();
      if (!watching.has(owner.id)) {
        watching.add(owner.id);
        const stop = status.subscribe(() => owner.emit({ ev: 'mailState' }));
        owner.cleanup(() => { watching.delete(owner.id); stop(); });
      }
      const isolated = api.isolated();
      return { available: api.available(), isolated, config: config.get(), status: status.get(), cacheDir: isolated ? '' : api.cacheDir() };
    } },
    // A file is { file } (a lease of this window) or { path } (Mail in the session).
    'mail.request': { run: ([op, args, list = []], owner) => reply(() => {
      const chosen = list.map(item => {
        if (typeof item?.path === 'string') return item.path;
        const file = files.get(item?.file);
        if (!file || file.owner !== owner.id) throw Object.assign(new Error('file'), { code: 'file' });
        return { fd: file.fd };
      });
      return api.request(op, args, { files: chosen });
    }) },
    // The system's file dialog: this process receives only the chosen file.
    'mail.pickFile': {
      run(args, owner) {
        const picked = api.pick();
        if (!picked || picked.fd < 0) return null;
        const size = api.fdSize(picked.fd);
        return { file: lease(owner, picked.fd), name: String(picked.name || ''), size: size > 0 ? size : 0 };
      },
      dispose(result, owner) { if (result?.file) owner.release(`mail-file:${result.file}`); },
    },
    // A cached attachment the server opens for the isolated app (a forwarded message's files).
    'mail.attachmentFile': {
      run([relative], owner) {
        const fd = api.attachmentFd(relative);
        return fd >= 0 ? { file: lease(owner, fd) } : null;
      },
      dispose(result, owner) { if (result?.file) owner.release(`mail-file:${result.file}`); },
    },
    'mail.releaseFile': { run([id], owner) { owner.release(`mail-file:${id}`); return null; } },
    'mail.saveConfig': { run([next]) { config.save(next); return null; } },
    'mail.account': { run: ([action, ...args]) => reply(async () => {
      if (!['add', 'update', 'remove', 'setPassword', 'trustCertificate'].includes(action)) throw new Error('app.invalidRequest');
      return (await accounts[action](...args)) ?? null;
    }) },
    'mail.allowImages': { run([urls]) { api.allowImages(urls); return null; } },
    'mail.openAttachment': { run([relative, name]) { api.openAttachment(relative, name); return null; } },
    // Isolated Mail: a read-only descriptor of the cached attachment, then the system's save dialog.
    'mail.saveAttachment': { async run([relative, name]) {
      const fd = api.attachmentFd(relative);
      if (fd < 0) return false;
      let data;
      try { data = api.fdRead(fd); } finally { closeFd(fd); }
      return !!(await api.saveFile(name, data));
    } },
    'mail.reveal': { run([path]) { api.reveal(path); return null; } },
  };
}
