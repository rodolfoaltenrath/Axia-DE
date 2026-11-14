// Existing backup APIs keep the session engine, notifications and encrypted
// storage. Transport raw status changes; native Rust chooses every operation.
export function backupServices({ backup, backupStatus, events }) {
  return {
    'backup.watch': { run(args, owner) {
      owner.cleanup(backupStatus.subscribe(() => owner.emit({ kind: 'backup', status: backupStatus.get() })));
      return backupStatus.get();
    } },
    'backup.events': { run: (args, owner) => events(owner.id) },
    'backup.config': { run: () => backup.config() },
    'backup.configure': { run([config]) { backup.configure(config); return backup.config() || config; } },
    'backup.run': { run: () => backup.runNow() },
    'backup.pause': { run: ([on]) => backup.pause(on) },
    'backup.cancel': { run: () => backup.cancel() },
    'backup.list': { run: () => backup.list() },
    'backup.ls': { run: ([id, rel]) => backup.ls(id, rel) },
    'backup.path': { run: ([id]) => backup.path(id) },
    'backup.restore': { run: ([id, rel, options]) => backup.restore(id, rel, options) },
    'backup.remove': { run: ([id]) => backup.remove(id) },
    'backup.unlock': { run: ([pass, create]) => backup.unlock(pass, create) },
    'backup.lock': { run: () => backup.lock() },
  };
}
