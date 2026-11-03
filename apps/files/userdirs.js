// Resolve sidebar folders without evaluating the shell-like XDG configuration.
// An explicit $HOME disables a place; configured paths override guessed names.
const PLACES = [
  ['DOWNLOAD', 'downloads', ['Downloads', 'Descargas', 'Transferências']],
  ['DOCUMENTS', 'documents', ['Documentos', 'Documents']],
  ['DESKTOP', 'desktop', ['Área de trabalho', 'Área de Trabalho', 'Desktop', 'Escritorio']],
  ['PICTURES', 'pictures', ['Imagens', 'Pictures', 'Imágenes']],
  ['MUSIC', 'music', ['Músicas', 'Música', 'Music']],
  ['VIDEOS', 'videos', ['Vídeos', 'Videos']],
];

function absolute(path) {
  if (!path.startsWith('/') || /[\0\r\n]/.test(path)) return null;
  // Let the filesystem resolve links and ".."; lexical collapsing could change
  // the destination of a configured path through a symbolic link.
  return path.replace(/\/+$/, '') || '/';
}

export function resolveUserDirs(home, config, isDirectory) {
  const configured = new Map();
  for (const line of config.split('\n')) {
    const m = /^\s*XDG_([A-Z]+)_DIR\s*=\s*"((?:\\.|[^"\\])*)"\s*(?:#.*)?$/.exec(line);
    if (!m) continue;
    // Only the standard HOME prefix expands. Escaped dollars remain literal.
    const raw = m[2].replace(/^\$HOME(?=\/|$)|^\$\{HOME\}(?=\/|$)/, () => home);
    configured.set(m[1], absolute(raw.replace(/\\(["$`\\])/g, '$1')));
  }
  const out = [];
  for (const [key, label, names] of PLACES) {
    const path = configured.has(key) ? configured.get(key)
      : names.map((name) => home + '/' + name).find(isDirectory);
    if (path && path !== home && isDirectory(path)) out.push({ label, path });
  }
  return out;
}
