// Arquivos is authored in Rust. This file is only the native app-host binding.
import { createElement as h } from 'react';
import { useAppWindow } from '@lordos/sdk';
import { RustFiles, RustFileIcon } from '../../sdk/rustfiles.jsx';
export const Icon = RustFileIcon;
export default function Files() {
  const { args } = useAppWindow();
  return h(RustFiles, { mode: 'browser', args });
}
