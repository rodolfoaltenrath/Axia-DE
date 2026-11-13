// Shared confined filesystem I/O. Applications choose paths and mutation order.
export function fileServices(fs) {
  return {
    'fs.stat': { run: ([path]) => fs.stat(path) },
    'fs.list': { run: ([path]) => fs.list(path) },
    'fs.rename': { run: ([from, to]) => fs.rename(from, to) },
    'fs.mkdir': { run: ([path]) => fs.mkdir(path) },
    'fs.copy': { run: ([from, to]) => fs.copy(from, to) },
    'fs.jobs': { run: () => fs.jobs() },
    'fs.forgetJob': { run: ([id]) => fs.forgetJob(id) },
    'fs.trash': { run: ([path]) => fs.trash(path) },
  };
}
