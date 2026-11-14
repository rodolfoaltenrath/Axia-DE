// Clipboard I/O only. Rust chooses the destination and Markdown insertion.
export function clipboardServices({ clipboard, fs }) {
  return {
    'clipboard.read': { run: () => ({kind: clipboard.state?.().kind || 'text', text: clipboard.get() || ''}) },
    'clipboard.image': { run([path]) { const data=clipboard.imageData?.();if(!data)return false;fs.write(path,data);return true; } },
  };
}
