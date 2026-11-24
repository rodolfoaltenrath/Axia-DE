// Transport for the compiled Passwords app, which runs in its own isolated process
// (axia-server --app passwords). No UI and no vault logic: everything decrypted is
// handled by axia-vault, sandboxed and without network. This lends the Rust window the
// vault's requests and events, the files the system dialog opened and the secret clipboard.
// Files are descriptors the dialog opened, held here and named to Rust by an id; a vault
// request takes them (they are used up) and closing a window closes the ones left.
// See docs/PASSWORDS.md.
export function vaultServices({ vaultRequest, onVaultEvent, requestPermission, clipboard, events, appId = 'passwords' }) {
  // Picked files: id → { fd, owner }.
  const files = new Map();
  let fileSeq = 0;
  const closeFd = fd => { try { lord.broker.fdClose(fd); } catch (e) { /* already closed */ } };

  // FIDO2: the USB permission, then the key is lent to axia-vault for this request.
  async function fido() {
    if (!(await requestPermission(appId, 'usb'))) throw new Error('fido-denied');
    const attached = lord.vault.fidoAttach(appId);
    if (!attached.ok) throw new Error(attached.error || 'fido-denied');
  }
  function pick(save, suggest) {
    let result = null;
    try { result = lord.broker.pick(save ? 1 : 0, suggest || ''); } catch (e) { /* cancelled or denied */ }
    return result && result.fd >= 0 ? result : null;
  }

  return {
    // axia-vault's events (locking, changes, the key's touch) into the window's queue, until it closes
    'vault.watch': { run(args, owner) {
      owner.cleanup(onVaultEvent(event => owner.emit({ kind: 'vault', ...event })));
      return null;
    } },
    // the window's queue: the vault's events and its menus' commands
    'vault.events': { run: (args, owner) => events(owner.id) },
    'vault.request': { async run([op, args, ids, needsFido], owner) {
      const fds = [];
      for (const id of ids || []) {
        const file = files.get(id);
        if (!file || file.owner !== owner.id) throw new Error('file');
        files.delete(id);            // the request takes the descriptor
        fds.push(file.fd);
      }
      try {
        if (needsFido) await fido();
        return await vaultRequest(String(op), args || {}, fds.length ? { fds } : undefined);
      } catch (error) {
        throw new Error(error?.code || error?.message || 'other');
      }
    } },
    'file.pick': { run([save, suggest], owner) {
      const result = pick(save, suggest);
      if (!result) return null;
      const id = ++fileSeq;
      files.set(id, { fd: result.fd, owner: owner.id });
      owner.cleanup(() => { const file = files.get(id); if (file) { files.delete(id); closeFd(file.fd); } });
      return { id, name: String(result.name || '') };
    } },
    // Save a text (the recovery key): the dialog, then the file. null when cancelled.
    'file.save': { run([suggest, text]) {
      const result = pick(true, suggest);
      if (!result) return null;
      const ok = lord.broker.fdWrite(result.fd, String(text));
      closeFd(result.fd);
      if (!ok) throw new Error('file');
      return true;
    } },
    // A secret: axia-server marks it sensitive and clears it after `secs` if unchanged.
    'clipboard.secret': { run([value, secs]) { lord.clipboard.set(String(value), { secure: true, clearAfter: Number(secs) || 30 }); return null; } },
    'clipboard.text': { run([value]) { clipboard.set(String(value), { origin: appId }); return null; } },
  };
}
